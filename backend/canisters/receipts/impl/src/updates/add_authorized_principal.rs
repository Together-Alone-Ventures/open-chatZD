use crate::guards::caller_is_controller;
use crate::mutate_state;
use canister_api_macros::update;
use canister_tracing_macros::trace;
use receipts_canister::add_authorized_principal::*;
use types::SuccessOnly;

/// Controller-gated. Grants `store` rights to `args.principal` (e.g. the
/// local_user_index export orchestrator). Env/operator-driven (§5).
#[update(guard = "caller_is_controller", msgpack = true)]
#[trace]
fn add_authorized_principal(args: Args) -> Response {
    mutate_state(|state| state.data.authorized_principals.insert(args.principal));
    SuccessOnly::Success
}
