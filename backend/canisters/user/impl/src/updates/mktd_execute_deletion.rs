use crate::guards::caller_is_owner;
use crate::{mktd, read_state};
use canister_api_macros::update;
use canister_tracing_macros::trace;
use user_canister::mktd_execute_deletion::{Response::*, *};

/// MKTd02 Phase A (S2). Owner-guarded. Tombstones PII, publishes the certified
/// commitment, takes the finalization lock, returns the pending receipt id.
///
/// Drives the engine **directly**, NOT through `execute_update`: the engine
/// calls back into the adapter (`get_state_bytes` / `tombstone_state`) which
/// take their own `read_state` / `mutate_state` borrows, so the handler must
/// hold none. (`execute_update` would also self-trip the D8 block after
/// tombstoning.)
#[update(guard = "caller_is_owner", msgpack = true)]
#[trace]
fn mktd_execute_deletion(_args: Args) -> Response {
    // record_id (D7/S5): domain-tagged hash of this user's durable UserId
    // (the canister's own principal) — never `caller()`.
    let record_id = read_state(|state| mktd::record_id_for(state.env.canister_id().into()));

    let mut adapter = mktd::MKTdUserAdapter;
    let config = mktd::config();

    match mktd02::execute_deletion_with_record_id(&mut adapter, &config, record_id) {
        Ok(receipt_id) => Success(receipt_id.to_vec()),
        Err(e) => Error(e.to_string()),
    }
}
