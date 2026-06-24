use crate::model::export_pending::{ExportPendingRecord, ExportStatus};
use crate::{RuntimeState, UserIndexEvent, UserToDelete, mutate_state, read_state};
use constants::{MINUTE_IN_MS, SECOND_IN_MS};
use ic_cdk::call::RejectCode;
use ic_cdk_timers::TimerId;
use std::cell::Cell;
use std::time::Duration;
use tracing::{error, trace, warn};
use types::{C2CError, CanisterId, Empty, Milliseconds, UserId};

thread_local! {
    static TIMER_ID: Cell<Option<TimerId>> = Cell::default();
    static PARKED_TIMER_ID: Cell<Option<TimerId>> = Cell::default();
}

/// Bounded fast-retry backoff (unchanged P2 window).
const FAST_RETRY_INTERVAL_MS: Milliseconds = 30 * SECOND_IN_MS;
/// Slow self-healing drain cadence for the durable retryable set.
const PARK_RETRY_INTERVAL_MS: Milliseconds = 5 * MINUTE_IN_MS;

/// Retry limit of the bounded fast-retry window. At this many attempts the item
/// is moved out of the in-memory queue into the DURABLE parked set (G's fix —
/// no silent drop). Smaller under `test_mode` so the parked transition is
/// reachable deterministically in PocketIC.
fn park_threshold(test_mode: bool) -> u32 {
    if test_mode { 3 } else { 50 }
}

/// Emit a `warn!` once a retryable failure has persisted past this attempt count.
/// Production observability discipline (G): this is NOT `test_mode`-dependent —
/// the only sanctioned `test_mode` branch is `park_threshold`. Tests that need to
/// observe the terminal state assert the `error!` park log, not a lowered warn.
const WARN_THRESHOLD: u32 = 10;

pub(crate) fn start_job_if_required(state: &RuntimeState, delay: Option<Milliseconds>) -> bool {
    if TIMER_ID.get().is_none() && !state.data.users_to_delete_queue.is_empty() {
        let timer_id = ic_cdk_timers::set_timer(Duration::from_millis(delay.unwrap_or_default()), run);
        TIMER_ID.set(Some(timer_id));
        true
    } else {
        false
    }
}

/// Periodic drain of the DURABLE retryable set (G option (c): resumable retry, so
/// deletion self-heals once the receipts canister recovers OR once a failing
/// uninstall starts succeeding). Drains BOTH `Parked` and `ExportedUninstallPending`.
pub(crate) fn start_parked_retry_job_if_required(state: &RuntimeState) -> bool {
    if PARKED_TIMER_ID.get().is_none() && state.data.export_pending.has_retryable() {
        let timer_id = ic_cdk_timers::set_timer(Duration::from_millis(PARK_RETRY_INTERVAL_MS), run_parked);
        PARKED_TIMER_ID.set(Some(timer_id));
        true
    } else {
        false
    }
}

fn run() {
    trace!("'delete_users' running");
    TIMER_ID.set(None);

    if let Some(user) = mutate_state(get_next) {
        ic_cdk::futures::spawn(process_user(user));
    }
}

fn run_parked() {
    trace!("'retry_parked_exports' running");
    PARKED_TIMER_ID.set(None);

    let now = read_state(|state| state.env.now());
    let due = read_state(|state| state.data.export_pending.due_for_retry(now));
    for record in due {
        ic_cdk::futures::spawn(process_parked(record));
    }

    // Re-arm while anything remains retryable (self-healing).
    mutate_state(|state| start_parked_retry_job_if_required(state));
}

fn get_next(state: &mut RuntimeState) -> Option<UserToDelete> {
    state.data.users_to_delete_queue.pop_front()
}

async fn process_user(user: UserToDelete) {
    let outcome = process_user_inner(&user).await;

    mutate_state(|state| {
        apply_outcome(state, &user, outcome);

        let more = !state.data.users_to_delete_queue.is_empty();
        start_job_if_required(state, more.then_some(FAST_RETRY_INTERVAL_MS));
        start_parked_retry_job_if_required(state);
    });
}

/// Retry of a DURABLE retryable record (slow drain path) — a `Parked` record
/// (retry export) or an `ExportedUninstallPending` record (retry uninstall only).
async fn process_parked(record: ExportPendingRecord) {
    let user = UserToDelete {
        user_id: record.user_id,
        #[allow(deprecated)]
        triggered_by_user: false,
        attempt: record.attempt as usize,
    };
    let outcome = process_user_inner(&user).await;

    mutate_state(|state| {
        apply_outcome(state, &user, outcome);
        start_parked_retry_job_if_required(state);
    });
}

/// Fold a completed attempt's outcome back into durable state. The ONLY place the
/// pending record is removed is a confirmed `Deleted` — never before uninstall.
fn apply_outcome(state: &mut RuntimeState, user: &UserToDelete, outcome: ProcessOutcome) {
    match outcome {
        ProcessOutcome::Deleted(canisters_to_notify) => {
            // Uninstall confirmed (or already-uninstalled accepted in EUP) — only
            // now remove the durable record, then run deletion-complete bookkeeping.
            state.data.export_pending.remove(&user.user_id.into());
            complete_deletion(state, user.user_id, canisters_to_notify);
        }
        ProcessOutcome::ExportFailed(failure) => record_export_failure(state, user, &failure),
        ProcessOutcome::UninstallPending(failure) => record_uninstall_pending(state, user.user_id, &failure),
    }
}

/// Existing "deletion fully complete" bookkeeping, factored out so the fast path
/// and the drain path complete identically. Reached ONLY via `ProcessOutcome::Deleted`
/// — i.e. after export + uninstall both succeed (or already-uninstalled in EUP).
fn complete_deletion(state: &mut RuntimeState, user_id: UserId, canisters_to_notify: Vec<CanisterId>) {
    state.data.global_users.remove(&user_id);
    state.data.local_users.remove(&user_id);

    let now = state.env.now();
    for canister_id in canisters_to_notify {
        state.push_event_to_user_index(UserIndexEvent::NotifyOfUserDeleted(canister_id, user_id), now);
    }
}

/// EXPORT-stage failure (receipt not yet durably stored). Writes/updates the
/// durable record as `Retrying` (fast window) or `Parked` (after the limit) — the
/// retained-copy-first parked lifecycle. Never reaches uninstall.
fn record_export_failure(state: &mut RuntimeState, user: &UserToDelete, failure: &DeletionFailure) {
    let now = state.env.now();
    let canister_id: CanisterId = user.user_id.into();
    let attempt = (user.attempt as u32).saturating_add(1);
    let park = attempt >= park_threshold(state.data.test_mode);

    let first_failed_at = state
        .data
        .export_pending
        .get(&canister_id)
        .map(|r| r.first_failed_at)
        .unwrap_or(now);

    let status = if park { ExportStatus::Parked } else { ExportStatus::Retrying };
    let next_retry_at = now + if park { PARK_RETRY_INTERVAL_MS } else { FAST_RETRY_INTERVAL_MS };

    state.data.export_pending.upsert(ExportPendingRecord {
        user_id: user.user_id,
        user_canister_id: canister_id,
        receipt_id: failure.receipt_id.clone(),
        receipts_canister_id: failure.receipts_canister_id,
        // Export not yet confirmed — this record re-runs the FULL non-EUP path on
        // retry (re-reading groups/communities while the canister is still
        // installed), so it carries no completion snapshot.
        canisters_to_notify: Vec::new(),
        attempt,
        first_failed_at,
        last_failed_at: now,
        last_error_class: failure.error_class.to_string(),
        next_retry_at,
        status,
    });

    if park {
        error!(
            event = "mktd_receipt_export_blocked_uninstall",
            user_canister_id = %canister_id,
            receipt_id = %failure.receipt_id.as_deref().map(hex::encode).unwrap_or_default(),
            receipts_canister_id = %failure.receipts_canister_id.map(|c| c.to_string()).unwrap_or_default(),
            attempt,
            error_class = failure.error_class,
            action = "uninstall_blocked_retained_copy_pending",
            status = "parked",
            "CVDR export blocked; user canister retained (tombstoned/finalized), uninstall deferred to durable parked retry"
        );
        start_parked_retry_job_if_required(state);
    } else {
        if attempt >= WARN_THRESHOLD {
            warn!(
                event = "mktd_receipt_export_retrying",
                user_canister_id = %canister_id,
                attempt,
                error_class = failure.error_class,
                action = "uninstall_blocked_retained_copy_pending",
                status = "retrying",
                "CVDR export still failing within the bounded retry window"
            );
        }
        state.data.users_to_delete_queue.push_back(UserToDelete {
            user_id: user.user_id,
            #[allow(deprecated)]
            triggered_by_user: user.triggered_by_user,
            attempt: attempt as usize,
        });
    }
}

/// UNINSTALL-stage failure AFTER a confirmed export. Keeps the record as
/// `ExportedUninstallPending` (G: do NOT roll back to "not exported"), reschedules
/// the uninstall retry on the slow drain, and emits a PII-free operator warn.
fn record_uninstall_pending(state: &mut RuntimeState, user_id: UserId, failure: &DeletionFailure) {
    let now = state.env.now();
    let canister_id: CanisterId = user_id.into();
    let existing = state.data.export_pending.get(&canister_id);

    let attempt = existing.as_ref().map(|r| r.attempt).unwrap_or(0).saturating_add(1);
    let first_failed_at = existing.as_ref().map(|r| r.first_failed_at).unwrap_or(now);
    let receipt_id = failure.receipt_id.clone().or_else(|| existing.as_ref().and_then(|r| r.receipt_id.clone()));
    let receipts_canister_id = failure.receipts_canister_id.or_else(|| existing.as_ref().and_then(|r| r.receipts_canister_id));
    // Preserve the completion snapshot captured when the record first entered EUP —
    // never refresh it from the (possibly destroyed) user canister (#13 / 1c).
    let canisters_to_notify = existing.as_ref().map(|r| r.canisters_to_notify.clone()).unwrap_or_default();

    state.data.export_pending.upsert(ExportPendingRecord {
        user_id,
        user_canister_id: canister_id,
        receipt_id: receipt_id.clone(),
        receipts_canister_id,
        canisters_to_notify,
        attempt,
        first_failed_at,
        last_failed_at: now,
        last_error_class: failure.error_class.to_string(),
        next_retry_at: now + PARK_RETRY_INTERVAL_MS,
        // NEVER rolled back to a not-exported status.
        status: ExportStatus::ExportedUninstallPending,
    });

    warn!(
        event = "mktd_receipt_uninstall_pending",
        user_canister_id = %canister_id,
        receipt_id = %receipt_id.as_deref().map(hex::encode).unwrap_or_default(),
        attempt,
        error_class = failure.error_class,
        action = "uninstall_retry_pending",
        status = "exported_uninstall_pending",
        "CVDR exported but uninstall not yet confirmed; record retained as ExportedUninstallPending, uninstall will be retried"
    );

    start_parked_retry_job_if_required(state);
}

/// Persist the `ExportedUninstallPending` transition BEFORE uninstall. This
/// durable write commits at the following `uninstall().await`; it is the ordering
/// guarantee — any replay after a crash/trap between export-success and a confirmed
/// uninstall sees EUP and retries uninstall WITHOUT re-exporting (idempotent).
fn persist_exported_uninstall_pending(
    state: &mut RuntimeState,
    user: &UserToDelete,
    ctx: &ExportContext,
    canisters_to_notify: &[CanisterId],
) {
    let now = state.env.now();
    let canister_id: CanisterId = user.user_id.into();
    let existing = state.data.export_pending.get(&canister_id);

    state.data.export_pending.upsert(ExportPendingRecord {
        user_id: user.user_id,
        user_canister_id: canister_id,
        receipt_id: ctx.receipt_id.clone(),
        receipts_canister_id: ctx.receipts_canister_id,
        // #13 snapshot: captured from THIS attempt's pre-uninstall read, persisted
        // BEFORE the uninstall await so recovery never re-reads the user canister.
        canisters_to_notify: canisters_to_notify.to_vec(),
        attempt: existing.as_ref().map(|r| r.attempt).unwrap_or(0),
        first_failed_at: existing.as_ref().map(|r| r.first_failed_at).unwrap_or(now),
        last_failed_at: existing.as_ref().map(|r| r.last_failed_at).unwrap_or(now),
        last_error_class: existing.as_ref().map(|r| r.last_error_class.clone()).unwrap_or_default(),
        next_retry_at: now + PARK_RETRY_INTERVAL_MS,
        status: ExportStatus::ExportedUninstallPending,
    });
    start_parked_retry_job_if_required(state);
}

async fn process_user_inner(user: &UserToDelete) -> ProcessOutcome {
    let canister_id: CanisterId = user.user_id.into();

    // A durable record in `ExportedUninstallPending` means export was already
    // confirmed on an earlier attempt. Per #13 the remaining work — uninstall +
    // bookkeeping — is completable from THIS durable snapshot alone, so we take the
    // recovery path and NEVER query the (possibly already-destroyed) user canister.
    let eup_record = read_state(|state| {
        state
            .data
            .export_pending
            .get(&canister_id)
            .filter(|r| matches!(r.status, ExportStatus::ExportedUninstallPending))
    });
    if let Some(record) = eup_record {
        return complete_from_eup(canister_id, record).await;
    }

    // --- Non-EUP path: a first attempt or an export-stage retry. The user canister
    // is still installed (uninstall is never reached until export is confirmed), so
    // read groups/communities — needed for completion — BEFORE export + uninstall.
    let canisters_to_notify = match user_canister_c2c_client::c2c_groups_and_communities(canister_id, &Empty {}).await {
        Ok(r) => r
            .groups
            .into_iter()
            .map(|g| g.into())
            .chain(r.communities.into_iter().map(|c| c.into()))
            .collect::<Vec<CanisterId>>(),
        // Pre-export transient failure → ordinary export-stage retry (we have not
        // exported, so this is never EUP).
        Err(_) => return ProcessOutcome::ExportFailed(DeletionFailure::new("groups_and_communities_unavailable", None, None)),
    };

    // EXPORT stage.
    let ctx = match export_finalized_receipt_first(canister_id).await {
        Ok(ctx) => ctx,
        // Export failed → NEVER reaches uninstall (retained-copy-first).
        Err(failure) => return ProcessOutcome::ExportFailed(failure),
    };

    // A finalized receipt was durably stored → persist EXPORTED_UNINSTALL_PENDING
    // (incl. the `canisters_to_notify` completion snapshot) BEFORE the uninstall
    // await. This is the ordering guarantee AND the recovery snapshot: any replay
    // after this point sees EUP and completes from the snapshot without re-export
    // and without re-reading the user canister.
    let exported = ctx.receipt_id.is_some();
    if exported {
        mutate_state(|state| persist_exported_uninstall_pending(state, user, &ctx, &canisters_to_notify));
    }

    // UNINSTALL stage.
    let result = utils::canister::uninstall(canister_id).await;
    match decide_uninstall(exported, &result) {
        UninstallDecision::Done => ProcessOutcome::Deleted(canisters_to_notify),
        UninstallDecision::KeepEup => {
            ProcessOutcome::UninstallPending(DeletionFailure::new("uninstall_failed", ctx.receipt_id, ctx.receipts_canister_id))
        }
        UninstallDecision::ExportRetry => ProcessOutcome::ExportFailed(DeletionFailure::new("uninstall_failed", None, None)),
    }
}

/// #13 EUP recovery: complete deletion from the DURABLE snapshot alone. The user
/// canister may already be uninstalled/destroyed, so we MUST NOT call
/// `c2c_groups_and_communities` (or any other query) against it — the
/// `canisters_to_notify` targets were captured into the record before uninstall.
///
/// Retry order (1b): the (idempotent) `uninstall_code` doubles as the
/// "is it already gone?" probe.
/// - `Ok` (uninstalled now, or was already module-less) → complete from the snapshot;
/// - already-gone per the narrowed #12 classifier → complete from the snapshot;
/// - a real uninstall failure → keep EUP, retry later.
async fn complete_from_eup(canister_id: CanisterId, record: ExportPendingRecord) -> ProcessOutcome {
    let result = utils::canister::uninstall(canister_id).await;
    match decide_uninstall(/* exported */ true, &result) {
        UninstallDecision::Done => ProcessOutcome::Deleted(record.canisters_to_notify),
        // `exported == true`, so `decide_uninstall` never returns `ExportRetry`; a
        // real failure is `KeepEup`. Either way we keep EUP and retry — no rollback.
        UninstallDecision::KeepEup | UninstallDecision::ExportRetry => {
            ProcessOutcome::UninstallPending(DeletionFailure::new("uninstall_failed", record.receipt_id, record.receipts_canister_id))
        }
    }
}

/// Pure lifecycle decision for the uninstall result (unit-tested). `exported`
/// means the receipt is durably stored (we are in / entering EUP).
/// - success → done;
/// - exported + already-uninstalled/not-installed → done (accepted ONLY in EUP);
/// - exported + other error → keep EUP (retry uninstall; no rollback);
/// - not-exported + error → ordinary export-stage retry.
fn decide_uninstall(exported: bool, result: &Result<(), C2CError>) -> UninstallDecision {
    match result {
        Ok(()) => UninstallDecision::Done,
        Err(error) => {
            if exported {
                if is_already_uninstalled(error) {
                    UninstallDecision::Done
                } else {
                    UninstallDecision::KeepEup
                }
            } else {
                UninstallDecision::ExportRetry
            }
        }
    }
}

/// Whether an uninstall error means the canister is genuinely GONE (no longer a
/// valid call destination), so the deletion goal is already met. Used ONLY in the
/// EUP state — it is NOT a general uninstall-error swallow.
///
/// #12 (narrowed): we accept ONLY a `DestinationInvalid` reject from our own
/// `uninstall_code` call to the management/routing layer. We deliberately do NOT
/// pattern-match the reject *message* — substrings like "not found" / "not
/// installed" / "no wasm module" also occur in authorization failures,
/// method-not-found, ordinary canister application errors, and generic rejects
/// where the canister still very much exists. Class, not text, is the signal.
fn is_already_uninstalled(error: &C2CError) -> bool {
    error.method_name() == "uninstall_code" && matches!(error.reject_code(), RejectCode::DestinationInvalid)
}

#[derive(Debug, PartialEq, Eq)]
enum UninstallDecision {
    Done,
    KeepEup,
    ExportRetry,
}

enum ProcessOutcome {
    /// Uninstall confirmed (or already-uninstalled accepted in EUP).
    Deleted(Vec<CanisterId>),
    /// Export not yet confirmed → Retrying/Parked.
    ExportFailed(DeletionFailure),
    /// Export confirmed, uninstall not yet confirmed → ExportedUninstallPending.
    UninstallPending(DeletionFailure),
}

/// Context learned during export — attached to a later uninstall failure so the
/// durable record carries the receipt id.
struct ExportContext {
    receipt_id: Option<Vec<u8>>,
    receipts_canister_id: Option<CanisterId>,
}

/// Sanitized, PII-free failure descriptor for the durable record + logging.
struct DeletionFailure {
    error_class: &'static str,
    receipt_id: Option<Vec<u8>>,
    receipts_canister_id: Option<CanisterId>,
}

impl DeletionFailure {
    fn new(error_class: &'static str, receipt_id: Option<Vec<u8>>, receipts_canister_id: Option<CanisterId>) -> Self {
        DeletionFailure { error_class, receipt_id, receipts_canister_id }
    }
}

/// Export this user's finalized CVDR to the durable receipts canister. `Err`
/// blocks uninstall. `Ok` when there is nothing to export (no finalized receipt)
/// or the export is durably stored (incl. the idempotent already-stored case — so
/// a retry creates no duplicate divergent record).
async fn export_finalized_receipt_first(user_canister_id: CanisterId) -> Result<ExportContext, DeletionFailure> {
    use user_canister::c2c_mktd_export_receipt::Response as ExportResponse;

    let exported = match user_canister_c2c_client::c2c_mktd_export_receipt(user_canister_id, &Empty {}).await {
        Ok(ExportResponse::Success(exported)) => exported,
        // No finalized receipt to export — uninstall may proceed.
        Ok(ExportResponse::NotFinalized) => {
            return Ok(ExportContext { receipt_id: None, receipts_canister_id: None });
        }
        Err(_) => return Err(DeletionFailure::new("export_source_unavailable", None, None)),
    };

    let receipt_id = Some(exported.receipt_id.clone());

    let Some(receipts_canister_id) = read_state(|state| state.data.receipts_canister_id) else {
        return Err(DeletionFailure::new("receipts_canister_unconfigured", receipt_id, None));
    };

    let store_args = receipts_canister::store::Args {
        receipt_id: exported.receipt_id,
        receipt_json: exported.receipt_json,
    };
    match receipts_canister_c2c_client::store(receipts_canister_id, &store_args).await {
        // Newly stored, or already durably present with identical bytes (idempotent).
        Ok(receipts_canister::store::Response::Success) | Ok(receipts_canister::store::Response::AlreadyExists) => {
            Ok(ExportContext { receipt_id, receipts_canister_id: Some(receipts_canister_id) })
        }
        Ok(_other) => Err(DeletionFailure::new("receipts_store_rejected", receipt_id, Some(receipts_canister_id))),
        Err(_) => Err(DeletionFailure::new("receipts_canister_unavailable", receipt_id, Some(receipts_canister_id))),
    }
}

#[cfg(test)]
mod tests {
    use super::{decide_uninstall, is_already_uninstalled, UninstallDecision};
    use candid::Principal;
    use ic_cdk::call::RejectCode;
    use types::C2CError;

    fn err(reject_code: RejectCode, message: &str) -> C2CError {
        err_for("uninstall_code", reject_code, message)
    }

    fn err_for(method: &str, reject_code: RejectCode, message: &str) -> C2CError {
        C2CError::new(Principal::anonymous(), method, reject_code, message.to_string())
    }

    #[test]
    fn already_uninstalled_detects_gone_canister() {
        // The ONLY accepted signal: a `DestinationInvalid` reject from our
        // `uninstall_code` management call — the canister id is no longer a valid
        // destination. The message is irrelevant to the decision.
        assert!(is_already_uninstalled(&err(RejectCode::DestinationInvalid, "canister not found")));
        assert!(is_already_uninstalled(&err(RejectCode::DestinationInvalid, "")));
    }

    // #12 negative classifier coverage: every one of these must be classified as
    // NOT-gone, so the EUP record is KEPT and deletion is NOT completed. Each is a
    // failure mode where the user canister may still exist.
    #[test]
    fn already_uninstalled_does_not_swallow_not_a_controller() {
        assert!(!is_already_uninstalled(&err(
            RejectCode::CanisterError,
            "Only the controllers of the canister can call ic00 method uninstall_code"
        )));
    }

    #[test]
    fn already_uninstalled_does_not_swallow_method_not_found() {
        assert!(!is_already_uninstalled(&err(
            RejectCode::CanisterError,
            "Canister has no query method 'uninstall_code'"
        )));
    }

    #[test]
    fn already_uninstalled_does_not_swallow_canister_application_error() {
        assert!(!is_already_uninstalled(&err(RejectCode::CanisterError, "Canister trapped: unreachable")));
    }

    #[test]
    fn already_uninstalled_does_not_swallow_authorization_failure() {
        assert!(!is_already_uninstalled(&err(RejectCode::CanisterReject, "Unauthorized")));
    }

    #[test]
    fn already_uninstalled_does_not_swallow_generic_not_found_message() {
        // A reject whose MESSAGE contains "not found" but whose class is NOT
        // DestinationInvalid must never be swallowed (the old substring bug).
        assert!(!is_already_uninstalled(&err(RejectCode::CanisterError, "Canister X is not found")));
        assert!(!is_already_uninstalled(&err(RejectCode::CanisterReject, "resource not_found")));
        assert!(!is_already_uninstalled(&err(RejectCode::CanisterError, "no wasm module")));
        assert!(!is_already_uninstalled(&err(RejectCode::CanisterError, "canister is not installed")));
    }

    #[test]
    fn already_uninstalled_does_not_swallow_routing_or_transient_failure() {
        // The canister may still exist behind a transient/routing failure.
        assert!(!is_already_uninstalled(&err(RejectCode::SysTransient, "subnet is overloaded")));
        assert!(!is_already_uninstalled(&err(RejectCode::SysFatal, "call rejected by routing layer")));
        // Even a `DestinationInvalid` that did NOT come from our `uninstall_code`
        // management call (a different call/layer) must not be treated as gone.
        assert!(!is_already_uninstalled(&err_for(
            "c2c_groups_and_communities",
            RejectCode::DestinationInvalid,
            "destination invalid"
        )));
    }

    #[test]
    fn negative_classifier_cases_keep_eup_not_complete() {
        // End-to-end through the lifecycle decision: each non-gone failure, while
        // exported (in EUP), must yield KeepEup — never Done.
        for error in [
            err(RejectCode::CanisterError, "Only the controllers of the canister can call ic00 method uninstall_code"),
            err(RejectCode::CanisterError, "Canister has no query method 'uninstall_code'"),
            err(RejectCode::CanisterError, "Canister trapped: unreachable"),
            err(RejectCode::CanisterReject, "Unauthorized"),
            err(RejectCode::CanisterError, "Canister X is not found"),
            err(RejectCode::SysTransient, "subnet is overloaded"),
            err(RejectCode::SysFatal, "call rejected by routing layer"),
        ] {
            assert_eq!(decide_uninstall(true, &Err(error)), UninstallDecision::KeepEup);
        }
    }

    #[test]
    fn uninstall_success_is_done() {
        assert_eq!(decide_uninstall(true, &Ok(())), UninstallDecision::Done);
        assert_eq!(decide_uninstall(false, &Ok(())), UninstallDecision::Done);
    }

    #[test]
    fn exported_uninstall_failure_keeps_eup_no_rollback() {
        // Exported + a REAL uninstall failure → keep EUP (never roll back).
        let real = Err(err(RejectCode::CanisterError, "not authorized"));
        assert_eq!(decide_uninstall(true, &real), UninstallDecision::KeepEup);
    }

    #[test]
    fn exported_already_uninstalled_is_success_only_in_eup() {
        let gone = Err(err(RejectCode::DestinationInvalid, "canister not found"));
        // In EUP (exported) → accepted as success.
        assert_eq!(decide_uninstall(true, &gone), UninstallDecision::Done);
        // NOT exported → never swallow as success; ordinary export-stage retry.
        assert_eq!(decide_uninstall(false, &gone), UninstallDecision::ExportRetry);
    }

    #[test]
    fn not_exported_uninstall_failure_is_export_retry() {
        let any = Err(err(RejectCode::SysTransient, "busy"));
        assert_eq!(decide_uninstall(false, &any), UninstallDecision::ExportRetry);
    }
}
