use crate::lifecycle::init_state;
use crate::memory::get_stable_memory_map_memory;
use crate::{Data, mutate_state_bypass, openchat_bot};
use canister_tracing_macros::trace;
use ic_cdk::init;
use tracing::info;
use user_canister::init::Args;
use utils::env::Environment;
use utils::env::canister::CanisterEnv;

#[init]
#[trace]
fn init(args: Args) {
    canister_logger::init(args.test_mode);
    stable_memory_map::init(get_stable_memory_map_memory());

    let env = Box::new(CanisterEnv::new(args.rng_seed));
    let now = env.now();

    let data = Data::new(
        args.owner,
        args.user_index_canister_id,
        args.local_user_index_canister_id,
        args.group_index_canister_id,
        args.identity_canister_id,
        args.escrow_canister_id,
        args.video_call_operators,
        args.username,
        args.test_mode,
        args.referred_by,
        now,
    );

    init_state(env, data, args.wasm_version);

    // Deletion + lifecycle-GC bypass only: install-time wiring before any deletion state can exist.
    mutate_state_bypass(|state| {
        for message in args.openchat_bot_messages {
            openchat_bot::send_message(message.into(), Vec::new(), true, state);
        }
    });

    // MKTd02 Leaf-mode CVDR engine: initialise its 8 stable-memory slots
    // (base 100) on the SAME MemoryManager and publish the initial certified
    // commitment. Must run after all initial PII writes and with no state
    // borrow held (the engine calls back into the adapter via read_state).
    let module_hash = args.mktd_module_hash.unwrap_or([0u8; 32]);
    let adapter = crate::mktd::MKTdUserAdapter;
    crate::memory::with_memory_manager(|mm| {
        mktd02::init(&adapter, mm, crate::mktd::config(), module_hash);
    });

    info!(version = %args.wasm_version, "Initialization complete");
}
