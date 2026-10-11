use crate::memory::get_upgrades_memory;
use crate::take_state;
use canister_tracing_macros::trace;
use ic_cdk::pre_upgrade;
use rand::Rng;
use stable_memory::get_writer;
use tracing::info;

#[pre_upgrade]
#[trace]
fn pre_upgrade() {
    // R-2 upgrade interlock — FIRST, before any state is taken or serialised. While a receipt is
    // inside `uninstall → index evidence stored`, the code that performed the deletion must stay
    // installed; trapping here fails `install_code` and leaves this wasm and its state untouched.
    let refusal = crate::read_state(|state| {
        let blockers = state.data.cvdr.evidence_capturable(
            state.env.now().saturating_mul(1_000_000),
            state.data.cvdr_code_epoch_started_at_ns,
        );
        crate::model::cvdr::evidence_upgrade_refusal(&blockers)
    });
    if let Some(refusal) = refusal {
        ic_cdk::trap(refusal);
    }

    info!("Pre-upgrade starting");

    let mut state = take_state();
    state.data.rng_seed = state.env.rng().r#gen();

    let errors = canister_logger::export_errors();
    let logs = canister_logger::export_logs();
    let traces = canister_logger::export_traces();

    let stable_state = (&state.data, errors, logs, traces);

    let mut memory = get_upgrades_memory();
    let writer = get_writer(&mut memory);

    msgpack::serialize(stable_state, writer).unwrap();
}
