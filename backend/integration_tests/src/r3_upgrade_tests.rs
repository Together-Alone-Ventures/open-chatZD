//! R-3 (suite-v5 retrofit): removal of the legacy user-canister MKTd02 producer and the receipts
//! canister changed these lifecycle argument / stored-state shapes:
//!
//! - `user`: `init::Args` and `post_upgrade::Args` lose `mktd_module_hash`; `Data` loses
//!   `mktd_finalized_receipt_id`; the engine's stable MemoryIds 100..=107 stay allocated, unread.
//! - `openchat_installer`, `user_index`, `local_user_index`: `init::Args` and `Data` lose
//!   `receipts_canister_id`; `local_user_index` `Data` also loses `user_canister_module_hash`.
//!
//! The PocketIC test upgrades a deployment installed from the Docker-built `c744de1` wasms to the
//! current wasms through the production upgrade paths (each baseline wasm is SHA-256-checked against
//! docs/dev/v5/BASELINE_c744de1.md §3.2 when loaded — `wasms::baseline_c744de1`). The wire tests pin the staggered-rollout
//! case where a not-yet-upgraded parent still sends the retired field to an upgraded child.

use crate::client::{register_user, register_user_and_include_auth};
use crate::cvdr_tests::{cvdr_metrics, delete_and_reach_awaiting, metrics_json, wait_for_version};
use crate::utils::tick_many;
use crate::{TestEnv, client, wasms};
use candid::types::Label;
use candid::types::value::{IDLArgs, IDLField, IDLValue};
use candid::{CandidType, Principal};
use openchat_installer_canister::CanisterType;
use serde::Serialize;
use serde::de::DeserializeOwned;
use sha256::sha256;
use types::{BuildVersion, CanisterWasm};

fn bumped(wasm: &CanisterWasm, version: BuildVersion) -> CanisterWasm {
    let mut wasm = wasm.clone();
    wasm.version = version;
    wasm
}

#[test]
#[ignore = "needs wasms/baseline_c744de1/{openchat_installer,user_index,local_user_index,user}.wasm.gz \
            (Docker-built at c744de1; docs/dev/v5/BASELINE_c744de1.md §3.2). Run with --ignored."]
fn r3_upgrade_from_c744de1_keeps_state_and_function() {
    let TestEnv {
        mut env,
        canister_ids,
        controller,
    } = crate::setup::setup_env_from_baseline_c744de1();
    let env = &mut env;
    let canister_ids = &canister_ids;

    // State created under the OLD wasms (engine initialised in every user canister, slots 100..=107).
    let (user1, user1_auth) = register_user_and_include_auth(env, canister_ids);
    let user2 = register_user(env, canister_ids);
    client::user::happy_path::send_text_message(env, &user1, user2.user_id, "before the R-3 upgrade", None);
    let events_before = format!(
        "{:?}",
        client::user::happy_path::events(env, &user1, user2.user_id, 0.into(), true, 10, 10)
    );
    let stable_before = metrics_json(env, user1.canister())["stable_memory_used"]
        .as_u64()
        .expect("stable_memory_used");

    let version = BuildVersion::new(0, 0, 1);

    // 1. openchat_installer — controller-driven upgrade; stored `Data` still carries `receipts_canister_id`.
    env.upgrade_canister(
        canister_ids.openchat_installer,
        wasms::OPENCHAT_INSTALLER.module.clone().into(),
        candid::encode_one(openchat_installer_canister::post_upgrade::Args { wasm_version: version }).unwrap(),
        Some(controller),
    )
    .expect("openchat_installer upgrade from c744de1");

    // 2. user_index — through the (upgraded) installer's own upgrade endpoint.
    let user_index_wasm = bumped(&wasms::USER_INDEX, version);
    client::openchat_installer::happy_path::upload_wasm_in_chunks(
        env,
        controller,
        canister_ids.openchat_installer,
        &user_index_wasm.module,
        CanisterType::UserIndex,
    );
    let response = client::openchat_installer::upgrade_canister(
        env,
        controller,
        canister_ids.openchat_installer,
        &openchat_installer_canister::upgrade_canister::Args {
            canister_type: CanisterType::UserIndex,
            version,
            wasm_hash: sha256(&user_index_wasm.module),
            filter: None,
        },
    );
    assert!(
        matches!(response, types::UpgradeChunkedCanisterWasmResponse::Success),
        "user_index upgrade from c744de1: {response:?}"
    );
    wait_for_version(env, canister_ids.user_index, version);

    // 3. local_user_index — through user_index (new user_index builds the new post_upgrade args).
    client::user_index::happy_path::upgrade_local_user_index_canister_wasm(
        env,
        controller,
        canister_ids.user_index,
        bumped(&wasms::LOCAL_USER_INDEX, version),
    );
    for subnet in &canister_ids.subnets {
        wait_for_version(env, subnet.local_user_index, version);
    }

    // 4. user canisters — through user_index -> local_user_index upgrade job (new post_upgrade args,
    //    no `mktd_module_hash`; no engine reconnect).
    client::user_index::happy_path::upgrade_user_canister_wasm(
        env,
        controller,
        canister_ids.user_index,
        bumped(&wasms::USER, version),
    );
    wait_for_version(env, user1.canister(), version);
    wait_for_version(env, user2.canister(), version);

    // Stored state survived field removal, and the chat history is byte-for-byte what it was.
    let events_after = format!(
        "{:?}",
        client::user::happy_path::events(env, &user1, user2.user_id, 0.into(), true, 10, 10)
    );
    assert_eq!(
        events_after, events_before,
        "direct chat history unchanged across the R-3 upgrade"
    );

    // Reserved MemoryIds 100..=107: the retired engine's buckets are left in place, never reclaimed.
    let stable_after = metrics_json(env, user1.canister())["stable_memory_used"]
        .as_u64()
        .expect("stable_memory_used");
    assert!(
        stable_after >= stable_before,
        "stable memory must not shrink: retired engine slots stay allocated ({stable_before} -> {stable_after})"
    );

    // Upgraded canisters still work: writes, a fresh registration (new init args end to end), and
    // the CVDR-on-Index deletion leg up to AwaitingCertificate.
    client::user::happy_path::send_text_message(env, &user2, user1.user_id, "after the R-3 upgrade", None);
    let user3 = register_user(env, canister_ids);
    client::user::happy_path::send_text_message(env, &user3, user1.user_id, "from a post-upgrade user", None);
    tick_many(env, 3);

    let lui = user1.local_user_index;
    let (drafts_before, _, _) = cvdr_metrics(env, lui);
    delete_and_reach_awaiting(env, canister_ids, &user1, &user1_auth);
    let (drafts, _, awaiting) = cvdr_metrics(env, lui);
    assert_eq!(drafts, drafts_before + 1, "deletion captured a draft on the upgraded index");
    assert!(awaiting, "deletion reaches AwaitingCertificate on the upgraded index");
}

// ---------------------------------------------------------------------------------------------
// Wire compatibility: an old parent still sends the retired field to an already-upgraded child.
// ---------------------------------------------------------------------------------------------

/// Re-encode `args` as Candid with one extra, unknown record field appended.
fn candid_with_extra_opt_field<T: CandidType>(args: &T, name: &str, value: IDLValue) -> Vec<u8> {
    let bytes = candid::encode_one(args).unwrap();
    let mut decoded = IDLArgs::from_bytes(&bytes).unwrap();
    let IDLValue::Record(fields) = &mut decoded.args[0] else {
        panic!("init args must be a Candid record");
    };
    fields.push(IDLField {
        id: Label::Named(name.to_string()),
        val: IDLValue::Opt(Box::new(value)),
    });
    fields.sort_unstable_by_key(|f| f.id.get_id());
    decoded.to_bytes().unwrap()
}

fn assert_candid_ignores<T: CandidType + DeserializeOwned + std::fmt::Debug>(args: &T, name: &str, value: IDLValue) {
    let bytes = candid_with_extra_opt_field(args, name, value);
    let decoded: T = candid::decode_one(&bytes).unwrap_or_else(|e| panic!("retired field `{name}` must be ignored: {e}"));
    assert_eq!(format!("{decoded:?}"), format!("{args:?}"));
}

#[test]
#[expect(deprecated)]
fn user_init_args_ignore_retired_mktd_module_hash() {
    let args = user_canister::init::Args {
        owner: Principal::anonymous(),
        group_index_canister_id: Principal::anonymous(),
        user_index_canister_id: Principal::anonymous(),
        local_user_index_canister_id: Principal::anonymous(),
        identity_canister_id: Principal::anonymous(),
        notifications_canister_id: Principal::anonymous(),
        bot_api_gateway_canister_id: Principal::anonymous(),
        proposals_bot_canister_id: Principal::anonymous(),
        escrow_canister_id: Principal::anonymous(),
        wasm_version: BuildVersion::new(1, 2, 3),
        username: "r3".to_string(),
        openchat_bot_messages: Vec::new(),
        video_call_operators: vec![Principal::anonymous()],
        referred_by: None,
        rng_seed: [7; 32],
        test_mode: true,
    };
    let hash = IDLValue::Vec(vec![IDLValue::Nat8(9); 32]);
    assert_candid_ignores(&args, "mktd_module_hash", hash);
}

#[test]
fn local_user_index_init_args_ignore_retired_receipts_canister_id() {
    let args = local_user_index_canister::init::Args {
        wasm_version: BuildVersion::new(1, 2, 3),
        expected_index_module_hash: Some([5; 32]),
        user_index_canister_id: Principal::anonymous(),
        group_index_canister_id: Principal::anonymous(),
        notifications_index_canister_id: Principal::anonymous(),
        identity_canister_id: Principal::anonymous(),
        proposals_bot_canister_id: Principal::anonymous(),
        cycles_dispenser_canister_id: Principal::anonymous(),
        escrow_canister_id: Principal::anonymous(),
        event_relay_canister_id: Principal::anonymous(),
        online_users_canister_id: Principal::anonymous(),
        internet_identity_canister_id: Principal::anonymous(),
        website_canister_id: Principal::anonymous(),
        video_call_operators: vec![Principal::anonymous()],
        oc_secret_key_der: vec![1, 2, 3],
        rng_seed: [7; 32],
        ic_root_key: vec![4, 5, 6],
        openai_api_key: None,
        test_mode: true,
    };
    assert_candid_ignores(
        &args,
        "receipts_canister_id",
        IDLValue::Principal(Principal::management_canister()),
    );
}

#[test]
fn user_post_upgrade_args_ignore_retired_mktd_module_hash() {
    // The pre-R-3 shape a not-yet-upgraded local_user_index serialises (msgpack, struct-as-map).
    #[derive(Serialize)]
    struct PreR3Args {
        wasm_version: BuildVersion,
        mktd_module_hash: Option<[u8; 32]>,
    }
    let bytes = msgpack::serialize_then_unwrap(PreR3Args {
        wasm_version: BuildVersion::new(1, 2, 3),
        mktd_module_hash: Some([9; 32]),
    });
    let args: user_canister::post_upgrade::Args = msgpack::deserialize_then_unwrap(&bytes);
    assert_eq!(args.wasm_version, BuildVersion::new(1, 2, 3));
}
