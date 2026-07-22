use crate::{RuntimeState, read_state};
use ic_cdk::query;
use local_user_index_canister::get_cvdr::{FrozenWire, PendingInfo, Response::*, *};

/// Public fetch by the unguessable bearer `receipt_id` (spec §11.1/§11.2). Canonical for
/// canister-to-canister, test, and byte-equality surfaces; the HTTP route is canonical for
/// users and frontend V1.
///
/// Serves FACTS, NOT VERDICTS (§11.2): no tier (`VerifiedFinal` / `LateFinalized`) is reported
/// on this or any surface. Tiers are verifier-derived by CVDR-Verify from `certificate_time`
/// against the window; the serving layer never classifies.
///
/// `Available` is checked before `Pending`, so a post-capture (scrubbed) draft — which still
/// exists alongside its stored package — reports Available rather than Pending forever.
#[query]
fn get_cvdr(args: Args) -> Response {
    read_state(|state| get_cvdr_impl(args, state))
}

fn get_cvdr_impl(args: Args, state: &RuntimeState) -> Response {
    if let Some(package) = state.data.cvdr.get_frozen_package(&args.receipt_id) {
        // §11.3: the stored package, through the single constructor. Never re-derived.
        Available(FrozenWire::from(&package))
    } else if state.data.cvdr.find_any_draft_by_receipt_id(&args.receipt_id).is_some() {
        // §11.2: Pending covers every pre-package draft stage, and covers "stuck".
        Pending(PendingInfo::default())
    } else {
        NotFound
    }
}
