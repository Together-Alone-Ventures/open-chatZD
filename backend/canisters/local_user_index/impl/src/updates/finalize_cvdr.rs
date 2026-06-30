use crate::model::cvdr::{self, ReleasedCvdr};
use crate::{RuntimeState, mutate_state};
use ic_cdk::update;
use local_user_index_canister::finalize_cvdr::{Response::*, *};
use tracing::warn;

// The certificate is the authorization (NNS-signed, only producible after this canister
// published the exact commitment), so this ingress endpoint accepts any caller acting as a
// relay; it can only complete a deletion the index itself initiated. See `inspect_message`.
#[update]
fn finalize_cvdr(args: Args) -> Response {
    mutate_state(|state| finalize_cvdr_impl(args, state))
}

fn finalize_cvdr_impl(args: Args, state: &mut RuntimeState) -> Response {
    // Single-slot guard: only the receipt currently holding the certified-data slot can be
    // finalized. If it is no longer pending but already stored, this is an idempotent retry.
    if state.data.cvdr_awaiting_receipt_id != Some(args.receipt_id) {
        if state.data.cvdr.get_by_receipt_id(&args.receipt_id).is_some() {
            return Success;
        }
        return NotPending;
    }

    let Some(draft) = state
        .data
        .cvdr
        .awaiting_certificate_draft()
        .filter(|d| d.receipt_id == args.receipt_id)
    else {
        return NotPending;
    };

    // SECURITY-CRITICAL: fully verify the certificate (BLS -> subnet delegation -> NNS root
    // key, + time) AND that it certifies THIS canister's certified_data == the pending
    // commitment. An unverified certificate must not finalize or complete the deletion.
    let self_canister_id = state.env.canister_id();
    let ic_root_key = state.env.ic_root_key();
    let now = state.env.now();
    if !cvdr::certificate_verifies_commitment(&args.certificate, self_canister_id, &ic_root_key, now, &draft.commitment) {
        return CertificateMismatch;
    }

    // CORROBORATION ONLY (never substitution): the captured executor module hash (taken
    // pre-uninstall) is authoritative for H_index. Comparing it to the index's CURRENT deployed
    // hash only tells us whether an index upgrade intervened mid-flight. The captured value
    // always wins; the live value never overrides or substitutes it. A mismatch is expected and
    // tolerated (the receipt records the wasm that actually performed the de-reference).
    if state.data.executor_module_hash.as_slice() != draft.executor_module_hash.as_slice() {
        warn!(
            event = "cvdr_executor_hash_changed_mid_flight",
            user_canister_id = %draft.user_canister_id,
            "index upgraded between capture and finalize; receipt records the captured (pre-upgrade) executor hash"
        );
    }

    state.data.cvdr.store_released(ReleasedCvdr {
        encoder_version: cvdr::CVDR_ENCODER_VERSION.to_string(),
        receipt_id: draft.receipt_id,
        record_id: draft.record_id,
        deletion_seq: draft.deletion_seq,
        user_canister_id: draft.user_canister_id,
        index_canister_id: draft.index_canister_id,
        module_hash_pre: draft.module_hash_pre.clone(),
        // Authoritative captured executor hash — NOT the live/current value.
        executor_module_hash: draft.executor_module_hash.clone(),
        h_user_pre: draft.h_user_pre,
        h_index: draft.h_index,
        commitment: draft.commitment,
        certificate: args.certificate,
        created_at: draft.created_at,
        finalized_at: now,
    });

    // Completion is re-gated on the releasable CVDR being durably stored (done above):
    // remove the user mappings + fire the membership-removal notifications.
    crate::jobs::delete_users::complete_deletion(state, draft.user_id, draft.canisters_to_notify.clone());

    // Free the single certified-data slot, drop the draft, and re-arm the job so the next
    // queued deletion can claim the slot.
    state.data.cvdr.remove_draft(&draft.user_canister_id);
    state.data.cvdr_awaiting_receipt_id = None;
    crate::jobs::delete_users::start_job_if_required(state, None);

    Success
}
