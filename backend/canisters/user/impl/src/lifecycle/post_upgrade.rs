use crate::Data;
use crate::lifecycle::init_state;
use crate::memory::{get_stable_memory_map_memory, get_upgrades_memory};
use canister_api_macros::post_upgrade;
use canister_logger::LogEntry;
use canister_tracing_macros::trace;
use stable_memory::get_reader;
use tracing::info;
use user_canister::post_upgrade::Args;
use utils::env::canister::CanisterEnv;

#[post_upgrade(msgpack = true)]
#[trace]
fn post_upgrade(args: Args) {
    stable_memory_map::init(get_stable_memory_map_memory());

    let memory = get_upgrades_memory();
    let reader = get_reader(&memory);

    let (data, errors, logs, traces): (Data, Vec<LogEntry>, Vec<LogEntry>, Vec<LogEntry>) =
        msgpack::deserialize(reader).unwrap();

    canister_logger::init_with_logs(data.test_mode, errors, logs, traces);

    let env = Box::new(CanisterEnv::new(data.rng_seed));
    init_state(env, data, args.wasm_version);

    // MKTd02: reconnect to the engine's stable slots (base 100) on the SAME
    // MemoryManager, recompute the state hash and re-publish the certified
    // commitment, and update module_hash. Runs AFTER state is restored and with
    // no borrow held. If a receipt is pending finalization, this traps by
    // design (finalize before upgrading) — see mktd02::on_post_upgrade.
    let module_hash = args.mktd_module_hash.unwrap_or([0u8; 32]);
    let adapter = crate::mktd::MKTdUserAdapter;
    crate::memory::with_memory_manager(|mm| {
        mktd02::on_post_upgrade(&adapter, mm, crate::mktd::config(), module_hash);
    });

    let total_instructions = ic_cdk::api::call_context_instruction_counter();
    info!(version = %args.wasm_version, total_instructions, "Post-upgrade complete");
}
