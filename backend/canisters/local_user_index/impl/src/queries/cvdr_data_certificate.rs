use crate::{RuntimeState, read_state};
use ic_cdk::query;
use local_user_index_canister::cvdr_data_certificate::{Response::*, *};

// Public query — the certificate certifies a public commitment and is itself NNS-signed, so
// there is nothing to gate. The external finalizer polls this, then relays via `finalize_cvdr`.
#[query]
fn cvdr_data_certificate(_args: Args) -> Response {
    // `data_certificate()` only returns the certificate in a (non-replicated) query context.
    let Some(certificate) = ic_cdk::api::data_certificate() else {
        return NotAvailable;
    };
    read_state(|state| cvdr_data_certificate_impl(certificate, state))
}

fn cvdr_data_certificate_impl(certificate: Vec<u8>, state: &RuntimeState) -> Response {
    match (state.data.cvdr_awaiting_receipt_id, state.data.cvdr.awaiting_certificate_draft()) {
        (Some(receipt_id), Some(draft)) if draft.receipt_id == receipt_id => Success(SuccessResult {
            receipt_id,
            commitment: draft.commitment,
            certificate,
        }),
        _ => NotAvailable,
    }
}
