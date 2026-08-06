use crate::guards::caller_is_owner;
use crate::mutate_state_bypass;
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
        Ok(()) => {
            // Post-tombstone host writes are blocked except this narrow
            // finalized-receipt-id pointer capture needed for post-uninstall
            // export — so it MUST use `mutate_state_bypass` (the D8 write-guard
            // would otherwise trap). Immutable once set: an idempotent same-value
            // set is allowed; a different value is rejected (never overwritten,
            // never cleared), so retry / export-failure paths cannot rewrite it.
            let outcome =
                mutate_state_bypass(|state| record_finalized_receipt_id(&mut state.data.mktd_finalized_receipt_id, receipt_id));
            match outcome {
                Ok(()) => Success,
                Err(()) => Error("finalized receipt id already set to a different value".to_string()),
            }
        }
        Err(e) => Error(e.to_string()),
    }
}

/// Immutable-once-set capture of the finalized `receipt_id` pointer.
/// - unset (`None`) → set it,
/// - already set to the SAME id → idempotent `Ok` (no change),
/// - already set to a DIFFERENT id → `Err` (never overwrite, never clear).
///
/// There is no clear/reset path: nothing — including the retry / export-failure
/// paths — can blank this or point it at a different id once set.
fn record_finalized_receipt_id(slot: &mut Option<[u8; 32]>, receipt_id: [u8; 32]) -> Result<(), ()> {
    match *slot {
        Some(existing) if existing == receipt_id => Ok(()),
        Some(_) => Err(()),
        None => {
            *slot = Some(receipt_id);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::record_finalized_receipt_id;

    const ID_A: [u8; 32] = [0xAA; 32];
    const ID_B: [u8; 32] = [0xBB; 32];

    #[test]
    fn sets_when_unset() {
        let mut slot = None;
        assert_eq!(record_finalized_receipt_id(&mut slot, ID_A), Ok(()));
        assert_eq!(slot, Some(ID_A));
    }

    #[test]
    fn same_value_set_is_idempotent() {
        let mut slot = Some(ID_A);
        assert_eq!(record_finalized_receipt_id(&mut slot, ID_A), Ok(()));
        assert_eq!(slot, Some(ID_A), "idempotent set must not change the value");
    }

    #[test]
    fn different_value_is_rejected_and_does_not_overwrite() {
        let mut slot = Some(ID_A);
        assert_eq!(record_finalized_receipt_id(&mut slot, ID_B), Err(()));
        assert_eq!(
            slot,
            Some(ID_A),
            "a different-id set must be rejected and leave the original intact"
        );
    }

    #[test]
    fn there_is_no_clear_path() {
        // The function only ever sets a value or leaves it unchanged — it can
        // never produce `None` from `Some`. Exhaustively: from Some(A), neither a
        // same-value nor a different-value call clears the slot.
        let mut slot = Some(ID_A);
        let _ = record_finalized_receipt_id(&mut slot, ID_A);
        let _ = record_finalized_receipt_id(&mut slot, ID_B);
        assert_eq!(slot, Some(ID_A), "retry / failure paths cannot clear the finalized id");
    }
}
