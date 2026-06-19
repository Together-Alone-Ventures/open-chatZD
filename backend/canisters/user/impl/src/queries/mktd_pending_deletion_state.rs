use crate::read_state;
use canister_api_macros::query;
use user_canister::mktd_pending_deletion_state::*;

/// MKTd02 pending-finalization-state query (S8) for P3 recovery detection.
/// Read-only; reports whether a pending (post-Phase-A, pre-finalize) receipt
/// exists and its receipt id. Leaks no raw principal / raw record_id. Not
/// owner-guarded — low-sensitivity status used by the recovery tooling.
#[query(msgpack = true)]
fn mktd_pending_deletion_state(_args: Args) -> Response {
    let tombstoned = read_state(|state| state.data.pii_tombstoned);

    if !mktd02::is_initialised() {
        return Response { pending: false, tombstoned, receipt_id: None };
    }

    let pending = mktd02::is_pending_finalization();
    let receipt_id = if pending {
        // get_pending_certificate() resolves the persisted pending id in query
        // context; receipt_id is a hash (no raw identifier leaked).
        mktd02::get_pending_certificate().map(|pc| pc.receipt_id.to_vec())
    } else {
        None
    };

    Response { pending, tombstoned, receipt_id }
}
