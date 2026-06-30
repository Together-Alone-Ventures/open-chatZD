//! v5 CVDR-on-Index delete leg.
//!
//! The local_user_index drives a user deletion through a forward-only, durable,
//! upgrade-surviving state machine and ends by publishing a single IC-certified
//! commitment over the deletion. Completion (mapping removal + membership-removal
//! notifications) is performed by the external-finalizer-driven `finalize_cvdr`
//! update once the IC `data_certificate()` is captured — NOT by this job.
//!
//! Leg order (CD §1), per in-flight [`CvdrDraft`] `stage`:
//! 1. `Captured`            capture H_user_pre (`canister_status` module hash) + targets +
//!                          record_id, allocate `deletion_seq` + nonce, persist the draft
//!                          BEFORE any destructive step.
//! 2. (Captured -> )        `uninstall_code`, then confirm no-module via `canister_status`.
//! 3. `Uninstalled`         single-slot guard: if the certified-data slot is free (or
//!                          already ours) publish the commitment via `certified_data_set`,
//!                          claim the slot, advance to `AwaitingCertificate`.
//! 4. `AwaitingCertificate` wait for the external finalizer. `finalize_cvdr` verifies the
//!                          certificate binds the commitment, stores the releasable CVDR,
//!                          then runs `complete_deletion` and frees the slot.
//!
//! Forward-recovery only: every stage is idempotent and resumable from the persisted
//! draft; nothing rolls back. The durable draft is stable-backed (survives upgrades),
//! so there is no upgrade-trapping heap lock.

use crate::model::cvdr::{self, CvdrDraft, DraftStage};
use crate::{RuntimeState, UserIndexEvent, UserToDelete, mutate_state, read_state};
use constants::SECOND_IN_MS;
use ic_cdk::management_canister::CanisterStatusArgs;
use ic_cdk_timers::TimerId;
use rand::RngCore;
use std::cell::Cell;
use std::time::Duration;
use tracing::{trace, warn};
use types::{CanisterId, Empty, Milliseconds, UserId};

thread_local! {
    static TIMER_ID: Cell<Option<TimerId>> = Cell::default();
}

/// Bounded fast-retry backoff between leg attempts (transient failures / slot busy).
const FAST_RETRY_INTERVAL_MS: Milliseconds = 30 * SECOND_IN_MS;

/// Emit a `warn!` once a retryable attempt has persisted past this count (e.g. a delete
/// blocked indefinitely behind a stuck certified-data slot, or an absent finalizer).
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

fn run() {
    trace!("'delete_users' running");
    TIMER_ID.set(None);

    if let Some(user) = mutate_state(get_next) {
        ic_cdk::futures::spawn(process_user(user));
    }
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
    });
}

/// Fold a completed attempt's outcome back into durable state. The job NEVER completes
/// a deletion (that is `finalize_cvdr`'s job once the certificate is captured); it only
/// drives the draft up to `AwaitingCertificate` or re-queues a retry.
fn apply_outcome(state: &mut RuntimeState, user: &UserToDelete, outcome: ProcessOutcome) {
    match outcome {
        // Commitment published; the durable draft is AwaitingCertificate. Drop from the
        // active queue — completion is performed externally by `finalize_cvdr`.
        ProcessOutcome::AwaitingFinalizer => {}
        // Transient failure or the single certified-data slot is occupied by another
        // in-flight deletion: re-queue on the fast interval (forward-only; no rollback).
        ProcessOutcome::Retry { error_class } => {
            let attempt = (user.attempt as u32).saturating_add(1);
            if attempt >= WARN_THRESHOLD {
                warn!(
                    event = "cvdr_delete_retrying",
                    user_canister_id = %CanisterId::from(user.user_id),
                    attempt,
                    error_class,
                    "v5 CVDR delete leg still retrying (transient failure or certified-data slot busy / awaiting finalizer)"
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
}

/// Deletion bookkeeping, reached ONLY once the releasable CVDR is durably stored (gated
/// by `finalize_cvdr`). Removes the local/global mappings and fires the membership-removal
/// notification chain. `pub(crate)` so the finalize update can complete from the draft.
pub(crate) fn complete_deletion(state: &mut RuntimeState, user_id: UserId, canisters_to_notify: Vec<CanisterId>) {
    state.data.global_users.remove(&user_id);
    state.data.local_users.remove(&user_id);

    let now = state.env.now();
    for canister_id in canisters_to_notify {
        state.push_event_to_user_index(UserIndexEvent::NotifyOfUserDeleted(canister_id, user_id), now);
    }
}

async fn process_user_inner(user: &UserToDelete) -> ProcessOutcome {
    let canister_id: CanisterId = user.user_id.into();

    // Forward-only recovery: resume from the durable draft if present, else capture fresh.
    let draft = match read_state(|state| state.data.cvdr.get_draft(&canister_id)) {
        Some(draft) => draft,
        None => match capture_draft(user, canister_id).await {
            Ok(draft) => draft,
            Err(error_class) => return ProcessOutcome::Retry { error_class },
        },
    };

    advance_draft(draft).await
}

/// Capture the immutable pre-uninstall witnesses and persist the `Captured` draft BEFORE
/// any destructive step. All inputs (targets, pre-uninstall module hash) are read while
/// the user canister is still installed, so recovery never needs to re-read it.
async fn capture_draft(user: &UserToDelete, canister_id: CanisterId) -> Result<CvdrDraft, &'static str> {
    // Membership-removal targets — captured before uninstall.
    let canisters_to_notify = match user_canister_c2c_client::c2c_groups_and_communities(canister_id, &Empty {}).await {
        Ok(r) => r
            .groups
            .into_iter()
            .map(|g| g.into())
            .chain(r.communities.into_iter().map(|c| c.into()))
            .collect::<Vec<CanisterId>>(),
        Err(_) => return Err("groups_and_communities_unavailable"),
    };

    // H_user_pre input: the pre-uninstall module hash (the code being destroyed).
    let module_hash_pre = match ic_cdk::management_canister::canister_status(&CanisterStatusArgs { canister_id }).await {
        Ok(status) => status.module_hash.unwrap_or_default(),
        Err(_) => return Err("canister_status_unavailable"),
    };

    let record_id = cvdr::record_id_for(user.user_id);
    // Capture the executor (this index) provenance from durable deploy-supplied state at the
    // SAME pre-uninstall point as the target's module_hash_pre. This captured value is
    // authoritative for H_index; a mid-flight index upgrade cannot change it.
    let (deletion_seq, nonce, now, index_canister_id, executor_module_hash) = mutate_state(|state| {
        let seq = state.data.cvdr_next_deletion_seq;
        state.data.cvdr_next_deletion_seq = seq.saturating_add(1);
        let mut nonce = [0u8; 32];
        state.env.rng().fill_bytes(&mut nonce);
        (seq, nonce, state.env.now(), state.env.canister_id(), state.data.executor_module_hash.to_vec())
    });

    let h_user_pre = cvdr::h_user_pre(canister_id, &module_hash_pre);
    let h_index = cvdr::h_index(index_canister_id, &executor_module_hash);
    let commitment = cvdr::commitment(&record_id, deletion_seq, &h_user_pre, &h_index, canister_id);
    let receipt_id = cvdr::receipt_id_for(&record_id, deletion_seq, &nonce);

    let draft = CvdrDraft {
        user_id: user.user_id,
        user_canister_id: canister_id,
        index_canister_id,
        record_id,
        deletion_seq,
        nonce,
        receipt_id,
        module_hash_pre,
        executor_module_hash,
        h_user_pre,
        h_index,
        commitment,
        canisters_to_notify,
        created_at: now,
        attempt: user.attempt as u32,
        stage: DraftStage::Captured,
    };
    mutate_state(|state| state.data.cvdr.upsert_draft(draft.clone()));
    Ok(draft)
}

/// Advance the draft forward by exactly one stage per attempt (each step idempotent).
async fn advance_draft(draft: CvdrDraft) -> ProcessOutcome {
    let canister_id = draft.user_canister_id;
    match draft.stage {
        DraftStage::Captured => {
            // `uninstall_code` (idempotent) then confirm no-module before advancing.
            if utils::canister::uninstall(canister_id).await.is_err() {
                return ProcessOutcome::Retry { error_class: "uninstall_failed" };
            }
            match ic_cdk::management_canister::canister_status(&CanisterStatusArgs { canister_id }).await {
                Ok(status) if status.module_hash.is_none() => {
                    match mutate_state(|state| advance_stage(state, canister_id, DraftStage::Uninstalled)) {
                        Some(advanced) => advance_publish(advanced),
                        None => ProcessOutcome::Retry { error_class: "draft_lost" },
                    }
                }
                Ok(_) => ProcessOutcome::Retry { error_class: "module_still_present" },
                Err(_) => ProcessOutcome::Retry { error_class: "status_unavailable" },
            }
        }
        DraftStage::Uninstalled => advance_publish(draft),
        // Already published. Re-run the publish (idempotent — the slot is already ours) so
        // certified_data is re-set if this attempt follows an upgrade that cleared it; then
        // wait for `finalize_cvdr`.
        DraftStage::AwaitingCertificate => advance_publish(draft),
    }
}

/// Single-slot guard + certified-commitment publish. Publishes ONLY if the certified-data
/// slot is free or already held by THIS draft; otherwise the later deletion waits (no
/// overwrite of a pending commitment before its certificate is captured — CD §4).
fn advance_publish(draft: CvdrDraft) -> ProcessOutcome {
    let canister_id = draft.user_canister_id;
    let publish = mutate_state(|state| match state.data.cvdr_awaiting_receipt_id {
        Some(receipt_id) if receipt_id != draft.receipt_id => false,
        _ => {
            state.data.cvdr_awaiting_receipt_id = Some(draft.receipt_id);
            advance_stage(state, canister_id, DraftStage::AwaitingCertificate);
            true
        }
    });

    if publish {
        // Publish the commitment to certified_data; the cert is readable next round via
        // the `cvdr_data_certificate` query and returned to `finalize_cvdr`.
        ic_cdk::api::certified_data_set(&draft.commitment);
        ProcessOutcome::AwaitingFinalizer
    } else {
        ProcessOutcome::Retry { error_class: "certified_slot_busy" }
    }
}

/// Persist a forward stage transition on the durable draft, returning the updated draft.
fn advance_stage(state: &mut RuntimeState, canister_id: CanisterId, stage: DraftStage) -> Option<CvdrDraft> {
    let mut draft = state.data.cvdr.get_draft(&canister_id)?;
    draft.stage = stage;
    state.data.cvdr.upsert_draft(draft.clone());
    Some(draft)
}

enum ProcessOutcome {
    /// Commitment published; the draft is AwaitingCertificate. Completion is external.
    AwaitingFinalizer,
    /// Transient failure or certified-data slot busy → re-queue (forward-only).
    Retry { error_class: &'static str },
}

/// Resume in-flight CVDR deletions after a local_user_index upgrade. The durable drafts
/// (stable memory) survive the upgrade, but two things do not: the IC `certified_data`
/// (cleared on upgrade) and any user already popped off the volatile delete queue. This
/// re-publishes the one pending commitment and re-enqueues lost drafts so the forward-only
/// job drives them to completion. Called from `init_state`; a no-op on a fresh install.
pub(crate) fn resume_in_flight_drafts(state: &mut RuntimeState) {
    let drafts = state.data.cvdr.all_drafts();
    if drafts.is_empty() {
        return;
    }

    let queued: std::collections::HashSet<UserId> =
        state.data.users_to_delete_queue.iter().map(|u| u.user_id).collect();

    for draft in drafts {
        // Re-publish the pending certified commitment for the single draft holding the slot.
        if draft.stage == DraftStage::AwaitingCertificate && state.data.cvdr_awaiting_receipt_id == Some(draft.receipt_id) {
            ic_cdk::api::certified_data_set(&draft.commitment);
        }
        // Re-enqueue any draft the queue lost when its user was popped before the upgrade.
        if !queued.contains(&draft.user_id) {
            state.data.users_to_delete_queue.push_back(UserToDelete {
                user_id: draft.user_id,
                #[allow(deprecated)]
                triggered_by_user: false,
                attempt: draft.attempt as usize,
            });
        }
    }
}
