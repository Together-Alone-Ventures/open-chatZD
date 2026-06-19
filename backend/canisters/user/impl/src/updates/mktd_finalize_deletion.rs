use crate::guards::caller_is_owner;
use canister_api_macros::update;
use canister_tracing_macros::trace;
use user_canister::mktd_finalize_deletion::{Response::*, *};

/// MKTd02 Phase C (S4). Owner-guarded. The owner-guard IS the host
/// authorization the engine requires *before* calling
/// `finalize_receipt_after_host_authorization` (A2 performs no caller/controller
/// check of its own). A2 is reachable ONLY through this guarded wrapper.
///
/// Does NOT route through `execute_update`: `pii_tombstoned` is set, so the D8
/// block would trap. The engine's A2 guards (lock held / id-match /
/// already-finalized) are surfaced verbatim.
#[update(guard = "caller_is_owner", msgpack = true)]
#[trace]
fn mktd_finalize_deletion(args: Args) -> Response {
    let receipt_id: [u8; 32] = match args.receipt_id.as_slice().try_into() {
        Ok(id) => id,
        Err(_) => return Error("InvalidReceiptId: expected exactly 32 bytes".to_string()),
    };

    match mktd02::finalize_receipt_after_host_authorization(&receipt_id, args.certificate) {
        Ok(()) => Success,
        Err(e) => Error(e.to_string()),
    }
}
