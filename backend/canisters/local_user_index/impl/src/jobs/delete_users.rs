//! CVDR-on-Index delete leg (CVDR finalization rework — spec §2/§3/§5/§8/§11.4).
//!
//! The local_user_index drives a user deletion through a forward-only, durable,
//! upgrade-surviving state machine. **OpenChat-native user deletion completes inside THIS job,
//! independently of certificate capture** (spec §8, the cleanup split): after `uninstall_code`
//! and the receipt publish, the job itself performs the cleanup (global/local user-mapping
//! removal + membership-removal notifications) — it is NOT gated on any finalizer. The receipt's
//! claim is "targets captured; notification attempted or queued" (never "confirmed erased").
//!
//! Spec §11.4 sequencing: `prepare_account_deletion` must create a [`DraftStage::Prepared`] draft
//! (and hand RevealWire to the user) **before** Identity enqueue reaches this job. Cold capture
//! inside the job is removed — missing prepare → `prepare_required` retry.
//!
//! CVDR finalization (capturing the IC certificate into the immutable frozen package) is a
//! SEPARATE, CVDR-only step (self-finalization §6 / backstop §7) that never holds the user
//! deletion half-open. Its store is gated on FULL on-chain verification BEFORE store.
//!
//! Leg order, per in-flight [`CvdrDraft`] `stage`:
//! 1. `Prepared`            (from `prepare_account_deletion`) salt+targets+receipt_id; RevealWire
//!                          already handed to the user. Not finalizable; not `/cvdr_live`.
//! 2. (`Prepared` -> )      `uninstall_code`, confirm no-module, record `uninstall_completed_at`.
//! 3. `Uninstalled`         ATOMIC publish (spec §3) + cleanup (spec §8) → `AwaitingCertificate`.
//! 4. `AwaitingCertificate` user deletion DONE; CVDR certificate pending.
//!
//! Forward-recovery only: every stage is idempotent and resumable from the persisted
//! draft; nothing rolls back. The durable draft is stable-backed (survives upgrades); the
//! certified receipt tree is heap-resident and rebuilt in post_upgrade (spec §4).

use crate::model::cvdr::{CvdrDraft, DraftStage};
use crate::{RuntimeState, UserIndexEvent, UserToDelete, mutate_state, read_state};
use constants::SECOND_IN_MS;
use ic_cdk::management_canister::CanisterStatusArgs;
use ic_cdk_timers::TimerId;
use std::cell::Cell;
use std::time::Duration;
use tracing::{trace, warn};
use types::{CanisterId, Milliseconds, UserId};

thread_local! {
    static TIMER_ID: Cell<Option<TimerId>> = Cell::default();
}

/// Backoff between attempts after a transient failure. Success paths must not use this delay.
const FAST_RETRY_INTERVAL_MS: Milliseconds = 30 * SECOND_IN_MS;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScheduleKind {
    Success,
    Retry,
}

fn successor_delay_ms(kind: ScheduleKind, next_front_is_same_user: bool) -> Option<Milliseconds> {
    match kind {
        ScheduleKind::Success => None,
        ScheduleKind::Retry if next_front_is_same_user => Some(FAST_RETRY_INTERVAL_MS),
        ScheduleKind::Retry => None,
    }
}

/// Delay to pass to `start_job_if_required` after `apply_outcome`, or `None` if the queue is idle.
///
/// Callers must invoke this **after** `apply_outcome` so a Retry push_back is visible at `front`.
fn successor_timer_delay(
    kind: ScheduleKind,
    next_front_user: Option<UserId>,
    attempted_user: UserId,
) -> Option<Option<Milliseconds>> {
    next_front_user.map(|front| successor_delay_ms(kind, front == attempted_user))
}

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
    let kind = match &outcome {
        ProcessOutcome::AwaitingCertificate => ScheduleKind::Success,
        ProcessOutcome::Retry { .. } => ScheduleKind::Retry,
    };

    mutate_state(|state| {
        apply_outcome(state, &user, outcome);
        let next_front = state.data.users_to_delete_queue.front().map(|u| u.user_id);
        if let Some(delay) = successor_timer_delay(kind, next_front, user.user_id) {
            start_job_if_required(state, delay);
        }
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

    // Spec §11.4: prepare must have created the draft. No cold capture in the delete job.
    let draft = match read_state(|state| state.data.cvdr.get_draft(&canister_id)) {
        Some(draft) => draft,
        None => return ProcessOutcome::Retry { error_class: "prepare_required" },
    };

    advance_draft(draft).await
}

/// Advance the draft forward by exactly one stage per attempt (each step idempotent).
async fn advance_draft(draft: CvdrDraft) -> ProcessOutcome {
    let canister_id = draft.user_canister_id;
    match draft.stage {
        // Prepared (spec §11.4) and legacy Captured: irreversible uninstall leg.
        DraftStage::Prepared | DraftStage::Captured => {
            // Flip Prepared → Captured *before* the first await so a mid-uninstall upgrade
            // re-enqueues via `resume_in_flight_drafts` (which intentionally skips pure Prepared
            // drafts that never entered the delete commit).
            if draft.stage == DraftStage::Prepared {
                let flipped = mutate_state(|state| {
                    let mut d = state.data.cvdr.get_draft(&canister_id)?;
                    if d.stage == DraftStage::Prepared {
                        d.stage = DraftStage::Captured;
                        state.data.cvdr.upsert_draft(d);
                    }
                    Some(())
                });
                if flipped.is_none() {
                    return ProcessOutcome::Retry { error_class: "draft_lost" };
                }
            }
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

/// Stages whose receipt leaf must be re-inserted into the heap tree after upgrade (spec §4 / M7).
/// `FailedStuck` remains finalizable via §7 backstop and still needs `/cvdr_live` witnesses.
pub(crate) fn should_reinsert_receipt_leaf(stage: DraftStage) -> bool {
    matches!(stage, DraftStage::AwaitingCertificate | DraftStage::FailedStuck)
}

/// Stages that should re-enter the delete queue after upgrade (irreversible mid-flight only).
pub(crate) fn should_resume_delete_queue(stage: DraftStage) -> bool {
    matches!(
        stage,
        DraftStage::Captured | DraftStage::Uninstalled | DraftStage::AwaitingCertificate
    )
}

/// Rebuild heap receipt-tree leaves from durable CVDR state (frozen packages + finalizable drafts).
/// Does not touch `certified_data` — caller asserts the root after rebuild.
pub(crate) fn rebuild_receipt_tree_from_durable(
    cvdr: &crate::model::cvdr::CvdrStore,
    tree: &mut crate::model::cvdr::ReceiptTree,
) {
    for (receipt_id, receipt_hash) in cvdr.frozen_receipt_leaves() {
        tree.insert(&receipt_id, &receipt_hash);
    }
    for draft in cvdr.all_drafts() {
        if should_reinsert_receipt_leaf(draft.stage) {
            tree.insert(&draft.receipt_id, &draft.receipt_hash());
        }
    }
}

/// Post-upgrade CVDR recovery. Two pieces of state do not survive a local_user_index upgrade:
/// the heap-resident certified receipt tree, and the IC `certified_data` (cleared on upgrade).
///
/// Per spec §4, rebuild the receipt tree from durable state — every frozen package PLUS every
/// in-flight `AwaitingCertificate` draft **and** `FailedStuck` draft (published-but-unfinalized
/// receipts must stay in the tree so `/cvdr_live` + §7 backstop still work after upgrade) —
/// then re-assert `certified_data_set(root)`.
/// Also re-enqueue any draft whose user was popped off the volatile delete queue before the
/// upgrade, so the forward-only job drives it to completion. Called from `init_state`; on a
/// fresh install the stores are empty and this is a no-op.
pub(crate) fn resume_in_flight_drafts(state: &mut RuntimeState) {
    rebuild_receipt_tree_from_durable(&state.data.cvdr, &mut state.data.cvdr_receipt_tree);

    let drafts = state.data.cvdr.all_drafts();
    let queued: std::collections::HashSet<UserId> =
        state.data.users_to_delete_queue.iter().map(|u| u.user_id).collect();

    for draft in &drafts {
        // Re-enqueue mid-flight irreversible deletions the queue lost on upgrade.
        // Do NOT enqueue `Prepared` — that would uninstall without user RevealWire ack /
        // identity delete commit (spec §11.4).
        // FailedStuck is not re-queued for uninstall (already past uninstall); self-finalize
        // / backstop own remediation.
        if should_resume_delete_queue(draft.stage) && !queued.contains(&draft.user_id) {
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

#[cfg(test)]
mod scheduler_tests {
    use super::*;
    use candid::Principal;
    use std::collections::VecDeque;
    use types::UserId;

    fn uid(byte: u8) -> UserId {
        UserId::from(Principal::from_slice(&[byte; 29]))
    }

    /// Mirrors `apply_outcome` queue effects then `successor_timer_delay` (ordering under test).
    fn delay_after(kind: ScheduleKind, queue_after_pop: &[u8], attempted: u8) -> Option<Option<u64>> {
        let mut q: VecDeque<u8> = queue_after_pop.iter().copied().collect();
        if matches!(kind, ScheduleKind::Retry) {
            q.push_back(attempted);
        }
        let next_front = q.front().copied().map(uid);
        successor_timer_delay(kind, next_front, uid(attempted))
    }

    #[test]
    fn resume_stage_policy_matches_helpers() {
        assert!(should_reinsert_receipt_leaf(DraftStage::AwaitingCertificate));
        assert!(should_reinsert_receipt_leaf(DraftStage::FailedStuck));
        assert!(!should_reinsert_receipt_leaf(DraftStage::Prepared));
        assert!(!should_reinsert_receipt_leaf(DraftStage::Captured));
        assert!(!should_reinsert_receipt_leaf(DraftStage::CertificateCaptured));
        assert!(!should_reinsert_receipt_leaf(DraftStage::LateFinalized));

        assert!(should_resume_delete_queue(DraftStage::Captured));
        assert!(should_resume_delete_queue(DraftStage::Uninstalled));
        assert!(should_resume_delete_queue(DraftStage::AwaitingCertificate));
        assert!(!should_resume_delete_queue(DraftStage::FailedStuck));
        assert!(!should_resume_delete_queue(DraftStage::Prepared));
    }

    #[test]
    fn rebuild_tree_reinserts_failed_stuck_leaf_for_witness() {
        use crate::model::cvdr::{CvdrDraft, CvdrStore, ReceiptTree};
        use ic_certification::{HashTree, LookupResult};

        let mut store = CvdrStore::default();
        let stuck = CvdrDraft {
            user_id: uid(1),
            user_canister_id: Principal::from_slice(&[1u8; 29]),
            index_canister_id: Principal::from_slice(&[2u8; 29]),
            record_id: [1u8; 32],
            deletion_seq: 1,
            nonce: [2u8; 32],
            receipt_id: [9u8; 32],
            module_hash_pre: vec![],
            executor_module_hash: vec![7u8; 32],
            h_user_pre: [3u8; 32],
            h_index: [4u8; 32],
            commitment: [5u8; 32],
            salt: [0xABu8; 32],
            canisters_to_notify: vec![Principal::from_slice(&[5u8; 29])],
            uninstall_completed_at: 111,
            receipt_committed_at: 222,
            finalize_attempt: 3,
            finalize_last_attempt_at: 333,
            created_at: 1,
            attempt: 0,
            stage: DraftStage::FailedStuck,
        };
        let expected_hash = stuck.receipt_hash();
        let stuck_id = stuck.receipt_id;
        store.upsert_draft(stuck);

        let prepared_id = [7u8; 32];
        store.upsert_draft(CvdrDraft {
            user_id: uid(2),
            user_canister_id: Principal::from_slice(&[8u8; 29]),
            index_canister_id: Principal::from_slice(&[2u8; 29]),
            record_id: [8u8; 32],
            deletion_seq: 1,
            nonce: [2u8; 32],
            receipt_id: prepared_id,
            module_hash_pre: vec![],
            executor_module_hash: vec![],
            h_user_pre: [0u8; 32],
            h_index: [0u8; 32],
            commitment: [0u8; 32],
            salt: [0xCDu8; 32],
            canisters_to_notify: vec![],
            uninstall_completed_at: 0,
            receipt_committed_at: 0,
            finalize_attempt: 0,
            finalize_last_attempt_at: 0,
            created_at: 1,
            attempt: 0,
            stage: DraftStage::Prepared,
        });

        // Simulate upgrade: heap tree wiped, rebuild from durable drafts.
        let mut tree = ReceiptTree::default();
        rebuild_receipt_tree_from_durable(&store, &mut tree);
        assert!(!tree.is_empty());

        let witness: HashTree =
            serde_cbor::from_slice(&tree.witness_cbor(&stuck_id)).expect("witness decodes");
        assert_eq!(witness.digest(), tree.root());
        match witness.lookup_path([b"receipts".as_slice(), stuck_id.as_slice()]) {
            LookupResult::Found(v) => assert_eq!(v, expected_hash.as_slice()),
            other => panic!("FailedStuck leaf missing after rebuild: {other:?}"),
        }
        match witness.lookup_path([b"receipts".as_slice(), prepared_id.as_slice()]) {
            LookupResult::Found(_) => panic!("Prepared must not appear as a receipt leaf"),
            _ => {}
        }

        // Cleanup shared stable draft memory for other unit tests in this process.
        store.remove_draft(&Principal::from_slice(&[1u8; 29]));
        store.remove_draft(&Principal::from_slice(&[8u8; 29]));
    }

    #[test]
    fn success_always_schedules_immediately() {
        assert_eq!(successor_delay_ms(ScheduleKind::Success, true), None);
        assert_eq!(successor_delay_ms(ScheduleKind::Success, false), None);
    }

    #[test]
    fn retry_backs_off_only_when_same_user_is_next() {
        assert_eq!(
            successor_delay_ms(ScheduleKind::Retry, true),
            Some(FAST_RETRY_INTERVAL_MS)
        );
        assert_eq!(successor_delay_ms(ScheduleKind::Retry, false), None);
    }

    #[test]
    fn retry_backoff_is_thirty_seconds() {
        assert_eq!(FAST_RETRY_INTERVAL_MS, 30_000);
    }

    #[test]
    fn success_with_others_waiting_runs_immediately() {
        assert_eq!(delay_after(ScheduleKind::Success, &[2, 3], 1), Some(None));
    }

    #[test]
    fn success_alone_leaves_queue_idle() {
        assert_eq!(delay_after(ScheduleKind::Success, &[], 1), None);
    }

    #[test]
    fn retry_alone_backs_off_after_requeue() {
        assert_eq!(
            delay_after(ScheduleKind::Retry, &[], 1),
            Some(Some(FAST_RETRY_INTERVAL_MS))
        );
    }

    #[test]
    fn retry_with_other_ahead_runs_immediately() {
        assert_eq!(delay_after(ScheduleKind::Retry, &[2], 1), Some(None));
    }

    #[test]
    fn retry_does_not_backoff_when_same_user_is_only_behind_another() {
        // After push_back(1), front is 2 — must not treat attempted user as front.
        assert_eq!(delay_after(ScheduleKind::Retry, &[2, 3], 1), Some(None));
    }

    #[test]
    fn k_successes_never_accumulate_thirty_second_gaps() {
        let mut forced_backoff_ms = 0u64;
        for i in 0..8u8 {
            let remaining: Vec<u8> = ((i + 1)..8).collect();
            if let Some(Some(ms)) = delay_after(ScheduleKind::Success, &remaining, i) {
                forced_backoff_ms = forced_backoff_ms.saturating_add(ms);
            }
        }
        assert_eq!(forced_backoff_ms, 0);
    }
}
