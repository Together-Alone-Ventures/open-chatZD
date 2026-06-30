use crate::Data;
use crate::lifecycle::{init_env, init_state};
use crate::memory::{get_stable_memory_map_memory, get_upgrades_memory};
use crate::wasm_hash::deployed_module_hash;
use canister_logger::LogEntry;
use canister_tracing_macros::trace;
use ic_cdk::post_upgrade;
use local_user_index_canister::post_upgrade::Args;
use stable_memory::get_reader;
use tracing::info;
use utils::cycles::init_cycles_dispenser_client;

#[post_upgrade]
#[trace]
fn post_upgrade(args: Args) {
    stable_memory_map::init(get_stable_memory_map_memory());

    let memory = get_upgrades_memory();
    let reader = get_reader(&memory);

    let (mut data, errors, logs, traces): (Data, Vec<LogEntry>, Vec<LogEntry>, Vec<LogEntry>) =
        msgpack::deserialize(reader).unwrap();

    if data.user_canister_module_hash == [0; 32] {
        let user_wasm = &data
            .child_canister_wasms
            .get(local_user_index_canister::ChildCanisterType::User)
            .wasm
            .module;
        if !user_wasm.is_empty() {
            data.user_canister_module_hash =
                deployed_module_hash(user_wasm).expect("user canister wasm hash");
        }
    }

    // CVDR v5: refresh the deploy-supplied executor module hash (H_index provenance) to the
    // newly-installed wasm. Any draft captured before this upgrade keeps its PRE-upgrade hash
    // (the captured value is authoritative — see finalize_cvdr corroboration).
    data.executor_module_hash = args.executor_module_hash;

    canister_logger::init_with_logs(data.test_mode, errors, logs, traces);

    let env = init_env(data.rng_seed);
    init_cycles_dispenser_client(data.cycles_dispenser_canister_id, data.test_mode);
    init_state(env, data, args.wasm_version);

    let total_instructions = ic_cdk::api::call_context_instruction_counter();
    info!(version = %args.wasm_version, total_instructions, "Post-upgrade complete");
}
