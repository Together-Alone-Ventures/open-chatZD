//! P2 — dedicated receipts canister: store + public read-by-id PocketIC suite.
//!
//! Covers the receipts-canister-local portion of the G-specified matrix (§8):
//! store success, idempotent / dup-different reject, bad id, unauthorized store,
//! and the public-read privacy surface (§10) — strict id, GET-only, 404s,
//! byte-identity of served bytes vs the user-canister finalized export, and a
//! sentinel-PII grep on the served bytes with a `record_id`-is-a-hash check.
//!
//! The export bytes are produced exactly as the P1f test hook does
//! (`serde_json::to_vec(&receipt)` over a finalized `mktd_get_receipt`), so the
//! bytes stored/served here are the same canonical artifact the pre-uninstall
//! export forwards.

use crate::client::execute_msgpack_update_no_unwrap;
use crate::env::ENV;
use crate::utils::tick_many;
use crate::{CanisterIds, TestEnv, User, client, wasms};
use candid::Principal;
use pocket_ic::PocketIc;
use std::ops::Deref;
use std::time::Duration;
use types::{BuildVersion, CanisterId, CanisterWasm, Empty, HttpRequest};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Drive Phase A→B→C on a fresh user and return its finalized receipt: the raw
/// 32-byte id and the canonical JSON bytes (the exact export payload).
fn finalized_receipt(env: &mut PocketIc, canister_ids: &CanisterIds) -> (User, Vec<u8>, Vec<u8>) {
    let user = client::register_user(env, canister_ids);
    let (receipt_id, canonical) = finalize_user(env, &user);
    (user, receipt_id, canonical)
}

/// Drive Phase A→B→C on an already-registered user; return (receipt_id, canonical
/// JSON bytes).
fn finalize_user(env: &mut PocketIc, user: &User) -> (Vec<u8>, Vec<u8>) {
    let receipt_id = match client::user::mktd_execute_deletion(env, user.principal, user.canister(), &Empty {}) {
        user_canister::mktd_execute_deletion::Response::Success(id) => id,
        other => panic!("Phase A expected Success, got {other:?}"),
    };

    tick_many(env, 2);
    let pc = match client::user::mktd_pending_certificate(env, user.principal, user.canister(), &Empty {}) {
        user_canister::mktd_pending_certificate::Response::Success(pc) => pc,
        other => panic!("Phase B expected Success, got {other:?}"),
    };

    let response = client::user::mktd_finalize_deletion(
        env,
        user.principal,
        user.canister(),
        &user_canister::mktd_finalize_deletion::Args {
            receipt_id: receipt_id.clone(),
            certificate: pc.certificate,
        },
    );
    assert!(
        matches!(response, user_canister::mktd_finalize_deletion::Response::Success),
        "Phase C expected Success, got {response:?}"
    );

    // Canonical bytes, exactly as the test hook / pre-uninstall export produce.
    let receipt = match client::user::mktd_get_receipt(
        env,
        user.principal,
        user.canister(),
        &user_canister::mktd_get_receipt::Args { receipt_id: receipt_id.clone() },
    ) {
        user_canister::mktd_get_receipt::Response::Success(r) => r,
        other => panic!("finalized receipt export expected Success, got {other:?}"),
    };
    let canonical = serde_json::to_vec(&receipt).expect("serialize");

    (receipt_id, canonical)
}

fn store(
    env: &mut PocketIc,
    sender: Principal,
    canister_ids: &CanisterIds,
    receipt_id: Vec<u8>,
    receipt_json: Vec<u8>,
) -> receipts_canister::store::Response {
    client::receipts::store(
        env,
        sender,
        canister_ids.receipts,
        &receipts_canister::store::Args { receipt_id, receipt_json },
    )
}

fn http_get(env: &PocketIc, canister_ids: &CanisterIds, url: String) -> types::HttpResponse {
    client::http_request(
        env,
        Principal::anonymous(),
        canister_ids.receipts,
        &HttpRequest { method: "GET".to_string(), url, headers: Vec::new(), body: Vec::new() },
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// store success → public read-by-id returns the byte-identical artifact with the
/// hardened headers (nosniff / no-store / attachment).
#[test]
fn store_then_read_is_byte_identical() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, controller, .. } = wrapper.env();

    let (_user, receipt_id, canonical) = finalized_receipt(env, canister_ids);
    let receipt_id_hex = hex::encode(&receipt_id);

    assert_eq!(
        store(env, *controller, canister_ids, receipt_id.clone(), canonical.clone()),
        receipts_canister::store::Response::Success,
        "authorized store of a finalized receipt must succeed"
    );

    let resp = http_get(env, canister_ids, format!("/receipt?id={receipt_id_hex}"));
    assert_eq!(resp.status_code, 200, "read-by-id must be 200");
    assert_eq!(resp.body, canonical, "served bytes must be byte-identical to the export");

    let header = |k: &str| {
        resp.headers
            .iter()
            .find(|h| h.0.eq_ignore_ascii_case(k))
            .map(|h| h.1.to_ascii_lowercase())
    };
    assert_eq!(header("x-content-type-options").as_deref(), Some("nosniff"), "nosniff header");
    assert_eq!(header("cache-control").as_deref(), Some("no-store"), "no-store header");
    assert!(
        header("content-disposition").is_some_and(|v| v.contains(".json")),
        "attachment .json filename"
    );
}

/// Idempotency (§2): same id + identical bytes → AlreadyExists; same id +
/// different bytes → Conflict, and the originally-stored bytes are unchanged.
#[test]
fn store_is_idempotent_and_rejects_divergent_bytes() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, controller, .. } = wrapper.env();

    let (user, receipt_id, canonical) = finalized_receipt(env, canister_ids);
    let receipt_id_hex = hex::encode(&receipt_id);

    assert_eq!(
        store(env, *controller, canister_ids, receipt_id.clone(), canonical.clone()),
        receipts_canister::store::Response::Success
    );

    // Same id + identical bytes → idempotent OK.
    assert_eq!(
        store(env, *controller, canister_ids, receipt_id.clone(), canonical.clone()),
        receipts_canister::store::Response::AlreadyExists,
        "re-store of identical bytes must be idempotent"
    );

    // Same id + DIFFERENT bytes (a valid, same-receipt_id re-serialization with
    // different whitespace) → hard reject.
    let receipt = match client::user::mktd_get_receipt(
        env,
        user.principal,
        user.canister(),
        &user_canister::mktd_get_receipt::Args { receipt_id: receipt_id.clone() },
    ) {
        user_canister::mktd_get_receipt::Response::Success(r) => r,
        other => panic!("expected Success, got {other:?}"),
    };
    let pretty = serde_json::to_vec_pretty(&receipt).expect("serialize");
    assert_ne!(pretty, canonical, "pretty bytes must differ from canonical");
    assert_eq!(
        store(env, *controller, canister_ids, receipt_id.clone(), pretty),
        receipts_canister::store::Response::Conflict,
        "same id + different bytes must be a hard Conflict reject"
    );

    // The served bytes remain the originally-stored canonical artifact.
    let resp = http_get(env, canister_ids, format!("/receipt?id={receipt_id_hex}"));
    assert_eq!(resp.status_code, 200);
    assert_eq!(resp.body, canonical, "conflict must not overwrite the stored bytes");
}

/// Bad id (length / non-32-bytes) and non-finalized payloads are cleanly rejected
/// (Response variant, not a panic).
#[test]
fn store_rejects_bad_id_and_unfinalized_payload() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, controller, .. } = wrapper.env();

    let (_user, receipt_id, canonical) = finalized_receipt(env, canister_ids);

    // 31-byte id → InvalidReceiptId.
    assert_eq!(
        store(env, *controller, canister_ids, vec![0u8; 31], canonical.clone()),
        receipts_canister::store::Response::InvalidReceiptId,
        "short id must be rejected"
    );

    // Valid 32-byte id, but the payload is not a finalized receipt → NotFinalized.
    assert_eq!(
        store(env, *controller, canister_ids, receipt_id, b"{\"not\":\"a receipt\"}".to_vec()),
        receipts_canister::store::Response::NotFinalized,
        "non-finalized / unparseable payload must be rejected"
    );
}

/// WRITE-gating (§2): a caller outside the export authority cannot store — the
/// call is rejected at the message boundary (Err), not merely a Response variant.
#[test]
fn unauthorized_store_is_rejected() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();

    let (_user, receipt_id, canonical) = finalized_receipt(env, canister_ids);

    let outsider = Principal::from_slice(&[9, 9, 9, 9, 9, 9, 9, 1]);
    let result = execute_msgpack_update_no_unwrap(
        env,
        outsider,
        canister_ids.receipts,
        "store_msgpack",
        &receipts_canister::store::Args { receipt_id, receipt_json: canonical },
    );
    assert!(result.is_err(), "unauthorized store must be rejected, got {result:?}");
}

/// The export-authority management endpoints are controller-gated (G): a
/// non-controller cannot add or remove an authorized principal — both are
/// rejected at the message boundary, not a public auth-management surface.
#[test]
fn auth_management_is_controller_gated() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();

    let outsider = Principal::from_slice(&[7, 7, 7, 7, 7, 7, 7, 1]);
    let target = Principal::from_slice(&[1, 2, 3, 4]);

    let add = execute_msgpack_update_no_unwrap(
        env,
        outsider,
        canister_ids.receipts,
        "add_authorized_principal_msgpack",
        &receipts_canister::add_authorized_principal::Args { principal: target },
    );
    assert!(add.is_err(), "add_authorized_principal must be controller-gated, got {add:?}");

    let remove = execute_msgpack_update_no_unwrap(
        env,
        outsider,
        canister_ids.receipts,
        "remove_authorized_principal_msgpack",
        &receipts_canister::remove_authorized_principal::Args { principal: target },
    );
    assert!(remove.is_err(), "remove_authorized_principal must be controller-gated, got {remove:?}");
}

/// Public-read negative routes all return 404, no panic: bad length, non-hex,
/// unknown id, wrong path, wrong method. The valid id over GET is 200.
#[test]
fn read_route_negatives_all_return_404() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, controller, .. } = wrapper.env();

    let (_user, receipt_id, canonical) = finalized_receipt(env, canister_ids);
    let receipt_id_hex = hex::encode(&receipt_id);
    assert_eq!(
        store(env, *controller, canister_ids, receipt_id, canonical),
        receipts_canister::store::Response::Success
    );

    // Sanity: valid id over GET is downloadable.
    assert_eq!(
        http_get(env, canister_ids, format!("/receipt?id={receipt_id_hex}")).status_code,
        200,
        "valid id over GET must be 200"
    );

    // wrong_method: valid id but POST → 404 (method gate).
    let post = client::http_request(
        env,
        Principal::anonymous(),
        canister_ids.receipts,
        &HttpRequest {
            method: "POST".to_string(),
            url: format!("/receipt?id={receipt_id_hex}"),
            headers: Vec::new(),
            body: Vec::new(),
        },
    );
    assert_eq!(post.status_code, 404, "non-GET method must 404");

    assert_eq!(http_get(env, canister_ids, "/receipt?id=deadbeef".to_string()).status_code, 404, "short id");
    assert_eq!(
        http_get(env, canister_ids, format!("/receipt?id={}", "z".repeat(64))).status_code,
        404,
        "non-hex id"
    );
    assert_eq!(
        http_get(env, canister_ids, format!("/receipt?id={}", "0".repeat(64))).status_code,
        404,
        "unknown id"
    );
    assert_eq!(
        http_get(env, canister_ids, format!("/recipe?id={receipt_id_hex}")).status_code,
        404,
        "wrong path"
    );
}

/// Privacy surface (§10): the served bytes carry no plaintext profile PII, and
/// `record_id` is the 32-byte domain-tagged hash — NOT the raw subject principal.
#[test]
fn served_bytes_have_no_plaintext_pii() {
    // Distinctive, collision-proof PII sentinels (G / #3). The handle and display
    // name are registered as REAL profile values on the subject before finalizing,
    // so if the engine receipt ever embedded a subject's handle/display name the
    // grep below would catch it. The generated 5-char username can collide with hex
    // in the record_id; these distinctive strings cannot.
    const SENTINEL_HANDLE: &str = "ZZSENTINEL_HANDLE";
    const SENTINEL_NAME: &str = "ZZSENTINEL_NAME";
    const SENTINEL_EMAIL: &str = "sentinel@zz.invalid";

    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, controller, .. } = wrapper.env();

    // Register the subject and set the distinctive handle + display-name sentinels
    // as real profile PII, then let them propagate to the user canister BEFORE the
    // receipt is finalized.
    let user = client::register_user(env, canister_ids);
    client::user_index::happy_path::set_username(env, user.principal, canister_ids.user_index, SENTINEL_HANDLE.to_string());
    client::user_index::happy_path::set_display_name(
        env,
        user.principal,
        canister_ids.user_index,
        Some(SENTINEL_NAME.to_string()),
    );
    tick_many(env, 5);

    let (receipt_id, canonical) = finalize_user(env, &user);
    let receipt_id_hex = hex::encode(&receipt_id);
    assert_eq!(
        store(env, *controller, canister_ids, receipt_id, canonical),
        receipts_canister::store::Response::Success
    );

    let resp = http_get(env, canister_ids, format!("/receipt?id={receipt_id_hex}"));
    assert_eq!(resp.status_code, 200);
    let body = String::from_utf8(resp.body).expect("served bytes are utf-8 json");

    // None of the three distinctive PII sentinels — registered handle, registered
    // display name, or the email-shaped sentinel (the user-canister receipt carries
    // no email field by construction; this asserts that category stays clean too) —
    // may appear in the served receipt.
    for sentinel in [SENTINEL_HANDLE, SENTINEL_NAME, SENTINEL_EMAIL] {
        assert!(
            !body.contains(sentinel),
            "served receipt must not contain PII sentinel {sentinel:?}"
        );
    }

    // The generated username is also plaintext profile PII and must not appear.
    let username = crate::utils::principal_to_username(user.principal);
    assert!(
        !body.contains(&username),
        "served receipt must not contain the plaintext username"
    );

    // record_id is a hex string in the JSON; it must be the 32-byte (64-hex) hash
    // and must NOT equal the raw subject (user-canister) principal bytes.
    let json: serde_json::Value = serde_json::from_str(&body).expect("served body is json");
    let record_id = json["record_id"].as_str().expect("record_id is a hex string");
    assert_eq!(record_id.len(), 64, "record_id must be a 32-byte hash, got {} hex chars", record_id.len());
    let subject_principal_hex = hex::encode(Principal::from(user.user_id).as_slice());
    assert_ne!(
        record_id, subject_principal_hex,
        "record_id must be a hash, not the raw subject principal"
    );
}

// ---------------------------------------------------------------------------
// Full pre-uninstall pipeline (§8): export → uninstall → durable post-uninstall
// fetch, and the retained-copy-first export-failure branch (§10).
// ---------------------------------------------------------------------------

/// Returns true once the user canister has been uninstalled (no module hash).
fn is_uninstalled(env: &PocketIc, user: &User) -> bool {
    is_uninstalled_ids(env, user.canister(), user.local_user_index)
}

fn is_uninstalled_ids(env: &PocketIc, user_canister: CanisterId, lui: CanisterId) -> bool {
    is_uninstalled_by(env, user_canister, lui)
}

/// `canister_status` requires the sender to be a controller, so tests that
/// deliberately remove the LUI from a user canister's controllers must query
/// status via the controller they substituted in.
fn is_uninstalled_by(env: &PocketIc, user_canister: CanisterId, status_sender: Principal) -> bool {
    env.canister_status(user_canister, Some(status_sender)).unwrap().module_hash.is_none()
}

/// Wait for the asynchronous LUI-upgrade job to FULLY drain.
/// `upgrade_local_user_index_canister_wasm` only enqueues the work; the user_index
/// then stops → installs → RESTARTS each LUI (the env has one per application
/// subnet) over several rounds. Querying the LUIs directly is racy — right after we
/// enqueue they are still Running (upgrade not started), and mid-upgrade they are
/// stopped — so we instead poll the user_index (which is never stopped) until it
/// reports NO pending and NO in-progress upgrades. The job marks an upgrade complete
/// only after the LUI is restarted, so this is the race-free "every LUI is upgraded
/// AND running again" signal. A fixed `tick_many` here would otherwise leave a
/// still-stopped LUI and cascade `CanisterStopped` through the suite (#15). The
/// small periodic time-advance is well under the 5-minute park cadence.
fn await_lui_upgrades_complete(env: &mut PocketIc, canister_ids: &CanisterIds) -> bool {
    for i in 0..80 {
        let resp = client::http_request(
            env,
            Principal::anonymous(),
            canister_ids.user_index,
            &HttpRequest { method: "GET".to_string(), url: "/metrics".to_string(), headers: Vec::new(), body: Vec::new() },
        );
        if resp.status_code == 200 {
            if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&resp.body) {
                let pending = json["canister_upgrades_pending"].as_u64().unwrap_or(1);
                let in_progress = json["canister_upgrades_in_progress"].as_u64().unwrap_or(1);
                if pending == 0 && in_progress == 0 {
                    // Let the just-restarted LUIs settle their post_upgrade timers.
                    tick_many(env, 4);
                    return true;
                }
            }
        }
        if i % 4 == 3 {
            env.advance_time(Duration::from_secs(1));
        }
        tick_many(env, 4);
    }
    false
}

/// The aggregate count of DURABLE parked-export records (`export_pending.len()`),
/// read from the public `/metrics` route. Per G's ruling (b) there is NO
/// per-canister HTTP route; this aggregate is the only exposed observability, so
/// the parked-state tests assert RELATIVE deltas (robust to env-pool residue from
/// prior un-resumed parked records). The count is sourced solely from the stable
/// `ExportPending` map — so its survival across a LUI upgrade directly proves the
/// durable structure survived (the in-memory queue contributes nothing to it).
fn export_pending_count(env: &PocketIc, lui: CanisterId) -> u64 {
    let resp = client::http_request(
        env,
        Principal::anonymous(),
        lui,
        &HttpRequest { method: "GET".to_string(), url: "/metrics".to_string(), headers: Vec::new(), body: Vec::new() },
    );
    assert_eq!(resp.status_code, 200, "metrics route must be 200");
    let json: serde_json::Value = serde_json::from_slice(&resp.body).expect("metrics json");
    json["receipt_export_pending_count"].as_u64().expect("receipt_export_pending_count")
}

/// Aggregate count of records in the `ExportedUninstallPending` state (export
/// confirmed, only uninstall remains), from the public `/metrics`.
fn eup_count(env: &PocketIc, lui: CanisterId) -> u64 {
    let resp = client::http_request(
        env,
        Principal::anonymous(),
        lui,
        &HttpRequest { method: "GET".to_string(), url: "/metrics".to_string(), headers: Vec::new(), body: Vec::new() },
    );
    assert_eq!(resp.status_code, 200, "metrics route must be 200");
    let json: serde_json::Value = serde_json::from_slice(&resp.body).expect("metrics json");
    json["receipt_export_uninstall_pending_count"].as_u64().expect("receipt_export_uninstall_pending_count")
}

/// Self-isolate: authorize the LUI and drain the slow retry until THIS LUI has no
/// pending records left (count 0). Resolves any `Parked` (export-blocked) residue
/// left by earlier tests on the reused env, so a following assertion of "count 0"
/// is a DIRECT, per-record signal rather than env-pool noise. (EUP records stuck
/// on a deliberately-broken uninstall are cleaned up by their own test re-enabling
/// uninstall — see `exported_uninstall_pending_*`.)
fn drain_lui_to_empty(env: &mut PocketIc, controller: Principal, canister_ids: &CanisterIds, lui: CanisterId) {
    let _ = client::receipts::add_authorized_principal(
        env,
        controller,
        canister_ids.receipts,
        &receipts_canister::add_authorized_principal::Args { principal: lui },
    );
    assert!(
        step_until(env, PARK_RETRY_STEP_MS, 14, |env| export_pending_count(env, lui) == 0),
        "failed to drain LUI pending records to zero for an isolated start"
    );
}

/// #15 teardown: restore the shared env after the EUP lifecycle test, whether it
/// completed or panicked on an assertion. The test deliberately breaks uninstall by
/// removing the LUI from the user canister's controllers; if it fails mid-flow that
/// leaves a stranded `ExportedUninstallPending` record whose uninstall retry can
/// NEVER succeed (the LUI is no longer a controller), so the shared LUI's pending
/// count never returns to zero and every later receipts test that drains-to-empty
/// cascades. Re-adding the LUI as a controller lets the stranded record drain;
/// re-authorizing + draining returns the shared LUI to a clean state. Best-effort
/// and panic-proof: a stopped/unreachable LUI here must not raise a second panic
/// that masks the real failure.
fn restore_shared_env_after_eup(
    env: &mut PocketIc,
    controller: Principal,
    canister_ids: &CanisterIds,
    user_canister: CanisterId,
    lui: CanisterId,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // If the test panicked mid-upgrade the LUI may be stopped; tick the upgrade
        // job to completion so it is Running again before we touch it (and so the
        // next test never inherits a stopped LUI).
        let _ = await_lui_upgrades_complete(env, canister_ids);
        // Re-add the LUI as a controller so a stranded EUP record's uninstall retry
        // can complete (idempotent if the test already restored it).
        let _ = env.set_controllers(user_canister, Some(controller), vec![controller, lui]);
        // Re-authorize the LUI so any parked export can resume (idempotent).
        let _ = client::receipts::add_authorized_principal(
            env,
            controller,
            canister_ids.receipts,
            &receipts_canister::add_authorized_principal::Args { principal: lui },
        );
        // Drain this LUI's pending/EUP records back to empty so nothing carries into
        // the next test on the shared env.
        let _ = step_until(env, PARK_RETRY_STEP_MS, 14, |env| export_pending_count(env, lui) == 0);
    }));
}

/// Advance virtual time past the fast-retry backoff and tick, until `cond` holds
/// or `max_steps` is reached. Deterministic — waits for the actual condition
/// rather than a fixed tick count.
fn step_until(env: &mut PocketIc, step_ms: u64, max_steps: u32, mut cond: impl FnMut(&mut PocketIc) -> bool) -> bool {
    for _ in 0..max_steps {
        if cond(env) {
            return true;
        }
        env.advance_time(Duration::from_millis(step_ms));
        // Enough ticks to fully resolve one attempt's c2c round-trips
        // (groups_and_communities + export_receipt + store) on a busy reused env.
        tick_many(env, 8);
    }
    cond(env)
}

const FAST_RETRY_STEP_MS: u64 = 31_000;
const PARK_RETRY_STEP_MS: u64 = 5 * 60_000 + 1_000;

/// Drive the deletion pipeline to the DURABLE parked state: let the delete event
/// reach the LUI queue, then exhaust the bounded fast-retry window. Robust to the
/// shared/reused PocketIC env (extra steps + an initial settle).
/// Ensure the LUI is NOT in the receipts export authority. The authority set is
/// shared env-pool state (a prior success/resume test may have authorized this
/// LUI), so failure-path tests must explicitly revoke to force a deterministic
/// export failure.
fn revoke_lui_export_rights(env: &mut PocketIc, controller: Principal, canister_ids: &CanisterIds, lui: CanisterId) {
    let r = client::receipts::remove_authorized_principal(
        env,
        controller,
        canister_ids.receipts,
        &receipts_canister::remove_authorized_principal::Args { principal: lui },
    );
    assert!(matches!(r, types::SuccessOnly::Success), "revoke LUI export rights: {r:?}");
}

/// Drive the deletion pipeline until the user's export is DURABLY parked: the
/// parked count rises by one and STAYS elevated across more fast-retry cycles than
/// the (test-mode) park threshold — proving the item was moved into the durable
/// stable set, not silently dropped at the retry limit (the old bug).
fn drive_to_parked(env: &mut PocketIc, lui: CanisterId, baseline: u64) -> bool {
    // Let the DeleteUser event propagate identity → user_index → LUI queue.
    tick_many(env, 10);
    // A durable record appears on the first export failure.
    if !step_until(env, FAST_RETRY_STEP_MS, 40, |env| export_pending_count(env, lui) > baseline) {
        return false;
    }
    // Exhaust well beyond the park threshold; the durable count must stay up.
    for _ in 0..8 {
        env.advance_time(Duration::from_millis(FAST_RETRY_STEP_MS));
        tick_many(env, 8);
    }
    export_pending_count(env, lui) > baseline
}

/// SUCCESS PATH: with the local_user_index authorized as an export principal, a
/// finalized CVDR is exported to the receipts canister BEFORE the user canister
/// is uninstalled, and the receipt remains fetchable (byte-identical, and valid
/// as a finalized receipt — the CVDR-Verify V1 structural acceptance) AFTER the
/// source canister is destroyed. Waits for the actual uninstall condition.
#[test]
#[ignore = "Superseded by CVDR-on-Index v5; P2 LUI→receipts export path banked."]
fn finalized_receipt_survives_uninstall_and_is_fetchable() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, controller } = wrapper.env();

    let (user, user_auth) = client::register_user_and_include_auth(env, canister_ids);
    let (receipt_id, canonical) = finalize_user(env, &user);
    let receipt_id_hex = hex::encode(&receipt_id);

    // Authorize THIS user's local_user_index as an export principal (the dynamic
    // id, granted env/operator-style via the controller-gated endpoint).
    let auth_response = client::receipts::add_authorized_principal(
        env,
        *controller,
        canister_ids.receipts,
        &receipts_canister::add_authorized_principal::Args { principal: user.local_user_index },
    );
    assert!(matches!(auth_response, types::SuccessOnly::Success), "authorize LUI: {auth_response:?}");

    // Drive the real deletion pipeline (identity → user_index → local_user_index),
    // waiting for the actual uninstall rather than assuming a fixed tick count.
    client::local_user_index::happy_path::prepare_account_deletion(env, &user);
    client::identity::happy_path::delete_user(env, &user_auth, canister_ids.identity);
    let (uc, lui) = (user.canister(), user.local_user_index);
    assert!(
        step_until(env, FAST_RETRY_STEP_MS, 15, |env| is_uninstalled_ids(env, uc, lui)),
        "user canister must be uninstalled after a successful export"
    );

    // … but the receipt survives in the durable store, byte-identical to the export.
    let resp = http_get(env, canister_ids, format!("/receipt?id={receipt_id_hex}"));
    assert_eq!(resp.status_code, 200, "receipt must remain fetchable post-uninstall");
    assert_eq!(resp.body, canonical, "post-uninstall served bytes must be byte-identical to the export");

    // CVDR-Verify V1 (structural acceptance): the served bytes parse as a
    // FINALIZED receipt (BLS certificate present) whose embedded receipt_id
    // matches the requested id. (`CVDR-Verify` proper is an external tool; this is
    // the in-repo structural-validity + byte-identity equivalent.)
    let receipt: serde_json::Value = serde_json::from_slice(&resp.body).expect("served bytes parse as json");
    assert!(
        !receipt["bls_certificate"].is_null(),
        "served receipt must be finalized (BLS certificate present)"
    );
    assert_eq!(
        receipt["receipt_id"].as_str(),
        Some(receipt_id_hex.as_str()),
        "served receipt id must match the requested id"
    );
}

/// FAILURE PATH → DURABLE PARKED STATE (G option (c), §1/§8/§10): a finalized
/// receipt that cannot be exported (LUI not in the receipts export authority)
/// must reach the DURABLE parked set — NOT disappear from the queue — while
/// uninstall stays unreachable, the tombstone/finalization never rolls back, and
/// nothing is stored.
#[test]
#[ignore = "Superseded by CVDR-on-Index v5; P2 LUI→receipts export path banked."]
fn export_failure_reaches_durable_parked_state() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, controller } = wrapper.env();

    let (user, user_auth) = client::register_user_and_include_auth(env, canister_ids);
    let (receipt_id, _canonical) = finalize_user(env, &user);
    let receipt_id_hex = hex::encode(&receipt_id);
    let lui = user.local_user_index;
    let baseline = export_pending_count(env, lui);

    // LUI deliberately NOT authorized → store rejected → export can never succeed.
    revoke_lui_export_rights(env, *controller, canister_ids, lui);
    client::local_user_index::happy_path::prepare_account_deletion(env, &user);
    client::identity::happy_path::delete_user(env, &user_auth, canister_ids.identity);

    // Drive the bounded fast-retry window past its limit → DURABLE parked record.
    // (Per-canister status/attempt/error_class are in the error!/warn! logs;
    // here we assert the durable aggregate the metric exposes.)
    let parked = drive_to_parked(env, lui, baseline);
    assert!(parked, "export failure must reach the durable parked set, not vanish from the queue");
    assert_eq!(
        export_pending_count(env, lui),
        baseline + 1,
        "exactly one durable parked record must be retained"
    );

    // Retained-copy-first: uninstall must NOT have fired.
    assert!(!is_uninstalled(env, &user), "uninstall must be unreachable while export is parked");

    // Tombstone/finalization did not roll back — Phase A again is D8-rejected.
    let phase_a_again = client::user::mktd_execute_deletion(env, user.principal, user.canister(), &Empty {});
    assert!(
        !matches!(phase_a_again, user_canister::mktd_execute_deletion::Response::Success(_)),
        "tombstoned/finalized state must not have rolled back, got {phase_a_again:?}"
    );

    // Nothing leaked into the durable store.
    assert_eq!(
        http_get(env, canister_ids, format!("/receipt?id={receipt_id_hex}")).status_code,
        404,
        "a failed export must not have stored anything"
    );
}

/// G's hard point: the parked record lives in STABLE memory and must survive a
/// `local_user_index` upgrade (not heap/job-queue state).
#[test]
#[ignore = "Superseded by CVDR-on-Index v5; P2 LUI→receipts export path banked."]
fn parked_export_survives_local_user_index_upgrade() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, controller } = wrapper.env();

    let (user, user_auth) = client::register_user_and_include_auth(env, canister_ids);
    finalize_user(env, &user);
    let lui = user.local_user_index;
    let baseline = export_pending_count(env, lui);

    revoke_lui_export_rights(env, *controller, canister_ids, lui);
    client::local_user_index::happy_path::prepare_account_deletion(env, &user);
    client::identity::happy_path::delete_user(env, &user_auth, canister_ids.identity);
    let parked = drive_to_parked(env, lui, baseline);
    assert!(parked, "must be parked before the upgrade");
    let count_before = export_pending_count(env, lui);
    assert_eq!(count_before, baseline + 1, "one durable parked record before upgrade");

    // Upgrade ALL local_user_index canisters via the real user_index upgrade path
    // (bumped version over the same module → a genuine upgrade through
    // pre_upgrade/post_upgrade).
    let upgraded_wasm = CanisterWasm {
        version: BuildVersion::new(0, 0, 1),
        module: wasms::LOCAL_USER_INDEX.module.clone(),
    };
    client::user_index::happy_path::upgrade_local_user_index_canister_wasm(
        env,
        *controller,
        canister_ids.user_index,
        upgraded_wasm,
    );
    // Wait for the async upgrade to RESTART the LUI rather than assuming a fixed
    // tick count — a still-stopped LUI here would cascade through the suite (#15).
    assert!(await_lui_upgrades_complete(env, canister_ids), "LUI upgrade job must fully drain (all LUIs restarted)");

    // The durable count is sourced solely from the stable `ExportPending` map, so
    // its survival across the upgrade proves the stable structure survived (a
    // heap/queue-backed structure would not contribute to it post-upgrade).
    assert_eq!(
        export_pending_count(env, lui),
        count_before,
        "durable parked record must survive the LUI upgrade"
    );
    assert!(!is_uninstalled(env, &user), "uninstall still unreachable after upgrade");
}

/// RESUME / SELF-HEAL (G option (c)): once the receipts canister accepts the
/// store (LUI authorized), the parked drain must resume → export → uninstall,
/// remove the durable record, and — leaning on store idempotency — create no
/// duplicate divergent record.
#[test]
#[ignore = "Superseded by CVDR-on-Index v5; P2 LUI→receipts export path banked."]
fn parked_export_resumes_after_authorization() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, controller } = wrapper.env();

    // Isolate FIRST (advances virtual time) BEFORE minting the auth delegation, so
    // a residue-laden suite env can't expire the delegation before `delete_user`
    // (same #15 root cause as the EUP test). The registration LUI is deterministic.
    let lui = client::user_index::happy_path::user_registration_canister(env, canister_ids.user_index);
    let _ = await_lui_upgrades_complete(env, canister_ids);
    // Drain any residual pending records so this LUI has none. After this,
    // `export_pending_count == 0`, so a later "count back to 0" is a DIRECT,
    // per-record signal for THIS user — not an uninstall proxy and not env noise.
    drain_lui_to_empty(env, *controller, canister_ids, lui);

    let (user, user_auth) = client::register_user_and_include_auth(env, canister_ids);
    assert_eq!(user.local_user_index, lui, "registration must land on the drained LUI");
    let (receipt_id, canonical) = finalize_user(env, &user);
    let receipt_id_hex = hex::encode(&receipt_id);
    let user_canister = user.canister();

    // Start unauthorized so the export deterministically fails and parks.
    revoke_lui_export_rights(env, *controller, canister_ids, lui);
    client::local_user_index::happy_path::prepare_account_deletion(env, &user);
    client::identity::happy_path::delete_user(env, &user_auth, canister_ids.identity);
    let parked = drive_to_parked(env, lui, 0);
    assert!(parked, "must be parked before resume");
    assert_eq!(export_pending_count(env, lui), 1, "exactly this user's record is parked (isolated)");
    assert!(!is_uninstalled(env, &user), "not uninstalled while parked");

    // Operator grants the LUI export rights — the receipts canister now recovers.
    let auth_response = client::receipts::add_authorized_principal(
        env,
        *controller,
        canister_ids.receipts,
        &receipts_canister::add_authorized_principal::Args { principal: lui },
    );
    assert!(matches!(auth_response, types::SuccessOnly::Success), "authorize LUI: {auth_response:?}");

    // The slow drain must self-heal: export → ExportedUninstallPending → uninstall →
    // record removed.
    assert!(
        step_until(env, PARK_RETRY_STEP_MS, 10, |env| export_pending_count(env, lui) == 0),
        "resumed export must remove this user's durable record"
    );

    // DIRECT per-user assertion (not uninstall-as-proxy): `export_pending` no longer
    // holds this user's record — the only record, since we isolated above.
    assert_eq!(export_pending_count(env, lui), 0, "this user's record must be gone from export_pending");
    assert!(is_uninstalled_ids(env, user_canister, lui), "and its canister must be uninstalled");

    // THIS user's receipt is stored exactly once, byte-identical (idempotency → no
    // duplicate divergent record) — the proof its specific export succeeded.
    let resp = http_get(env, canister_ids, format!("/receipt?id={receipt_id_hex}"));
    assert_eq!(resp.status_code, 200, "receipt fetchable after resumed export");
    assert_eq!(resp.body, canonical, "resumed export bytes are byte-identical");
}

/// EXPORTED-UNINSTALL-PENDING lifecycle (G's reorder). Export SUCCEEDS but
/// uninstall is made to fail (the LUI is removed from the user canister's
/// controllers, so `uninstall_code` is unauthorized while the caller-gated export
/// c2c still works). Covers, in one flow:
///  - the receipt IS durably exported, then the record is persisted as
///    `ExportedUninstallPending` BEFORE uninstall (the ordering guarantee);
///  - uninstall failure leaves it durable as EUP — NO rollback to "not exported"
///    across repeated retries;
///  - EUP survives a LUI upgrade in that state;
///  - complete-deletion bookkeeping does NOT run while EUP (canister still installed);
///  - re-enabling uninstall → the drain completes uninstall, removes the record,
///    and runs bookkeeping; idempotent (no duplicate divergent receipt).
#[test]
#[ignore = "Superseded by CVDR-on-Index v5; P2 LUI→receipts export path banked."]
fn exported_uninstall_pending_lifecycle() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, controller } = wrapper.env();

    // Isolate FIRST, BEFORE minting the auth delegation. `drain_lui_to_empty`
    // advances virtual time to clear residue; on a residue-laden suite env that can
    // be tens of minutes. Done after registration (as before) it expired the auth
    // delegation before `delete_user` (the #15 root cause: passed in isolation where
    // the drain is instant, failed in-suite). Draining first keeps the delegation
    // fresh. The registration LUI is deterministic, so we can drain it up front.
    let lui = client::user_index::happy_path::user_registration_canister(env, canister_ids.user_index);
    let _ = await_lui_upgrades_complete(env, canister_ids);
    drain_lui_to_empty(env, *controller, canister_ids, lui);

    let (user, user_auth) = client::register_user_and_include_auth(env, canister_ids);
    assert_eq!(user.local_user_index, lui, "registration must land on the drained LUI");
    let (receipt_id, canonical) = finalize_user(env, &user);
    let receipt_id_hex = hex::encode(&receipt_id);
    let user_canister = user.canister();

    // #15: run the whole lifecycle inside a panic guard so that — on ANY assertion
    // failure — the teardown below still restores the shared env (controllers,
    // authorization, drain) and the failure can't cascade through the rest of the
    // suite via a stranded EUP record / removed controller.
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
    assert_eq!(eup_count(env, lui), 0, "clean EUP baseline");

    // Authorize the LUI so EXPORT succeeds…
    let auth = client::receipts::add_authorized_principal(
        env,
        *controller,
        canister_ids.receipts,
        &receipts_canister::add_authorized_principal::Args { principal: lui },
    );
    assert!(matches!(auth, types::SuccessOnly::Success));

    // …but break UNINSTALL: remove the LUI from the user canister's controllers
    // (uninstall_code is controller-gated; the export c2c is caller-gated, so it
    // still works). `controller` becomes the sole controller (used for status).
    env.set_controllers(user_canister, Some(lui), vec![*controller])
        .expect("set_controllers to break uninstall");

    client::local_user_index::happy_path::prepare_account_deletion(env, &user);
    client::identity::happy_path::delete_user(env, &user_auth, canister_ids.identity);

    // Export succeeds → EUP persisted before uninstall → uninstall fails → durable EUP.
    assert!(
        step_until(env, FAST_RETRY_STEP_MS, 30, |env| eup_count(env, lui) == 1),
        "export must succeed and persist ExportedUninstallPending before a failing uninstall"
    );
    assert_eq!(export_pending_count(env, lui), 1, "the EUP record is the only pending record");
    assert!(!is_uninstalled_by(env, user_canister, *controller), "canister still installed while EUP (bookkeeping not run)");

    // The receipt IS durably exported already (proves export preceded the EUP state).
    let resp = http_get(env, canister_ids, format!("/receipt?id={receipt_id_hex}"));
    assert_eq!(resp.status_code, 200, "receipt durably exported before uninstall");
    assert_eq!(resp.body, canonical, "exported bytes byte-identical");

    // No rollback: drive several drain cycles while uninstall keeps failing — the
    // record must STAY ExportedUninstallPending (never flips back to not-exported).
    for _ in 0..3 {
        env.advance_time(Duration::from_millis(PARK_RETRY_STEP_MS));
        tick_many(env, 8);
    }
    assert_eq!(eup_count(env, lui), 1, "still EUP after repeated failing uninstall retries (no rollback)");
    assert_eq!(export_pending_count(env, lui), 1, "no extra/duplicate record");
    assert!(!is_uninstalled_by(env, user_canister, *controller), "still not uninstalled");

    // EUP survives a real LUI upgrade.
    let upgraded_wasm = CanisterWasm {
        version: BuildVersion::new(0, 0, 2),
        module: wasms::LOCAL_USER_INDEX.module.clone(),
    };
    client::user_index::happy_path::upgrade_local_user_index_canister_wasm(env, *controller, canister_ids.user_index, upgraded_wasm);
    // Wait for the async upgrade to RESTART the LUI rather than assuming a fixed
    // tick count — a still-stopped LUI here would cascade through the suite (#15).
    assert!(await_lui_upgrades_complete(env, canister_ids), "LUI upgrade job must fully drain (all LUIs restarted)");
    assert_eq!(eup_count(env, lui), 1, "ExportedUninstallPending must survive the LUI upgrade");
    assert!(!is_uninstalled_by(env, user_canister, *controller), "still installed after upgrade");

    // Re-enable uninstall (re-add the LUI as a controller) → the drain retries
    // uninstall (skipping re-export), completes, and removes the record.
    env.set_controllers(user_canister, Some(*controller), vec![*controller, lui])
        .expect("restore LUI as controller");
    assert!(
        step_until(env, PARK_RETRY_STEP_MS, 10, |env| export_pending_count(env, lui) == 0),
        "retry from ExportedUninstallPending must complete uninstall and remove the record"
    );

    // DIRECT removal + completion: record gone, canister uninstalled, receipt still
    // present exactly once (idempotent — re-export was skipped, no divergent record).
    assert_eq!(eup_count(env, lui), 0, "EUP record removed after completion");
    assert_eq!(export_pending_count(env, lui), 0, "no pending record remains");
    assert!(is_uninstalled_by(env, user_canister, *controller), "canister uninstalled after completion");
    let resp = http_get(env, canister_ids, format!("/receipt?id={receipt_id_hex}"));
    assert_eq!(resp.status_code, 200, "receipt still fetchable after completion");
    assert_eq!(resp.body, canonical, "no duplicate divergent receipt");
    })); // end panic guard

    // #15 teardown — runs on success AND on panic.
    restore_shared_env_after_eup(env, *controller, canister_ids, user_canister, lui);

    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}
