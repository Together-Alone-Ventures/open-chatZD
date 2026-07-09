use crate::{RuntimeState, read_state};
use ic_cdk::query;
use local_user_index_canister::get_cvdr::{Response::*, *};

// Public fetch by the unguessable `receipt_id`. The frozen-package delivery leg (spec §4
// Delivery — bearer-route shape + exposure policy) is a later slice; the legacy `ReleasedCvdr`
// store this served is stripped (never written since the finalization rework), so until the
// delivery leg lands this returns `NotFound`. The `Success(CvdrReceipt)` candid variant is
// retained (dormant) to avoid a candid/frontend change. Dev-branch interim, same class as the
// finalize_cvdr stub (spec §8b).
#[query]
fn get_cvdr(args: Args) -> Response {
    read_state(|state| get_cvdr_impl(args, state))
}

fn get_cvdr_impl(_args: Args, _state: &RuntimeState) -> Response {
    NotFound
}
