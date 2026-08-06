use crate::model::cvdr::{self, CvdrDraft, DraftStage, FinalizeVerdict, FrozenCvdrPackage, FrozenInsertError, Hash};
use crate::{RuntimeState, mutate_state};
use ic_cdk::update;
use local_user_index_canister::finalize_cvdr::{Response::*, *};
use tracing::{trace, warn};

// PERMISSIONLESS BACKSTOP (spec §7). Anyone may relay a `{certificate, witness}` snapshot they
// captured from this canister's own `/cvdr_live/<receipt_id>` query route; the certificate is the
// authorization (only the NNS can produce one, and only over this canister's certified receipt
// tree). The submission is stored ONLY if ALL eight §7 acceptance rules hold — this is the SAME
// store-gate the §6 self-loop uses (`cvdr::verify_finalization_package`), not a second verifier:
//
//   1. a receipt exists in a finalizable state (`AwaitingCertificate` / `FailedStuck`)  — rule 1
//   2. the submission matches the STORED receipt hash / root                            — rules 2–4
//   3. certificate verifies against the NNS root (full BLS + delegation + range)        — rule 3
//   4. witness proves `receipt_id -> receipt_hash`                                       — rule 4
//   5. certificate time satisfies the §5 window/tier rules                              — rules 5,8
//   6. first valid package wins                                                          — rule 6
//   7. an existing finalized package is NEVER overwritten                                — rule 7
//   8. a late package can only produce `LateFinalized`, never `VerifiedFinal`            — rule 8
//
// Rules 2–4 are enforced *against index-derived values*, not caller-supplied ones: `receipt_hash`
// is recomputed from the durable draft and the root is taken from the (BLS-verified) certificate,
// so the witness must bind OUR receipt under OUR certified root — a caller cannot substitute their
// own hash/root. Rules 6–7 are enforced by the insert-only frozen store (relied on, not
// re-implemented). Rule 8 is structural: the index stores the package but never stamps a tier; the
// in-window/late split is DERIVED from `certificate_time` (a package field) vs `receipt_committed_at`
// (hash-bound in `receipt_body`), so a verifier always reads a late package as `LateFinalized`.
// The handler runs in ONE message (no `await`), so it is atomic against the self-loop's separate
// continuation message: whichever lands first wins the insert-only slot; the loser is a no-op.
#[update]
fn finalize_cvdr(args: Args) -> Response {
    mutate_state(|state| finalize_cvdr_impl(args, state))
}

fn finalize_cvdr_impl(args: Args, state: &mut RuntimeState) -> Response {
    // Rule 7 (no overwrite) + rule 6 (first-wins): if a package is already stored for this receipt,
    // this submission is a no-op — the receipt is finalized. The insert-only store is authoritative;
    // this fast-path just gives the caller the honest `AlreadyFinalized` rather than a spurious
    // reject when the draft has already been captured + scrubbed.
    if state.data.cvdr.get_frozen_package(&args.receipt_id).is_some() {
        return AlreadyFinalized;
    }

    // Rule 1: a receipt must be in a finalizable state. `find_finalizable_draft_by_receipt_id`
    // matches `AwaitingCertificate` | `FailedStuck` — both retain the fields needed to recompute
    // `receipt_hash`, and both are legitimately backstop-finalizable (§7/§10). A captured/scrubbed
    // draft is excluded (its package would have been caught above).
    let Some(draft) = state.data.cvdr.find_finalizable_draft_by_receipt_id(&args.receipt_id) else {
        return NotPending;
    };

    let self_id = state.env.canister_id();
    let now = state.env.now();
    let ic_root_key = state.env.ic_root_key();
    // Index-derived (rule 2): the receipt hash the witness must bind, recomputed from the durable
    // draft — NOT taken from the submission.
    let receipt_hash = draft.receipt_hash();

    match cvdr::verify_finalization_package(
        &args.certificate,
        &args.witness,
        &draft.receipt_id,
        &receipt_hash,
        self_id,
        &ic_root_key,
        now,
        draft.receipt_committed_at,
    ) {
        // Rules 2–5 failed: NOTHING is stored (HARD SECURITY RULE — a forged/stale/mismatched
        // submission must not poison the first-wins slot). The reason is diagnostic only.
        FinalizeVerdict::Reject(reason) => {
            trace!(
                receipt_prefix = %cvdr::receipt_id_prefix(&draft.receipt_id),
                reason = reason.as_str(),
                "backstop submission rejected by the store-gate; nothing stored"
            );
            Rejected(reason.as_str().to_string())
        }
        // Verified, in-window (§5): store as a capture. Response `Captured` <-> DraftStage marker.
        FinalizeVerdict::InWindow { cert_time_ns, tree_root } => store_verified_package(
            state,
            draft,
            args.certificate,
            args.witness,
            receipt_hash,
            tree_root,
            cert_time_ns,
            DraftStage::CertificateCaptured,
            Captured,
        ),
        // Verified, LATE (§5/§7 rule 8): store, but it can only ever read as LateFinalized.
        FinalizeVerdict::Late { cert_time_ns, tree_root } => store_verified_package(
            state,
            draft,
            args.certificate,
            args.witness,
            receipt_hash,
            tree_root,
            cert_time_ns,
            DraftStage::LateFinalized,
            LateFinalized,
        ),
    }
}

/// Assemble the frozen package from the (verified) submission + the durable draft and insert it
/// (insert-only, enforcing rules 6–7). On success mark + scrub the draft; a lost race (the slot was
/// filled by the self-loop between the fast-path check and here — only possible ACROSS messages)
/// reads as `AlreadyFinalized`.
#[allow(clippy::too_many_arguments)]
fn store_verified_package(
    state: &mut RuntimeState,
    mut draft: CvdrDraft,
    certificate: Vec<u8>,
    witness: Vec<u8>,
    receipt_hash: Hash,
    tree_root: [u8; 32],
    cert_time_ns: u64,
    stage: DraftStage,
    success: Response,
) -> Response {
    let package = FrozenCvdrPackage {
        // Canonical body recomputed from the durable draft (== the /cvdr_live receipt_body). Must
        // be built BEFORE the scrub below, which drops the fields it needs.
        receipt_body: draft.receipt_body(),
        receipt_hash,
        tree_root,
        witness_bytes: witness,
        certificate_bytes: certificate,
        certificate_time: cert_time_ns,
    };
    match state
        .data
        .cvdr
        .insert_frozen_package(draft.receipt_id, draft.record_id, draft.deletion_seq, package)
    {
        Ok(()) => {
            draft.stage = stage;
            draft.scrub_sensitive();
            state.data.cvdr.upsert_draft(draft);
            crate::jobs::self_capture_index_evidence::start_if_required(state);
            success
        }
        // First-wins (rule 6/7): a concurrent path already stored it. No-op, not an error.
        Err(FrozenInsertError::AlreadyExists) => AlreadyFinalized,
        Err(FrozenInsertError::LogFull) => {
            warn!(event = "cvdr_frozen_log_full", receipt_prefix = %cvdr::receipt_id_prefix(&draft.receipt_id), "frozen-package log full");
            Rejected("frozen_log_full".to_string())
        }
    }
}
