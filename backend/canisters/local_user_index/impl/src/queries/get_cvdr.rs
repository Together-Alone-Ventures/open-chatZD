use crate::model::cvdr::ReleasedCvdr;
use crate::{RuntimeState, read_state};
use ic_cdk::query;
use local_user_index_canister::get_cvdr::{Response::*, *};

// Public query — keyed by the unguessable `receipt_id` (the public fetch capability). The
// `record_id` path is deliberately NOT exposed here in build-v1 (record_id is derivable from
// the public UserId, so it is restricted).
#[query]
fn get_cvdr(args: Args) -> Response {
    read_state(|state| get_cvdr_impl(args, state))
}

fn get_cvdr_impl(args: Args, state: &RuntimeState) -> Response {
    match state.data.cvdr.get_by_receipt_id(&args.receipt_id) {
        Some(cvdr) => Success(into_receipt(cvdr)),
        None => NotFound,
    }
}

fn into_receipt(c: ReleasedCvdr) -> CvdrReceipt {
    CvdrReceipt {
        encoder_version: c.encoder_version,
        receipt_id: c.receipt_id,
        record_id: c.record_id,
        deletion_seq: c.deletion_seq,
        user_canister_id: c.user_canister_id,
        index_canister_id: c.index_canister_id,
        module_hash_pre: c.module_hash_pre,
        executor_module_hash: c.executor_module_hash,
        h_user_pre: c.h_user_pre,
        h_index: c.h_index,
        commitment: c.commitment,
        certificate: c.certificate,
        created_at: c.created_at,
        finalized_at: c.finalized_at,
    }
}
