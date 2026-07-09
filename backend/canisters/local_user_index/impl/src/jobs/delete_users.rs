//! CVDR-on-Index delete leg (CVDR finalization rework — spec §2/§3/§5/§8).
//!
//! The local_user_index drives a user deletion through a forward-only, durable,
//! upgrade-surviving state machine. **OpenChat-native user deletion completes inside THIS job,
//! independently of certificate capture** (spec §8, the cleanup split): after `uninstall_code`
//! and the receipt publish, the job itself performs the cleanup (global/local user-mapping
//! removal + membership-removal notifications) — it is NOT gated on any finalizer. The receipt's
//! claim is "targets captured; notification attempted or queued" (never "confirmed erased").
//!
//! CVDR finalization (capturing the IC certificate into the immutable frozen package) is a
//! SEPARATE, CVDR-only step that never holds the user deletion half-open. Its store is gated on
//! FULL on-chain verification BEFORE store (spec §6 hard security rule — full BLS -> NNS + witness
//! binding; a response failing verification is discarded and retried, never stored, so a single
//! untrusted node cannot poison the first-wins slot). In Slice 1 the self-finalization loop (spec
//! §6) and the `finalize_cvdr` backstop (spec §7) are not yet built — `finalize_cvdr` is an inert
//! compile-safe stub (spec §8b). A published receipt therefore rests at `AwaitingCertificate`
//! meaning "user fully deleted; only the CVDR certificate is still pending".
//!
//! Leg order, per in-flight [`CvdrDraft`] `stage`:
//! 1. `Captured`            capture H_user_pre (`canister_status` module hash) + targets +
//!                          record_id + `salt` (raw_rand), allocate `deletion_seq` + nonce,
//!                          persist the draft BEFORE any destructive step.
//! 2. (Captured -> )        `uninstall_code`, confirm no-module, record `uninstall_completed_at`.
//! 3. `Uninstalled`         ATOMIC publish (spec §3): insert the receipt leaf into the certified
//!                          receipt tree + `certified_data_set(root)` + record
//!                          `receipt_committed_at`, all in one message with no `await` between;
//!                          then run the cleanup (spec §8); advance to `AwaitingCertificate`.
//!                          No single-slot guard — the tree holds many receipts under one root.
//! 4. `AwaitingCertificate` user deletion is DONE. Awaiting only the CVDR certificate, captured
//!                          by the CVDR-only finalization step (Slice 2).
//!
//! Forward-recovery only: every stage is idempotent and resumable from the persisted
//! draft; nothing rolls back. The durable draft is stable-backed (survives upgrades); the
//! certified receipt tree is heap-resident and rebuilt in post_upgrade (spec §4).

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
        // Receipt published AND user deletion completed (cleanup ran in the job, spec §8). Drop
        // from the active queue — only the CVDR certificate is still pending. Kick the
        // self-finalization loop (spec §6) to capture + store it.
        ProcessOutcome::AwaitingCertificate => {
            crate::jobs::self_finalize_cvdr::start_if_required(state);
        }
        // Transient failure: re-queue on the fast interval (forward-only; no rollback).
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

/// OpenChat-native deletion cleanup: removes the local/global mappings and fires the
/// membership-removal notification chain. Per spec §8 this runs from the delete JOB, after
/// uninstall + receipt publish, **independently of certificate capture** — never from the
/// finalization path. Called exactly once per deletion (on the first `Uninstalled ->
/// AwaitingCertificate` transition), so the `NotifyOfUserDeleted` chain is not re-fired on
/// idempotent re-publish/resume.
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

    // Fresh salt for TARGETS_COMMITMENT_V1 (spec §2), from management-canister raw_rand during
    // pre-commitments (await is fine here — the §3 atomicity rule covers only the publish message).
    // Never reused across deletions; committed into the receipt body and handed back in the
    // user-held reveal package (later slice).
    let salt = utils::canister::get_random_seed().await;

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
        salt,
        canisters_to_notify,
        uninstall_completed_at: 0,
        receipt_committed_at: 0,
        finalize_attempt: 0,
        finalize_last_attempt_at: 0,
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
                    // Record uninstall_completed_at (IC consensus ns, spec §5) and advance.
                    let now_ns = ic_cdk::api::time();
                    let advanced = mutate_state(|state| {
                        let mut d = state.data.cvdr.get_draft(&canister_id)?;
                        if d.uninstall_completed_at == 0 {
                            d.uninstall_completed_at = now_ns;
                        }
                        d.stage = DraftStage::Uninstalled;
                        state.data.cvdr.upsert_draft(d);
                        Some(())
                    });
                    match advanced {
                        Some(()) => advance_publish(canister_id),
                        None => ProcessOutcome::Retry { error_class: "draft_lost" },
                    }
                }
                Ok(_) => ProcessOutcome::Retry { error_class: "module_still_present" },
                Err(_) => ProcessOutcome::Retry { error_class: "status_unavailable" },
            }
        }
        DraftStage::Uninstalled => advance_publish(canister_id),
        // Already published. Re-assert the certified root (idempotent — recomputes the same leaf)
        // in case an upgrade cleared certified_data; cleanup already ran and is NOT repeated.
        DraftStage::AwaitingCertificate => advance_publish(canister_id),
        // Terminal finalization states (spec §6 self-loop / §7 backstop): user deletion is long
        // done; the delete job has nothing left to do. Drop from the active queue. `LateFinalized`
        // is a backstop-stored late capture (spec §7 rule 8) — also terminal here.
        DraftStage::CertificateCaptured | DraftStage::LateFinalized | DraftStage::FailedStuck => {
            ProcessOutcome::AwaitingCertificate
        }
    }
}

/// ATOMIC receipt publish (spec §3) + cleanup split (spec §8), in a SINGLE message with no
/// `await` between read-root and set-root: record `receipt_committed_at`, insert the receipt
/// leaf into the certified receipt tree, and `certified_data_set(root)`. On the FIRST publish
/// only, run the OpenChat-native cleanup (mapping removal + membership notifications) so user
/// deletion completes independently of certificate capture. No single-slot guard — the tree
/// holds many receipts under one root, so concurrent deletions never serialize.
fn advance_publish(canister_id: CanisterId) -> ProcessOutcome {
    let now_ns = ic_cdk::api::time();
    mutate_state(|state| {
        let Some(mut draft) = state.data.cvdr.get_draft(&canister_id) else {
            return ProcessOutcome::Retry { error_class: "draft_lost" };
        };
        let first_publish = draft.stage != DraftStage::AwaitingCertificate;
        if draft.receipt_committed_at == 0 {
            draft.receipt_committed_at = now_ns;
        }
        // --- spec §3 atomicity: tree mutation + certified_data_set(root) + receipt_committed_at,
        //     one message, no await between ---
        let receipt_hash = draft.receipt_hash();
        state.data.cvdr_receipt_tree.insert(&draft.receipt_id, &receipt_hash);
        ic_cdk::api::certified_data_set(state.data.cvdr_receipt_tree.root());
        draft.stage = DraftStage::AwaitingCertificate;
        state.data.cvdr.upsert_draft(draft.clone());
        // --- spec §8 cleanup split: user deletion completes HERE, cert-independent (once) ---
        if first_publish {
            complete_deletion(state, draft.user_id, draft.canisters_to_notify.clone());
        }
        ProcessOutcome::AwaitingCertificate
    })
}

enum ProcessOutcome {
    /// Receipt published AND user deletion completed (cleanup ran, spec §8). The draft rests at
    /// `AwaitingCertificate` pending only the CVDR certificate (captured by the CVDR-only
    /// finalization step, Slice 2). Renamed from `AwaitingFinalizer` — there is no finalizer.
    AwaitingCertificate,
    /// Transient failure → re-queue (forward-only).
    Retry { error_class: &'static str },
}

/// Post-upgrade CVDR recovery. Two pieces of state do not survive a local_user_index upgrade:
/// the heap-resident certified receipt tree, and the IC `certified_data` (cleared on upgrade).
///
/// Per spec §4, rebuild the receipt tree from durable state — every frozen package PLUS every
/// in-flight `AwaitingCertificate` draft (published-but-unfinalized receipts must stay in the
/// tree so their certificate can still be captured) — then re-assert `certified_data_set(root)`.
/// Also re-enqueue any draft whose user was popped off the volatile delete queue before the
/// upgrade, so the forward-only job drives it to completion. Called from `init_state`; on a
/// fresh install the stores are empty and this is a no-op.
pub(crate) fn resume_in_flight_drafts(state: &mut RuntimeState) {
    // Finalized receipts (frozen packages) — reuse the stored receipt_hash verbatim.
    for (receipt_id, receipt_hash) in state.data.cvdr.frozen_receipt_leaves() {
        state.data.cvdr_receipt_tree.insert(&receipt_id, &receipt_hash);
    }

    let drafts = state.data.cvdr.all_drafts();
    let queued: std::collections::HashSet<UserId> =
        state.data.users_to_delete_queue.iter().map(|u| u.user_id).collect();

    for draft in &drafts {
        // Published-but-unfinalized receipts: recompute the leaf and re-insert.
        if draft.stage == DraftStage::AwaitingCertificate {
            state.data.cvdr_receipt_tree.insert(&draft.receipt_id, &draft.receipt_hash());
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

    // Re-assert the certified root over the rebuilt tree (spec §4 upgrade rule).
    if !state.data.cvdr_receipt_tree.is_empty() {
        ic_cdk::api::certified_data_set(state.data.cvdr_receipt_tree.root());
    }
}
