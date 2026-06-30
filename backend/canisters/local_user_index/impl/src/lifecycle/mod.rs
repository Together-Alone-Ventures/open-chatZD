use crate::{Data, RuntimeState, WASM_VERSION};
use types::{BuildVersion, Timestamped};
use utils::env::Environment;
use utils::env::canister::CanisterEnv;

mod init;
mod inspect_message;
mod post_upgrade;
mod pre_upgrade;

fn init_env(rng_seed: [u8; 32]) -> Box<CanisterEnv> {
    Box::new(CanisterEnv::new(rng_seed))
}

fn init_state(env: Box<dyn Environment>, data: Data, wasm_version: BuildVersion) {
    let now = env.now();
    let mut state = RuntimeState::new(env, data);

    // v5: resume CVDR deletions that survived an upgrade — re-publish the pending certified
    // commitment (certified_data is cleared by upgrade) and re-enqueue mid-flight drafts the
    // volatile delete queue lost. No-op on a fresh install (no drafts).
    crate::jobs::delete_users::resume_in_flight_drafts(&mut state);

    crate::jobs::start(&state);
    crate::init_state(state);
    WASM_VERSION.set(Timestamped::new(wasm_version, now));
}
