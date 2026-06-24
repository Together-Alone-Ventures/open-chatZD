use crate::guards::caller_is_authorized;
use crate::model::receipt_store::StoreOutcome;
use crate::mutate_state;
use canister_api_macros::update;
use canister_tracing_macros::trace;
use mktd02::DeletionReceipt;
use receipts_canister::store::{Response::*, *};

/// Gated store (§2 WRITE / §3 store-and-serve). Authorized callers only (guard).
/// Validates enough metadata to refuse junk — but stores the supplied bytes
/// VERBATIM (never reserialized), so the served artifact stays byte-identical to
/// the user-canister export.
///
/// Validation:
/// - `receipt_id` is exactly 32 bytes,
/// - `receipt_json` parses as a finalized receipt (BLS certificate present),
/// - the receipt's embedded `receipt_id` matches the supplied key.
///
/// Idempotency (§2): same id + identical bytes → `AlreadyExists` (OK); same id +
/// different bytes → `Conflict` (hard reject). No body or identifier is logged.
#[update(guard = "caller_is_authorized", msgpack = true)]
#[trace]
fn store(args: Args) -> Response {
    let Ok(receipt_id) = <[u8; 32]>::try_from(args.receipt_id.as_slice()) else {
        return InvalidReceiptId;
    };

    // Parse for validation ONLY — the stored/served bytes are `args.receipt_json`
    // verbatim, not this reparsed value.
    let Ok(receipt) = serde_json::from_slice::<DeletionReceipt>(&args.receipt_json) else {
        return NotFinalized;
    };
    if receipt.bls_certificate.is_none() || receipt.receipt_id != receipt_id {
        return NotFinalized;
    }

    mutate_state(|state| match state.data.receipts.put(receipt_id, args.receipt_json) {
        StoreOutcome::Stored => Success,
        StoreOutcome::AlreadyExists => AlreadyExists,
        StoreOutcome::Conflict => Conflict,
    })
}
