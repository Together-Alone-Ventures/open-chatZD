//! Self-finalization loop (spec §6 — HARD SECURITY RULE).
//!
//! For each receipt at [`DraftStage::AwaitingCertificate`], the index captures its OWN IC
//! certificate without any external service (A1-proven): a periodic sweep drives due receipts
//! through a non-replicated `http_request` outcall (the [`http_outcall`] shim) to the canister's
//! own raw-domain `GET /cvdr_live/<receipt_id>` query route, which returns
//! `{receipt_body, witness, data_certificate()}`. The response is then put through the
//! **store-gate**: FULL on-chain verification BEFORE store ([`cvdr::verify_finalization_package`])
//! — BLS -> NNS delegation -> range covers self -> witness reconstructs -> `certified_data` ==
//! root -> witness leaf == receipt_hash -> window. Only a verified, in-window package is stored
//! (via the Slice-1 insert-only writer); anything else is discarded and retried, never stored, so
//! an untrusted single node cannot poison the first-wins slot with a forged package.
//!
//! Upgrade-robust: the sweep timer is heap-resident and re-armed from `resume_in_flight_drafts`
//! (post-upgrade). Backoff per receipt: ~3 s first attempt, then 5 s -> 15 s -> 45 s -> 2 m cap;
//! after 24 h with no verified in-window certificate the receipt goes [`DraftStage::FailedStuck`]
//! (the receipt stays in the tree — §10 — and the §7 backstop can still land it as LateFinalized).

use crate::model::cvdr::{self, CvdrDraft, DraftStage, FinalizeVerdict, FrozenCvdrPackage, FrozenInsertError};
use crate::model::http_outcall;
use crate::{RuntimeState, mutate_state, read_state};
use constants::SECOND_IN_MS;
use ic_cdk_timers::TimerId;
use std::cell::Cell;
use std::time::Duration;
use tracing::{trace, warn};
use types::{CanisterId, Milliseconds};

thread_local! {
    static TIMER_ID: Cell<Option<TimerId>> = Cell::default();
}

/// Sweep cadence. Also the first-attempt delay (~3 s post-publish, spec §6).
const SWEEP_INTERVAL_MS: Milliseconds = 3 * SECOND_IN_MS;
/// Tight initial response cap (spec §6); one retry at the larger cap on a size/parse miss.
const MAX_RESPONSE_BYTES: u64 = 16 * 1024;
const RETRY_RESPONSE_BYTES: u64 = 64 * 1024;
/// Retry cap: after this long with no verified in-window certificate, the receipt is FailedStuck.
const FINALIZE_GIVE_UP_MS: u64 = 24 * 60 * 60 * 1_000;
const NS_PER_MS: u64 = 1_000_000;

/// Backoff (ms) before finalize attempt `attempt` (0-indexed): 3 s initial, then 5 / 15 / 45 s,
/// then a 2 m cap (spec §6).
fn backoff_ms(attempt: u32) -> u64 {
    match attempt {
        0 => 3 * SECOND_IN_MS,
        1 => 5 * SECOND_IN_MS,
        2 => 15 * SECOND_IN_MS,
        3 => 45 * SECOND_IN_MS,
        _ => 120 * SECOND_IN_MS,
    }
}

/// Arm the sweep timer if there is finalization work and it is not already armed. Idempotent.
/// Called after a receipt is published (reaches AwaitingCertificate) and from post-upgrade resume.
pub(crate) fn start_if_required(state: &RuntimeState) -> bool {
    if TIMER_ID.get().is_none() && state.data.cvdr.awaiting_certificate_count() > 0 {
        let timer_id = ic_cdk_timers::set_timer(Duration::from_millis(SWEEP_INTERVAL_MS), run_sweep);
        TIMER_ID.set(Some(timer_id));
        true
    } else {
        false
    }
}

fn run_sweep() {
    TIMER_ID.set(None);
    let now_ns = ic_cdk::api::time();

    // Select due receipts and RESERVE each attempt (bump attempt + last_attempt_at) so a slow
    // in-flight attempt is not re-fired by the next sweep. Mark over-cap receipts FailedStuck.
    let due: Vec<CvdrDraft> = mutate_state(|state| {
        let mut due = Vec::new();
        for draft in state.data.cvdr.drafts_awaiting_certificate() {
            if now_ns.saturating_sub(draft.receipt_committed_at) > FINALIZE_GIVE_UP_MS * NS_PER_MS {
                let mut d = draft.clone();
                d.stage = DraftStage::FailedStuck;
                state.data.cvdr.upsert_draft(d);
                warn!(
                    event = "cvdr_finalize_failed_stuck",
                    user_canister_id = %draft.user_canister_id,
                    attempts = draft.finalize_attempt,
                    "no verified in-window certificate captured before the 24h cap; receipt stays in the tree, backstop can still finalize"
                );
                continue;
            }
            let due_at_ns = if draft.finalize_attempt == 0 {
                draft.receipt_committed_at + backoff_ms(0) * NS_PER_MS
            } else {
                draft.finalize_last_attempt_at + backoff_ms(draft.finalize_attempt) * NS_PER_MS
            };
            if now_ns >= due_at_ns {
                let mut d = draft.clone();
                d.finalize_attempt = d.finalize_attempt.saturating_add(1);
                d.finalize_last_attempt_at = now_ns;
                state.data.cvdr.upsert_draft(d.clone());
                due.push(d);
            }
        }
        due
    });

    for draft in due {
        ic_cdk::futures::spawn(attempt_finalize(draft));
    }

    // Re-arm while work remains.
    mutate_state(|state| {
        start_if_required(state);
    });
}

/// One finalization attempt for a single receipt: self-fetch (16 KB, one 64 KB retry) then the
/// store-gate. Never traps; on any miss it simply returns and the next sweep retries with backoff.
async fn attempt_finalize(draft: CvdrDraft) {
    let self_id = read_state(|state| state.env.canister_id());

    // Self-fetch the live certification payload; one retry at the larger cap on outcall/parse miss.
    let parsed = match fetch_live(self_id, &draft.receipt_id, MAX_RESPONSE_BYTES).await {
        Some(p) => Some(p),
        None => fetch_live(self_id, &draft.receipt_id, RETRY_RESPONSE_BYTES).await,
    };
    let Some((certificate, witness)) = parsed else {
        trace!(receipt_prefix = %cvdr::receipt_id_prefix(&draft.receipt_id), "cvdr self-fetch miss; will retry");
        return;
    };

    mutate_state(|state| {
        // Re-read the durable draft; skip if it is no longer awaiting (idempotent — first verified
        // package wins; a concurrent attempt may already have captured it).
        let Some(d) = state.data.cvdr.get_draft(&draft.user_canister_id) else {
            return;
        };
        if d.stage != DraftStage::AwaitingCertificate {
            return;
        }

        let receipt_hash = d.receipt_hash();
        let now = state.env.now();
        let ic_root_key = state.env.ic_root_key();

        match cvdr::verify_finalization_package(
            &certificate,
            &witness,
            &d.receipt_id,
            &receipt_hash,
            self_id,
            &ic_root_key,
            now,
            d.receipt_committed_at,
        ) {
            FinalizeVerdict::InWindow { cert_time_ns, tree_root } => {
                let package = FrozenCvdrPackage {
                    // Canonical body recomputed from the durable draft (== the route's receipt_body).
                    receipt_body: d.receipt_body(),
                    receipt_hash,
                    tree_root,
                    witness_bytes: witness,
                    certificate_bytes: certificate,
                    certificate_time: cert_time_ns,
                };
                match state.data.cvdr.insert_frozen_package(d.receipt_id, d.record_id, d.deletion_seq, package) {
                    Ok(()) | Err(FrozenInsertError::AlreadyExists) => {
                        // Stored (or already stored — idempotent). Terminal success. Scrub the
                        // retained draft of the salt + raw target list (privacy default, spec §6);
                        // the frozen package is the durable record.
                        let mut captured = d.clone();
                        captured.stage = DraftStage::CertificateCaptured;
                        captured.scrub_sensitive();
                        state.data.cvdr.upsert_draft(captured);
                        trace!(receipt_prefix = %cvdr::receipt_id_prefix(&d.receipt_id), "cvdr certificate captured + stored");
                    }
                    Err(FrozenInsertError::LogFull) => {
                        warn!(event = "cvdr_frozen_log_full", receipt_prefix = %cvdr::receipt_id_prefix(&d.receipt_id), "frozen-package log full");
                    }
                }
            }
            FinalizeVerdict::Late { cert_time_ns, .. } => {
                // Verified but LATE (spec §5/§6): the self-loop NEVER stores a late package — that
                // is the §7 backstop's `LateFinalized`. No durable state change here; the draft
                // stays AwaitingCertificate (until the 24h cap flips it to FailedStuck), and the
                // backstop can still land it. In practice this is rare: the give-up cap == the
                // window, so a self-loop attempt is normally in-window or already stuck.
                trace!(
                    receipt_prefix = %cvdr::receipt_id_prefix(&d.receipt_id),
                    cert_time_ns,
                    committed_at = d.receipt_committed_at,
                    "self-finalization certificate is late (outside the window); not stored by the self-loop (backstop handles as LateFinalized)"
                );
            }
            FinalizeVerdict::Reject(reason) => {
                // HARD RULE (narrow, exact claim): a failed verification NEVER creates a frozen
                // package (this arm mutates NO durable state — it only logs). The retry bookkeeping
                // (`finalize_attempt`, `finalize_last_attempt_at`) is bumped separately, at sweep
                // dispatch (see `run_sweep`), by design so backoff advances — that is not "no state
                // change on failure"; it is the retry mechanism. The next sweep retries with backoff.
                warn!(
                    event = "cvdr_finalize_rejected",
                    receipt_prefix = %cvdr::receipt_id_prefix(&d.receipt_id),
                    reason = reason.as_str(),
                    attempt = d.finalize_attempt,
                    "self-finalization package failed the store-gate; discarded (not stored)"
                );
            }
        }
    });
}

/// Non-replicated self-fetch of `GET /cvdr_live/<receipt_id>`, returning `(certificate, witness)`
/// bytes on success. `None` on outcall failure, non-JSON body, missing certificate, or bad hex.
async fn fetch_live(self_id: CanisterId, receipt_id: &[u8; 32], max_response_bytes: u64) -> Option<(Vec<u8>, Vec<u8>)> {
    let url = format!("https://{}.raw.icp0.io/cvdr_live/{}", self_id, hex::encode(receipt_id));
    let result = http_outcall::non_replicated_get(url, max_response_bytes).await.ok()?;

    #[derive(serde::Deserialize)]
    struct LiveResponse {
        witness_cbor: String,
        certificate: Option<String>,
    }
    let parsed: LiveResponse = serde_json::from_slice(&result.body).ok()?;
    let certificate = hex::decode(parsed.certificate?).ok()?;
    let witness = hex::decode(parsed.witness_cbor).ok()?;
    Some((certificate, witness))
}
