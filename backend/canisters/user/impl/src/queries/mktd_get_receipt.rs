use canister_api_macros::query;
use user_canister::mktd_get_receipt::{Response::*, *};

/// Returns only finalized engine receipts. The receipt contains hashes and
/// verification material, not the adapter's plaintext PII projection.
#[query(msgpack = true)]
fn mktd_get_receipt(args: Args) -> Response {
    let Ok(receipt_id) = args.receipt_id.as_slice().try_into() else {
        return InvalidReceiptId;
    };

    match mktd02::get_receipt(&receipt_id) {
        Some(receipt) if receipt.bls_certificate.is_some() => Success(Box::new(receipt)),
        Some(_) => NotFinalized,
        None => NotFound,
    }
}
