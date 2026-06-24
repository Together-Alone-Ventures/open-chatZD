use crate::guards::caller_is_local_user_index;
use crate::read_state;
use canister_api_macros::query;
use user_canister::c2c_mktd_export_receipt::{Response::*, *};

/// P2 export source. Authorized to the local_user_index orchestrator only (same
/// guard as `c2c_groups_and_communities`, the sibling pre-uninstall c2c call).
///
/// Produces the canonical receipt bytes the SAME way as the test hook
/// (`serde_json::to_vec(&receipt)`); the P1f gate proved this is byte-identical
/// to the `/mktd_receipt` download route. Only FINALIZED receipts (BLS cert
/// present) are exportable — a pending/unfinalized receipt is never returned.
#[query(guard = "caller_is_local_user_index", msgpack = true)]
fn c2c_mktd_export_receipt(_args: Args) -> Response {
    let Some(receipt_id) = read_state(|state| state.data.mktd_finalized_receipt_id) else {
        return NotFinalized;
    };

    match mktd02::get_receipt(&receipt_id) {
        Some(receipt) if receipt.bls_certificate.is_some() => {
            let receipt_json = serde_json::to_vec(&receipt).expect("receipt serialization is infallible");
            Success(ExportedReceipt {
                receipt_id: receipt_id.to_vec(),
                receipt_json,
            })
        }
        _ => NotFinalized,
    }
}
