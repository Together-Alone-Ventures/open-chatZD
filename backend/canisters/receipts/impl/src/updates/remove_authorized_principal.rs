use crate::guards::caller_is_controller;
use crate::mutate_state;
use canister_api_macros::update;
use canister_tracing_macros::trace;
use receipts_canister::remove_authorized_principal::*;
use types::SuccessOnly;

/// Controller-gated. Revokes `args.principal`'s `store` rights. Idempotent.
#[update(guard = "caller_is_controller", msgpack = true)]
#[trace]
fn remove_authorized_principal(args: Args) -> Response {
    mutate_state(|state| state.data.authorized_principals.remove(&args.principal));
    SuccessOnly::Success
}
