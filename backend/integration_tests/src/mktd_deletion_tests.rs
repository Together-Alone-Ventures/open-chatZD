//! MKTd02 (ICP-Delete-Leaf) Leaf-mode three-phase deletion — P1 PocketIC suite.
//!
//! Exercises the four user-canister endpoints against a live PocketIC replica:
//!   - Phase A  `mktd_execute_deletion`        (owner-guarded update)
//!   - Phase B  `mktd_pending_certificate`     (query — reads `data_certificate`)
//!   - Phase C  `mktd_finalize_deletion`       (owner-guarded update)
//!   -          `mktd_pending_deletion_state`  (query — S8 recovery probe)
//!
//! Engine: mktd02-v0.4.1 (`921d710`). Error strings asserted below are the
//! engine's verbatim `Display` output, surfaced through the host wrappers'
//! `Error(String)` arms.

use crate::client::execute_msgpack_update_no_unwrap;
use crate::env::ENV;
use crate::stable_memory::get_stable_memory_map;
use crate::utils::tick_many;
use crate::{CanisterIds, TestEnv, User, client};
use candid::Principal;
use ic_stable_structures::memory_manager::MemoryId;
use pocket_ic::{PocketIc, RejectCode};
use std::collections::BTreeSet;
use std::ops::Deref;
use types::{Empty, HttpRequest};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn register(env: &mut PocketIc, canister_ids: &CanisterIds) -> User {
    client::register_user(env, canister_ids)
}

/// Phase A as the owner; asserts Success and a 32-byte receipt id.
fn phase_a(env: &mut PocketIc, user: &User) -> Vec<u8> {
    let response = client::user::mktd_execute_deletion(env, user.principal, user.canister(), &Empty {});
    match response {
        user_canister::mktd_execute_deletion::Response::Success(receipt_id) => {
            assert_eq!(receipt_id.len(), 32, "receipt_id must be 32 bytes, got {}", receipt_id.len());
            receipt_id
        }
        other => panic!("Phase A expected Success, got {other:?}"),
    }
}

fn pending_state(env: &PocketIc, user: &User) -> user_canister::mktd_pending_deletion_state::Response {
    client::user::mktd_pending_deletion_state(env, user.principal, user.canister(), &Empty {})
}

/// Phase B (query). Ticks first so the `certified_data` published by Phase A is
/// sealed into a certificate before the query reads `ic0.data_certificate()`.
/// HALTs (fails) if the certificate comes back empty — no work-around (per packet).
fn phase_b(env: &mut PocketIc, user: &User) -> user_canister::mktd_pending_certificate::PendingCertificate {
    tick_many(env, 2);
    let response = client::user::mktd_pending_certificate(env, user.principal, user.canister(), &Empty {});
    match response {
        user_canister::mktd_pending_certificate::Response::Success(pc) => {
            assert!(
                !pc.certificate.is_empty(),
                "HALT: Phase B certificate is empty — PocketIC did not certify data after Phase A"
            );
            pc
        }
        user_canister::mktd_pending_certificate::Response::NotPending => {
            panic!("Phase B expected Success, got NotPending")
        }
    }
}

fn finalize(
    env: &mut PocketIc,
    user: &User,
    receipt_id: Vec<u8>,
    certificate: Vec<u8>,
) -> user_canister::mktd_finalize_deletion::Response {
    client::user::mktd_finalize_deletion(
        env,
        user.principal,
        user.canister(),
        &user_canister::mktd_finalize_deletion::Args { receipt_id, certificate },
    )
}

/// The host-reported stable-memory slot ids (the keys of `stable_memory_sizes`
/// in `/metrics`). The host occupies {0,1,2,3}; the engine's reserved slots
/// (100..=107) must never appear here.
fn stable_memory_slot_ids(env: &PocketIc, user: &User) -> BTreeSet<u64> {
    let response = client::http_request(
        env,
        user.principal,
        user.canister(),
        &HttpRequest {
            method: "GET".to_string(),
            // `extract_route` operates on the path only (it does not strip a
            // scheme/host) — a full URL would fall through to 404.
            url: "/metrics".to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        },
    );
    assert_eq!(response.status_code, 200, "metrics http status");
    let json: serde_json::Value = serde_json::from_slice(&response.body).expect("metrics body is JSON");
    json["stable_memory_sizes"]
        .as_object()
        .expect("stable_memory_sizes object")
        .keys()
        .map(|k| k.parse::<u64>().expect("numeric stable-memory slot key"))
        .collect()
}

fn err_text(response: &user_canister::mktd_finalize_deletion::Response) -> &str {
    match response {
        user_canister::mktd_finalize_deletion::Response::Error(e) => e,
        other => panic!("expected Error, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Cases
// ---------------------------------------------------------------------------

#[test]
fn round_trip_a_simb_c() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let user = register(env, canister_ids);

    // Phase A
    let receipt_id = phase_a(env, &user);

    // Pending state after A
    let state = pending_state(env, &user);
    assert!(state.pending, "pending must be true after Phase A");
    assert!(state.tombstoned, "tombstoned must be true after Phase A");
    assert_eq!(state.receipt_id.as_deref(), Some(receipt_id.as_slice()));

    // Phase B (query): certificate non-empty, bound to the same receipt
    let pc = phase_b(env, &user);
    assert_eq!(pc.receipt_id, receipt_id, "Phase B receipt_id must match Phase A");

    // Phase C
    let response = finalize(env, &user, receipt_id.clone(), pc.certificate);
    assert!(
        matches!(response, user_canister::mktd_finalize_deletion::Response::Success),
        "Phase C expected Success, got {response:?}"
    );

    // Pending state after finalize
    let state = pending_state(env, &user);
    assert!(!state.pending, "pending must be false after finalize");

    let exported = client::user::mktd_get_receipt(
        env,
        user.principal,
        user.canister(),
        &user_canister::mktd_get_receipt::Args {
            receipt_id: receipt_id.clone(),
        },
    );
    let receipt = match exported {
        user_canister::mktd_get_receipt::Response::Success(receipt) => receipt,
        other => panic!("finalized receipt export expected Success, got {other:?}"),
    };
    assert_eq!(receipt.receipt_id.as_slice(), receipt_id.as_slice());
    assert_eq!(receipt.canister_id, user.canister());
    assert_eq!(receipt.record_id.len(), 32);
    assert!(receipt.bls_certificate.is_some());
    assert_eq!(receipt.trust_root_key_id, "local-dev");

    let receipt_json = serde_json::to_string(&receipt).expect("finalized receipt serializes");
    println!("MKTD_FINALIZED_RECEIPT_JSON={receipt_json}");
    println!("MKTD_RECEIPT_MODULE_HASH={}", hex::encode(receipt.module_hash));
    println!("MKTD_TRUST_ROOT_KEY_ID={}", receipt.trust_root_key_id);

    if let Ok(path) = std::env::var("MKTD_RECEIPT_EXPORT_PATH") {
        std::fs::write(path, receipt_json).expect("write finalized receipt export");
    }
}

#[test]
fn pending_receipt_is_not_exported_and_pin_is_destroyed() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let user = register(env, canister_ids);
    client::user::happy_path::set_pin_number(env, &user, None, Some("1000".to_string()));

    let before = client::user::happy_path::initial_state(env, &user);
    assert!(before.pin_number_settings.is_some(), "PIN must be enabled before Phase A");

    let receipt_id = phase_a(env, &user);

    let pending = client::user::mktd_get_receipt(
        env,
        user.principal,
        user.canister(),
        &user_canister::mktd_get_receipt::Args {
            receipt_id: receipt_id.clone(),
        },
    );
    assert!(matches!(pending, user_canister::mktd_get_receipt::Response::NotFinalized));

    let after = client::user::happy_path::initial_state(env, &user);
    assert!(
        after.pin_number_settings.is_none(),
        "PIN credential must be destroyed by Phase A"
    );

    let pc = phase_b(env, &user);
    let response = finalize(env, &user, receipt_id.clone(), pc.certificate);
    assert!(matches!(response, user_canister::mktd_finalize_deletion::Response::Success));
    assert!(matches!(
        client::user::mktd_get_receipt(
            env,
            user.principal,
            user.canister(),
            &user_canister::mktd_get_receipt::Args { receipt_id },
        ),
        user_canister::mktd_get_receipt::Response::Success(_)
    ));
}

#[test]
fn non_owner_rejected_a_and_c() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let user = register(env, canister_ids);
    let non_owner = Principal::anonymous();

    // Phase A as non-owner -> rejected by caller_is_owner. A guard rejection is
    // surfaced as RejectCode::CanisterReject (distinct from a trap, which is
    // CanisterError) — PocketIC does not propagate the guard's message string.
    let a = execute_msgpack_update_no_unwrap(env, non_owner, user.canister(), "mktd_execute_deletion_msgpack", &Empty {});
    let a_err = a.expect_err("non-owner Phase A must be rejected");
    assert_eq!(
        a_err.reject_code,
        RejectCode::CanisterReject,
        "Phase A: {}",
        a_err.reject_message
    );

    // Phase C as non-owner -> rejected by caller_is_owner (guard runs before body)
    let c = execute_msgpack_update_no_unwrap(
        env,
        non_owner,
        user.canister(),
        "mktd_finalize_deletion_msgpack",
        &user_canister::mktd_finalize_deletion::Args {
            receipt_id: vec![0u8; 32],
            certificate: Vec::new(),
        },
    );
    let c_err = c.expect_err("non-owner Phase C must be rejected");
    assert_eq!(
        c_err.reject_code,
        RejectCode::CanisterReject,
        "Phase C: {}",
        c_err.reject_message
    );

    // No Phase A ran -> still no pending receipt.
    let state = pending_state(env, &user);
    assert!(!state.pending, "no pending receipt expected after rejected calls");
    assert!(!state.tombstoned, "tombstone must not be set after rejected calls");
}

#[test]
fn post_tombstone_write_traps() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let user = register(env, canister_ids);

    phase_a(env, &user);

    // An owner PII writer must now trap via assert_not_pending_deletion (D8).
    let result = execute_msgpack_update_no_unwrap(
        env,
        user.principal,
        user.canister(),
        "set_bio_msgpack",
        &user_canister::set_bio::Args {
            text: "should be blocked".to_string(),
        },
    );
    let err = result.expect_err("set_bio after Phase A must trap (D8)");
    assert!(
        err.reject_message.contains("D8"),
        "expected D8 trap, got: {}",
        err.reject_message
    );
}

#[test]
fn phase_a_leaves_direct_chat_stable_memory_untouched() {
    const STABLE_MEMORY_MAP_MEMORY_ID: MemoryId = MemoryId::new(3);

    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let user1 = register(env, canister_ids);
    let user2 = register(env, canister_ids);

    let baseline = get_stable_memory_map(env, user1.canister(), STABLE_MEMORY_MAP_MEMORY_ID).len();
    client::user::happy_path::send_text_message(env, &user1, user2.user_id, "delete me", None);
    tick_many(env, 3);

    assert!(
        get_stable_memory_map(env, user1.canister(), STABLE_MEMORY_MAP_MEMORY_ID).len() > baseline,
        "direct-chat message must be present in stable memory before Phase A"
    );

    let before_phase_a = get_stable_memory_map(env, user1.canister(), STABLE_MEMORY_MAP_MEMORY_ID).len();
    phase_a(env, &user1);
    tick_many(env, 2);

    assert_eq!(
        get_stable_memory_map(env, user1.canister(), STABLE_MEMORY_MAP_MEMORY_ID).len(),
        before_phase_a,
        "message content is outside the CVDR boundary and Phase A must leave it untouched"
    );
}

#[test]
fn a2_receipt_id_mismatch() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let user = register(env, canister_ids);

    let receipt_id = phase_a(env, &user);
    let pc = phase_b(env, &user);

    // Phase C with a wrong (but well-formed, 32-byte) receipt_id.
    let wrong_id = vec![0x9au8; 32];
    assert_ne!(wrong_id, receipt_id);
    let response = finalize(env, &user, wrong_id, pc.certificate);
    assert!(
        err_text(&response).contains("receipt_id mismatch"),
        "expected engine mismatch text, got: {}",
        err_text(&response)
    );

    // Lock still held -> still pending.
    let state = pending_state(env, &user);
    assert!(state.pending, "lock must stay held after a mismatch");
    assert_eq!(state.receipt_id.as_deref(), Some(receipt_id.as_slice()));
}

#[test]
fn a2_missing_lock() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let user = register(env, canister_ids);

    // Phase C before any Phase A -> NoPendingReceipt.
    let response = finalize(env, &user, vec![0u8; 32], Vec::new());
    assert!(
        err_text(&response).contains("no receipt pending finalization"),
        "expected NoPendingReceipt text, got: {}",
        err_text(&response)
    );
}

#[test]
fn a2_natural_double_finalize() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let user = register(env, canister_ids);

    let receipt_id = phase_a(env, &user);
    let pc = phase_b(env, &user);

    // First finalize succeeds and releases the lock.
    let first = finalize(env, &user, receipt_id.clone(), pc.certificate.clone());
    assert!(
        matches!(first, user_canister::mktd_finalize_deletion::Response::Success),
        "first finalize expected Success, got {first:?}"
    );

    // Natural double-finalize: lock already released -> NoPendingReceipt
    // (NOT AlreadyFinalized — the host never re-acquires the lock).
    let second = finalize(env, &user, receipt_id, pc.certificate);
    assert!(
        err_text(&second).contains("no receipt pending finalization"),
        "expected NoPendingReceipt on natural double-finalize, got: {}",
        err_text(&second)
    );
}

#[test]
fn record_id_observable() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();

    // record_id is not exposed; assert the downstream property instead: two
    // canisters with distinct principals yield distinct receipt ids.
    let user1 = register(env, canister_ids);
    let user2 = register(env, canister_ids);
    assert_ne!(user1.canister(), user2.canister(), "distinct subject principals");

    let rid1 = phase_a(env, &user1);
    let rid2 = phase_a(env, &user2);
    assert_ne!(rid1, rid2, "distinct subjects must yield distinct receipt ids");
}

#[test]
fn base_memory_id_no_collision() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();

    // Engine init runs on canister creation; a functioning, registered canister
    // implies init did not trap.
    let user = register(env, canister_ids);

    let before = stable_memory_slot_ids(env, &user);
    assert_eq!(
        before,
        BTreeSet::from([0, 1, 2, 3]),
        "host stable-memory shape must be exactly {{0,1,2,3}}, got {before:?}"
    );

    // Exercise the engine's reserved slots (100..=107) end-to-end.
    let receipt_id = phase_a(env, &user);
    let pc = phase_b(env, &user);
    let response = finalize(env, &user, receipt_id, pc.certificate);
    assert!(
        matches!(response, user_canister::mktd_finalize_deletion::Response::Success),
        "deletion flow must succeed, got {response:?}"
    );

    // Engine slots never bleed into the host's reported {0,3} band; shape intact.
    let after = stable_memory_slot_ids(env, &user);
    assert_eq!(after, before, "host stable-memory shape must be unchanged after the flow");
}

#[test]
fn pending_state_query_lifecycle() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let user = register(env, canister_ids);

    // Before Phase A
    let state = pending_state(env, &user);
    assert!(!state.pending, "pending must be false before Phase A");
    assert!(state.receipt_id.is_none());

    // After Phase A
    let receipt_id = phase_a(env, &user);
    let state = pending_state(env, &user);
    assert!(state.pending, "pending must be true after Phase A");
    assert_eq!(state.receipt_id.as_deref(), Some(receipt_id.as_slice()));

    // After finalize
    let pc = phase_b(env, &user);
    let response = finalize(env, &user, receipt_id, pc.certificate);
    assert!(matches!(response, user_canister::mktd_finalize_deletion::Response::Success));
    let state = pending_state(env, &user);
    assert!(!state.pending, "pending must be false after finalize");
    assert!(state.receipt_id.is_none());
}

#[test]
fn d8_failure_resumable() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let user = register(env, canister_ids);

    let receipt_id = phase_a(env, &user);
    let pc = phase_b(env, &user);

    // Induce a Phase C failure with a wrong receipt id.
    let bad = finalize(env, &user, vec![0x9au8; 32], pc.certificate.clone());
    assert!(
        err_text(&bad).contains("receipt_id mismatch"),
        "expected mismatch, got: {}",
        err_text(&bad)
    );

    // Canister stays pending and is not uninstalled (no pipeline triggered).
    assert!(
        env.canister_exists(user.canister()),
        "canister must still exist after a failed Phase C"
    );
    let state = pending_state(env, &user);
    assert!(state.pending, "must remain pending after a failed Phase C");

    // The D8 lock is still in force.
    let blocked = execute_msgpack_update_no_unwrap(
        env,
        user.principal,
        user.canister(),
        "set_bio_msgpack",
        &user_canister::set_bio::Args {
            text: "still blocked".to_string(),
        },
    );
    assert!(
        blocked.expect_err("set_bio must still trap").reject_message.contains("D8"),
        "D8 lock must still hold after a failed Phase C"
    );

    // A subsequent correct Phase C still finalizes.
    let good = finalize(env, &user, receipt_id, pc.certificate);
    assert!(
        matches!(good, user_canister::mktd_finalize_deletion::Response::Success),
        "correct Phase C must finalize, got {good:?}"
    );
    let state = pending_state(env, &user);
    assert!(!state.pending, "pending must clear after the correct Phase C");
}

#[test]
fn double_phase_a_rejected() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let user = register(env, canister_ids);

    let receipt_id = phase_a(env, &user);

    // A second Phase A is engine-rejected (already tombstoned).
    let second = client::user::mktd_execute_deletion(env, user.principal, user.canister(), &Empty {});
    match second {
        user_canister::mktd_execute_deletion::Response::Error(e) => {
            assert!(e.contains("already tombstoned"), "expected AlreadyTombstoned, got: {e}");
        }
        other => panic!("second Phase A expected Error, got {other:?}"),
    }

    // State unchanged: still exactly one pending receipt with the original id.
    let state = pending_state(env, &user);
    assert!(state.pending, "still pending after rejected second Phase A");
    assert!(state.tombstoned);
    assert_eq!(state.receipt_id.as_deref(), Some(receipt_id.as_slice()));
}

/// P1d re-verify: `receipt.module_hash` must equal the IC's on-chain
/// `module_hash` (the SHA-256 of the gzip bytes as submitted to install_code),
/// which is what CVDR-Verify V3 reads. Asserted against a LIVE user canister in
/// the alive-but-tombstoned window, before any uninstall.
#[test]
fn receipt_module_hash_is_the_on_chain_upload_hash() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let user = register(env, canister_ids);

    // Finalize a receipt on the live canister.
    let receipt_id = phase_a(env, &user);
    let pc = phase_b(env, &user);
    let response = finalize(env, &user, receipt_id.clone(), pc.certificate);
    assert!(
        matches!(response, user_canister::mktd_finalize_deletion::Response::Success),
        "Phase C expected Success, got {response:?}"
    );

    let exported = client::user::mktd_get_receipt(
        env,
        user.principal,
        user.canister(),
        &user_canister::mktd_get_receipt::Args { receipt_id },
    );
    let receipt = match exported {
        user_canister::mktd_get_receipt::Response::Success(receipt) => receipt,
        other => panic!("finalized receipt export expected Success, got {other:?}"),
    };
    let receipt_module_hash = receipt.module_hash;

    // PRIMARY: on-chain module_hash (read via the controller, local_user_index).
    let on_chain = env
        .canister_status(user.canister(), Some(user.local_user_index))
        .expect("canister_status")
        .module_hash
        .expect("running canister must report a module_hash");
    assert_eq!(
        on_chain.as_slice(),
        receipt_module_hash.as_slice(),
        "PRIMARY: receipt.module_hash must equal the on-chain module_hash"
    );

    // CROSS-CHECK: equals sha256 of the gzip wasm exactly as installed.
    let gzip = std::fs::read(crate::utils::local_bin().join("user.wasm.gz")).expect("read user.wasm.gz");
    let upload_hash = sha256::sha256(&gzip);
    assert_eq!(
        upload_hash, receipt_module_hash,
        "CROSS-CHECK: receipt.module_hash must equal sha256(user.wasm.gz)"
    );

    let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    println!("RECEIPT_MODULE_HASH = {}", hex(&receipt_module_hash));
    println!("ONCHAIN_MODULE_HASH = {}", hex(&on_chain));
    println!("SHA256_USER_WASM_GZ = {}", hex(&upload_hash));
}

/// CVDR receipt download path: the `/mktd_receipt?id=<hex>` http_request route
/// must serve bytes byte-identical to the canonical test-hook export
/// (`serde_json::to_string(&DeletionReceipt)`), so CVDR-Verify accepts the
/// downloaded file unchanged. Writes both artifacts (when the env vars are set)
/// for independent sha256 + CVDR-Verify re-derivation.
#[test]
fn receipt_download_path_matches_test_hook_export() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let user = register(env, canister_ids);

    // Finalize a receipt on the live canister.
    let receipt_id = phase_a(env, &user);
    let pc = phase_b(env, &user);
    let response = finalize(env, &user, receipt_id.clone(), pc.certificate);
    assert!(
        matches!(response, user_canister::mktd_finalize_deletion::Response::Success),
        "Phase C expected Success, got {response:?}"
    );

    // Reference bytes: engine canonical serde_json, exactly as the
    // MKTD_RECEIPT_EXPORT_PATH test hook emits (mktd_get_receipt -> serde_json).
    let exported = client::user::mktd_get_receipt(
        env,
        user.principal,
        user.canister(),
        &user_canister::mktd_get_receipt::Args { receipt_id: receipt_id.clone() },
    );
    let receipt = match exported {
        user_canister::mktd_get_receipt::Response::Success(receipt) => receipt,
        other => panic!("finalized receipt export expected Success, got {other:?}"),
    };
    let hook_bytes = serde_json::to_string(&receipt).expect("serialize").into_bytes();

    // Download-path bytes: the new http_request route.
    let receipt_id_hex = hex::encode(&receipt_id);
    let download = client::http_request(
        env,
        user.principal,
        user.canister(),
        &HttpRequest {
            method: "GET".to_string(),
            url: format!("/mktd_receipt?id={receipt_id_hex}"),
            headers: Vec::new(),
            body: Vec::new(),
        },
    );
    assert_eq!(download.status_code, 200, "download must be 200");
    assert!(
        download
            .headers
            .iter()
            .any(|h| h.0.eq_ignore_ascii_case("content-disposition") && h.1.contains(".json")),
        "download must carry an attachment .json filename"
    );
    let download_bytes = download.body;

    // ACCEPTANCE: byte-identical (the path must not reshape anything).
    assert_eq!(
        sha256::sha256(&download_bytes),
        sha256::sha256(&hook_bytes),
        "download-path JSON must be byte-identical to the test-hook export"
    );

    println!("MKTD_DOWNLOAD_SHA256={}", hex::encode(sha256::sha256(&download_bytes)));
    println!("MKTD_HOOK_SHA256={}", hex::encode(sha256::sha256(&hook_bytes)));

    if let Ok(path) = std::env::var("MKTD_RECEIPT_EXPORT_PATH") {
        std::fs::write(path, &hook_bytes).expect("write hook export");
    }
    if let Ok(path) = std::env::var("MKTD_RECEIPT_DOWNLOAD_PATH") {
        std::fs::write(path, &download_bytes).expect("write download export");
    }
}

/// Negative-route contract for `/mktd_receipt`: every malformed or non-matching
/// request must return 404 with no panic path. Covers bad id length, non-hex id,
/// unknown (well-formed but absent) id, wrong path, and — the load-bearing case —
/// a non-GET method against a *valid, existing* receipt id, which must still 404
/// (proving the GET-only method gate, not just a bad id, is what rejects it).
#[test]
fn receipt_route_negatives_all_return_404() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let user = register(env, canister_ids);

    // Finalize a real receipt so the wrong-method case uses a valid, existing id.
    let receipt_id = phase_a(env, &user);
    let pc = phase_b(env, &user);
    let response = finalize(env, &user, receipt_id.clone(), pc.certificate);
    assert!(
        matches!(response, user_canister::mktd_finalize_deletion::Response::Success),
        "Phase C expected Success, got {response:?}"
    );
    let receipt_id_hex = hex::encode(&receipt_id);

    let req = |method: &str, url: String| {
        client::http_request(
            env,
            user.principal,
            user.canister(),
            &HttpRequest { method: method.to_string(), url, headers: Vec::new(), body: Vec::new() },
        )
    };

    // Sanity: the valid id over GET is downloadable (200), so the 404s below are
    // about the negative input, not a broken happy path.
    assert_eq!(
        req("GET", format!("/mktd_receipt?id={receipt_id_hex}")).status_code,
        200,
        "valid id over GET must still download"
    );

    // wrong_method: valid, existing id but a non-GET method -> 404 (method gate).
    assert_eq!(
        req("POST", format!("/mktd_receipt?id={receipt_id_hex}")).status_code,
        404,
        "non-GET method against a valid id must be rejected as 404"
    );

    // bad length: too-short hex -> 404.
    assert_eq!(req("GET", "/mktd_receipt?id=deadbeef".to_string()).status_code, 404, "short id must 404");

    // non-hex: correct length (64 chars) but not hex -> 404.
    assert_eq!(
        req("GET", format!("/mktd_receipt?id={}", "z".repeat(64))).status_code,
        404,
        "non-hex id must 404"
    );

    // unknown: well-formed 64-hex id that was never finalized -> 404.
    assert_eq!(
        req("GET", format!("/mktd_receipt?id={}", "0".repeat(64))).status_code,
        404,
        "unknown id must 404"
    );

    // wrong path: matches no route -> 404.
    assert_eq!(
        req("GET", format!("/mktd_recipe?id={receipt_id_hex}")).status_code,
        404,
        "wrong path must 404"
    );
}
