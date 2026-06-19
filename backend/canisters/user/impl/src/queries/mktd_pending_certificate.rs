use canister_api_macros::query;
use user_canister::mktd_pending_certificate::{Response::*, *};

/// MKTd02 Phase B (S3). **Query** — `data_certificate()` is query-only.
///
/// Returns the BLS certificate + receipt binding for the pending receipt, to be
/// passed to Phase C. Not owner-guarded: it returns NO PII — only the certified
/// commitment, the IC-supplied certificate (verification material meant to be
/// exported/verified off-chain), and the receipt id (a hash). See P1 NOTES (W5).
#[query(msgpack = true)]
fn mktd_pending_certificate(_args: Args) -> Response {
    if !mktd02::is_initialised() {
        return NotPending;
    }
    match mktd02::get_pending_certificate() {
        Some(pc) => Success(PendingCertificate {
            receipt_id: pc.receipt_id.to_vec(),
            certified_commitment: pc.certified_commitment.to_vec(),
            certificate: pc.certificate,
        }),
        None => NotPending,
    }
}
