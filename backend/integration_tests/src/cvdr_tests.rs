//! CVDR-on-Index PocketIC suite.
//!
//! **Slice 1 baseline (spec §8/§8b).** The delete leg now COMPLETES user deletion (uninstall +
//! mapping removal + membership notifications) inside the job, INDEPENDENTLY of certificate
//! capture — the cert-absent path is the live path. CVDR finalization (capturing the IC
//! certificate into the immutable frozen package) is Slice 2 (self-finalization, spec §6) /
//! Slice 3 (backstop, spec §7); in Slice 1 `finalize_cvdr` and `cvdr_data_certificate` are inert
//! stubs (spec §8b). The single certified-data slot + global single-flight guard are gone (spec
//! §2 — a certified receipt tree holds many receipts under one root).
//!
//! Tests that exercise finalize / released-CVDR store / `/cvdr` serving / the removed single-slot
//! guard / on-chain certificate verification therefore assert Slice 2/3 behavior and are
//! `#[ignore]`d here (kept compiling, re-enabled when that behavior lands). The live Slice-1
//! assertions are: delete reaches `AwaitingCertificate` with the user fully deleted and NOTHING
//! finalized (this file) + membership removal with certificate capture entirely absent
//! (`delete_user_tests::deleted_user_removed_from_groups_and_communities`).

use crate::client::register_user_and_include_auth;
use crate::env::ENV;
use crate::utils::tick_many;
use crate::{CanisterIds, TestEnv, User, UserAuth, client};
use candid::Principal;
use local_user_index_canister::{cvdr_data_certificate, finalize_cvdr, get_cvdr};
use pocket_ic::PocketIc;
use pocket_ic::common::rest::{CanisterHttpReply, CanisterHttpResponse, MockCanisterHttpResponse};
use std::ops::Deref;
use std::time::Duration;
use types::{CanisterId, Empty, HttpRequest};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Register users until two land on the SAME local_user_index (the test env spans several
/// user subnets, and the single certified-data slot is per-canister). Returns the two
/// colliding users.
fn two_users_same_lui(env: &mut PocketIc, canister_ids: &CanisterIds) -> ((User, UserAuth), (User, UserAuth)) {
    let mut pool: Vec<(User, UserAuth)> = Vec::new();
    for _ in 0..12 {
        let (user, auth) = register_user_and_include_auth(env, canister_ids);
        if let Some(idx) = pool.iter().position(|(existing, _)| existing.local_user_index == user.local_user_index) {
            let first = pool.swap_remove(idx);
            return (first, (user, auth));
        }
        pool.push((user, auth));
    }
    panic!("could not find two users on the same local_user_index after 12 registrations");
}

/// Trigger a user deletion and let the forward-only delete job drive it to
/// `AwaitingCertificate` (capture -> uninstall -> publish commitment).
fn delete_and_reach_awaiting(env: &mut PocketIc, canister_ids: &CanisterIds, user: &UserAuth) {
    client::identity::happy_path::delete_user(env, user, canister_ids.identity);
    tick_many(env, 10);
}

/// Read the pending certificate the off-chain finalizer would relay. Ticks first so the
/// commitment published by the job is sealed into a certificate before the query reads
/// `data_certificate()`.
fn pending_certificate(env: &mut PocketIc, local_user_index: CanisterId) -> cvdr_data_certificate::SuccessResult {
    tick_many(env, 2);
    match client::local_user_index::cvdr_data_certificate(env, Principal::anonymous(), local_user_index, &Empty {}) {
        cvdr_data_certificate::Response::Success(r) => {
            assert!(!r.certificate.is_empty(), "HALT: certificate empty — PocketIC did not certify data");
            r
        }
        cvdr_data_certificate::Response::NotAvailable => panic!("expected a pending certificate, got NotAvailable"),
    }
}

fn finalize(
    env: &mut PocketIc,
    local_user_index: CanisterId,
    receipt_id: [u8; 32],
    certificate: Vec<u8>,
    witness: Vec<u8>,
) -> finalize_cvdr::Response {
    client::local_user_index::finalize_cvdr(
        env,
        Principal::anonymous(),
        local_user_index,
        &finalize_cvdr::Args { receipt_id, certificate, witness },
    )
}

/// Source a live `{receipt_id, certificate, witness}` backstop snapshot the way an operator
/// relaying to the §7 backstop would. Delegates to [`find_servable_cvdr_live`].
fn pending_live_package(env: &mut PocketIc, local_user_index: CanisterId) -> ([u8; 32], Vec<u8>, Vec<u8>) {
    let (_, receipt_id, certificate, witness) = find_servable_cvdr_live(env, local_user_index);
    (receipt_id, certificate, witness)
}

/// Read + parse the canister's own `/cvdr_live/<receipt_id>` payload into `(certificate, witness)`
/// bytes. `None` if the route 404s or omits the certificate (not a query context).
fn fetch_cvdr_live(env: &PocketIc, local_user_index: CanisterId, receipt_hex: &str) -> Option<(Vec<u8>, Vec<u8>)> {
    let live = client::http_request(
        env,
        Principal::anonymous(),
        local_user_index,
        &HttpRequest { method: "GET".to_string(), url: format!("/cvdr_live/{receipt_hex}"), headers: Vec::new(), body: Vec::new() },
    );
    if live.status_code != 200 {
        return None;
    }
    #[derive(serde::Deserialize)]
    struct Live {
        witness_cbor: String,
        certificate: Option<String>,
    }
    let parsed: Live = serde_json::from_slice(&live.body).ok()?;
    Some((hex::decode(parsed.certificate?).ok()?, hex::decode(parsed.witness_cbor).ok()?))
}

/// Find a finalizable receipt on THIS local_user_index whose `/cvdr_live` route CURRENTLY serves a
/// full snapshot (certificate present), and return its pending outcall request (for mocking), the
/// receipt_id, and the parsed `(certificate, witness)`.
///
/// Robust against the shared env pool: it URL-scopes to this lui AND skips stale outcalls left by
/// already-captured receipts (whose `/cvdr_live` now 404s) by requiring the fetch to succeed — so a
/// prior test's leftover outcall can never be mistaken for a live one.
fn find_servable_cvdr_live(
    env: &mut PocketIc,
    local_user_index: CanisterId,
) -> (pocket_ic::common::rest::CanisterHttpRequest, [u8; 32], Vec<u8>, Vec<u8>) {
    let lui_text = local_user_index.to_text();
    for _ in 0..30 {
        let candidates: Vec<pocket_ic::common::rest::CanisterHttpRequest> = env
            .get_canister_http()
            .into_iter()
            .filter(|r| r.url.contains("/cvdr_live/") && r.url.contains(&lui_text))
            .collect();
        for req in candidates {
            let receipt_hex = req.url.rsplit('/').next().unwrap().to_string();
            if let Some((certificate, witness)) = fetch_cvdr_live(env, local_user_index, &receipt_hex) {
                let receipt_id: [u8; 32] = hex::decode(&receipt_hex).unwrap().try_into().unwrap();
                return (req, receipt_id, certificate, witness);
            }
        }
        tick_many(env, 2);
        env.advance_time(Duration::from_secs(5));
    }
    panic!("no servable /cvdr_live receipt for {lui_text} appeared");
}

fn fetch_cvdr(env: &PocketIc, local_user_index: CanisterId, receipt_id: [u8; 32]) -> get_cvdr::Response {
    client::local_user_index::get_cvdr(env, Principal::anonymous(), local_user_index, &get_cvdr::Args { receipt_id })
}

/// The v5 metrics triple (drafts_in_flight, released_count, awaiting_certificate) read off
/// the public `/metrics` http route.
fn cvdr_metrics(env: &PocketIc, local_user_index: CanisterId) -> (u64, u64, bool) {
    let response = client::http_request(
        env,
        Principal::anonymous(),
        local_user_index,
        &HttpRequest { method: "GET".to_string(), url: "/metrics".to_string(), headers: Vec::new(), body: Vec::new() },
    );
    assert_eq!(response.status_code, 200, "metrics http status");
    let json: serde_json::Value = serde_json::from_slice(&response.body).expect("metrics JSON");
    (
        json["cvdr_drafts_in_flight"].as_u64().expect("cvdr_drafts_in_flight"),
        json["cvdr_released_count"].as_u64().expect("cvdr_released_count"),
        json["cvdr_awaiting_certificate"].as_bool().expect("cvdr_awaiting_certificate"),
    )
}

fn cvdr_index_evidence_count(env: &PocketIc, local_user_index: CanisterId) -> u64 {
    metrics_json(env, local_user_index)["cvdr_index_evidence_count"]
        .as_u64()
        .expect("cvdr_index_evidence_count")
}

fn metrics_json(env: &PocketIc, canister_id: CanisterId) -> serde_json::Value {
    let response = client::http_request(
        env,
        Principal::anonymous(),
        canister_id,
        &HttpRequest { method: "GET".to_string(), url: "/metrics".to_string(), headers: Vec::new(), body: Vec::new() },
    );
    assert_eq!(response.status_code, 200, "metrics http status");
    serde_json::from_slice(&response.body).expect("metrics JSON")
}

/// Receipts-canister stored-receipt count — proof of whether the index ever called `store`.
fn receipts_stored(env: &PocketIc, receipts: CanisterId) -> u64 {
    metrics_json(env, receipts)["receipts_stored"].as_u64().expect("receipts_stored")
}

/// The LUI's P2 durable parked/export-pending counts (`export_pending.len()` and its
/// uninstall-pending sub-count) — both must stay flat under v5 (the parked set is never fed).
fn lui_export_pending(env: &PocketIc, lui: CanisterId) -> (u64, u64) {
    let m = metrics_json(env, lui);
    (
        m["receipt_export_pending_count"].as_u64().expect("receipt_export_pending_count"),
        m["receipt_export_uninstall_pending_count"].as_u64().expect("receipt_export_uninstall_pending_count"),
    )
}

fn module_hash_is_none(env: &PocketIc, user: &User) -> bool {
    env.canister_status(user.canister(), Some(user.local_user_index))
        .expect("canister_status")
        .module_hash
        .is_none()
}

/// Deterministically wait for an in-flight LUI upgrade to COMPLETE. Polls the LUI's `/metrics`
/// `wasm_version` (via a RAW query that tolerates the stop window) until it reports the expected
/// bumped version. That version only appears AFTER post_upgrade has run and the canister is
/// Running again — so this never returns mid-stop (no `CanisterStopped` race) and has no pre-stop
/// false positive. Replaces a fixed `tick_many`, which was non-deterministic under load.
fn wait_for_lui_version(env: &mut PocketIc, lui: CanisterId, expected: types::BuildVersion) {
    let req = HttpRequest { method: "GET".to_string(), url: "/metrics".to_string(), headers: Vec::new(), body: Vec::new() };
    let payload = candid::encode_one(&req).unwrap();
    for _ in 0..300 {
        tick_many(env, 1);
        // Raw query: a reject (canister stopping/upgrading) is tolerated, not panicked.
        let Ok(bytes) = env.query_call(lui, Principal::anonymous(), "http_request", payload.clone()) else {
            continue;
        };
        let Ok(resp) = candid::decode_one::<types::HttpResponse>(&bytes) else { continue };
        if resp.status_code != 200 {
            continue;
        }
        let Ok(json) = serde_json::from_slice::<serde_json::Value>(&resp.body) else { continue };
        let v = &json["wasm_version"];
        if v["major"].as_u64() == Some(expected.major as u64)
            && v["minor"].as_u64() == Some(expected.minor as u64)
            && v["patch"].as_u64() == Some(expected.patch as u64)
        {
            return;
        }
    }
    panic!("LUI {lui} did not reach version {expected} after upgrade");
}

/// Independent reimplementation of the canister's domain-tagged SHA-256 (`SHA-256(tag || parts)`),
/// for offline V1 hash-chain recomputation from receipt bytes alone.
fn tagged(tag: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut preimage = Vec::from(tag);
    for part in parts {
        preimage.extend_from_slice(part);
    }
    sha256::sha256(&preimage)
}

// ---------------------------------------------------------------------------
// Cases
// ---------------------------------------------------------------------------

/// Slice-1 happy path (spec §8/§8b): delete -> the job uninstalls the user canister, publishes
/// the receipt into the certified tree, and completes user deletion (cleanup) — all WITHOUT any
/// certificate. No finalization runs; the draft rests `AwaitingCertificate` and nothing is
/// released. Certificate capture + the frozen-package store are Slice 2/3.
#[test]
fn delete_completes_cert_absent_and_awaits_certificate() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;

    // Baseline: the env pool reuses envs across tests, so assert DELTAS, not absolute counts.
    let (base_d, base_r, _) = cvdr_metrics(env, lui);

    delete_and_reach_awaiting(env, canister_ids, &user_auth);

    // Uninstalled by the leg; one draft awaiting the certificate; nothing released (cert-absent).
    assert!(module_hash_is_none(env, &user), "user canister must be uninstalled by the leg");
    let (drafts, released, awaiting) = cvdr_metrics(env, lui);
    assert_eq!((drafts, released, awaiting), (base_d + 1, base_r, true), "one draft awaiting, none released (cert-absent)");

    // The §7 backstop rejects an unknown receipt_id at rule 1 (no finalizable draft) → NotPending.
    let response = finalize(env, lui, [0u8; 32], Vec::new(), Vec::new());
    assert!(matches!(response, finalize_cvdr::Response::NotPending), "unknown receipt must be NotPending, got {response:?}");
    // Frozen-package serving is Slice 2 — nothing is fetchable yet.
    assert!(matches!(fetch_cvdr(env, lui, [0u8; 32]), get_cvdr::Response::NotFound));
}

/// Store/fetch round-trip: after finalize the released CVDR is fetchable by `receipt_id` via
/// both the query and the `/cvdr` http route, and the fetch is byte-stable (deterministic).
#[test]
#[ignore = "Slice 2/3: finalize + frozen-package store + /cvdr serving. finalize_cvdr is an inert stub in Slice 1 (spec §8b)."]
fn cvdr_store_fetch_round_trip() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;

    delete_and_reach_awaiting(env, canister_ids, &user_auth);
    let pending = pending_certificate(env, lui);
    assert!(matches!(finalize(env, lui, pending.receipt_id, pending.certificate, Vec::new()), finalize_cvdr::Response::Captured));

    // Query path.
    let receipt = match fetch_cvdr(env, lui, pending.receipt_id) {
        get_cvdr::Response::Success(r) => r,
        other => panic!("get_cvdr expected Success, got {other:?}"),
    };
    assert_eq!(receipt.receipt_id, pending.receipt_id, "receipt_id round-trips");
    assert_eq!(receipt.commitment, pending.commitment, "commitment round-trips");
    assert_eq!(receipt.encoder_version, "OPENCHATZD_CVDR_V1", "pinned encoder version");
    assert_eq!(receipt.user_canister_id, user.canister(), "subject canister");
    assert!(!receipt.certificate.is_empty(), "embedded certificate present (V2)");
    assert_eq!(receipt.h_index.len(), 32);
    assert_eq!(receipt.h_user_pre.len(), 32);

    // Byte-stable: two identical queries serialize identically.
    let again = match fetch_cvdr(env, lui, pending.receipt_id) {
        get_cvdr::Response::Success(r) => r,
        other => panic!("get_cvdr expected Success, got {other:?}"),
    };
    assert_eq!(
        serde_json::to_vec(&receipt).unwrap(),
        serde_json::to_vec(&again).unwrap(),
        "repeated fetch must be byte-identical"
    );

    // Http path: `/cvdr?receipt_id=<hex>` serves the same receipt (hex-encoded).
    let receipt_id_hex = hex::encode(pending.receipt_id);
    let download = client::http_request(
        env,
        Principal::anonymous(),
        lui,
        &HttpRequest { method: "GET".to_string(), url: format!("/cvdr?receipt_id={receipt_id_hex}"), headers: Vec::new(), body: Vec::new() },
    );
    assert_eq!(download.status_code, 200, "http /cvdr must be 200 for a stored receipt");
    let body: serde_json::Value = serde_json::from_slice(&download.body).expect("cvdr http JSON");
    assert_eq!(body["receipt_id"].as_str().unwrap(), receipt_id_hex, "http receipt_id matches");
    assert_eq!(body["encoder_version"].as_str().unwrap(), "OPENCHATZD_CVDR_V1");

    // Unknown receipt_id -> NotFound / 404.
    assert!(matches!(fetch_cvdr(env, lui, [0u8; 32]), get_cvdr::Response::NotFound));
    let miss = client::http_request(
        env,
        Principal::anonymous(),
        lui,
        &HttpRequest { method: "GET".to_string(), url: format!("/cvdr?receipt_id={}", "0".repeat(64)), headers: Vec::new(), body: Vec::new() },
    );
    assert_eq!(miss.status_code, 404, "unknown receipt_id must 404");
}

/// Absent/stuck finalizer: with no finalizer, the leg reaches AwaitingCertificate and STAYS
/// there across many ticks — completion (released CVDR) is withheld — and a late finalizer
/// still completes it. Forward recovery, no auto-advance.
#[test]
#[ignore = "Slice 2/3: this asserted completion is GATED on the finalizer — inverted by spec §8 (user deletion now completes cert-absent). Finalizer/store behavior is Slice 2/3."]
fn absent_finalizer_withholds_completion_and_resumes() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;

    let (base_d, base_r, _) = cvdr_metrics(env, lui);

    delete_and_reach_awaiting(env, canister_ids, &user_auth);

    // Capture the receipt the finalizer WOULD see, but do not finalize yet.
    let pending = pending_certificate(env, lui);

    // Let many job intervals elapse with no finalizer.
    tick_many(env, 20);

    // Still awaiting, still nothing newly released — completion is withheld.
    let (drafts, released, awaiting) = cvdr_metrics(env, lui);
    assert_eq!((drafts, released, awaiting), (base_d + 1, base_r, true), "draft must persist AwaitingCertificate; nothing released");
    assert!(matches!(fetch_cvdr(env, lui, pending.receipt_id), get_cvdr::Response::NotFound), "no released CVDR yet");
    assert!(module_hash_is_none(env, &user), "canister stays uninstalled while awaiting");

    // A late finalizer resumes it to completion.
    let pending = pending_certificate(env, lui);
    assert!(matches!(finalize(env, lui, pending.receipt_id, pending.certificate, Vec::new()), finalize_cvdr::Response::Captured));
    let (drafts, released, awaiting) = cvdr_metrics(env, lui);
    assert_eq!((drafts, released, awaiting), (base_d, base_r + 1, false), "late finalize completes it");
}

/// Concurrency (reborn from the old single-slot serialization test): spec §2 removes the single
/// certified-data slot + global guard (the certified receipt tree holds many receipts under one
/// root), so two concurrent deletions on one local_user_index both reach `AwaitingCertificate`
/// AT ONCE — no serialization.
#[test]
fn concurrent_deletes_both_reach_awaiting_certificate() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let ((user_a, auth_a), (user_b, auth_b)) = two_users_same_lui(env, canister_ids);
    assert_eq!(user_a.local_user_index, user_b.local_user_index, "test requires a shared local_user_index");
    let lui = user_b.local_user_index;
    let (base_d, _, _) = cvdr_metrics(env, lui);

    client::identity::happy_path::delete_user(env, &auth_a, canister_ids.identity);
    client::identity::happy_path::delete_user(env, &auth_b, canister_ids.identity);
    tick_many(env, 15);

    // Both uninstalled and both in flight simultaneously — no single-slot serialization.
    assert!(
        module_hash_is_none(env, &user_a) && module_hash_is_none(env, &user_b),
        "both user canisters uninstalled by the leg"
    );
    let (drafts, _, awaiting) = cvdr_metrics(env, lui);
    assert_eq!(drafts, base_d + 2, "both deletions are in flight at once (no single-slot serialization)");
    assert!(awaiting, "receipts awaiting certificate capture");
}

/// spec §6 self-finalization loop (HARD SECURITY RULE), end-to-end with a MOCKED outcall:
/// delete -> publish -> the sweep makes a non-replicated outcall to the canister's own
/// `/cvdr_live/<receipt_id>` route; we mock that outcall with the route's real output (a genuine
/// PocketIC-signed certificate over the receipt-tree root); the store-gate verifies it in FULL
/// (BLS -> NNS -> witness -> root -> leaf -> window) and stores the frozen package ->
/// `CertificateCaptured`. PocketIC cannot exercise the real gateway loop (A1-mainnet-proven); this
/// proves timer -> outcall -> verify-before-store -> store with a mocked gateway response.
#[test]
fn self_finalization_captures_and_stores_via_mocked_outcall() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;
    let (_, base_frozen, _) = cvdr_metrics(env, lui);

    client::identity::happy_path::delete_user(env, &user_auth, canister_ids.identity);
    tick_many(env, 10);

    // The self-finalization outcall carries the receipt_id in its URL. Use the robust, lui-scoped,
    // servability-checked finder so the shared env pool can't hand us another lui's / a captured
    // receipt's stale outcall.
    let (req, receipt_id, _, _) = find_servable_cvdr_live(env, lui);
    let receipt_hex = hex::encode(receipt_id);

    // Mock body = the canister's own /cvdr_live payload (a real PocketIC certificate over the root).
    let live = client::http_request(
        env,
        Principal::anonymous(),
        lui,
        &HttpRequest { method: "GET".to_string(), url: format!("/cvdr_live/{receipt_hex}"), headers: Vec::new(), body: Vec::new() },
    );
    assert_eq!(live.status_code, 200, "/cvdr_live must serve the live certification payload");
    let live_body = live.body;

    env.mock_canister_http_response(MockCanisterHttpResponse {
        subnet_id: req.subnet_id,
        request_id: req.request_id,
        response: CanisterHttpResponse::CanisterHttpReply(CanisterHttpReply {
            status: 200,
            headers: Vec::new(),
            body: live_body.clone(),
        }),
        additional_responses: Vec::new(),
    });

    // Continuation runs the store-gate: verified in-window -> frozen package stored. (The
    // `cvdr_awaiting_certificate` metric is global and the env pool is shared across tests — other
    // tests leave AwaitingCertificate drafts whose outcalls are unmocked — so the frozen-package
    // delta, not the awaiting flag, is this test's capture evidence.)
    tick_many(env, 5);
    let (_, frozen, _) = cvdr_metrics(env, lui);
    assert_eq!(frozen, base_frozen + 1, "self-finalization verified the certificate + stored exactly one frozen package");

    // Export the PocketIC-produced FrozenCvdrPackage + PocketIC NNS root key as the CVDR-Verify
    // END-TO-END fixture: a REAL certificate binding a REAL receipts-tree root (closes CVDR-Verify's
    // honest-gap-2 — no real VerifiedFinal artifact). The mocked body carries a genuine PocketIC
    // certificate, so this is not synthetic.
    export_cvdr_verify_e2e_fixture(env, lui, &receipt_hex, &live_body);
}

/// INDEX capture plumbing under PocketIC: after a frozen package exists, the job POSTs
/// `read_state` for `/module_hash`. PocketIC cannot mint a real NNS-rooted system-state
/// certificate for that path, so this test proves (1) the outcall is issued, (2) an invalid
/// gateway body is discarded (verify-before-store; count stays flat), and (3) after the 24h
/// give-up window the job stops retrying.
#[test]
fn index_evidence_capture_outcall_rejects_invalid_and_gives_up() {
    use std::time::Duration;

    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;
    let evidence_before = cvdr_index_evidence_count(env, lui);
    let (_, base_frozen, _) = cvdr_metrics(env, lui);

    client::identity::happy_path::delete_user(env, &user_auth, canister_ids.identity);
    tick_many(env, 10);

    let (req, _receipt_id, _, _) = find_servable_cvdr_live(env, lui);
    let live = client::http_request(
        env,
        Principal::anonymous(),
        lui,
        &HttpRequest {
            method: "GET".to_string(),
            url: format!("/cvdr_live/{}", hex::encode(_receipt_id)),
            headers: Vec::new(),
            body: Vec::new(),
        },
    );
    assert_eq!(live.status_code, 200);
    env.mock_canister_http_response(MockCanisterHttpResponse {
        subnet_id: req.subnet_id,
        request_id: req.request_id,
        response: CanisterHttpResponse::CanisterHttpReply(CanisterHttpReply {
            status: 200,
            headers: Vec::new(),
            body: live.body,
        }),
        additional_responses: Vec::new(),
    });
    tick_many(env, 5);
    let (_, frozen, _) = cvdr_metrics(env, lui);
    assert_eq!(frozen, base_frozen + 1, "frozen package must exist before INDEX capture");

    // INDEX job: first attempt after ~3s backoff from the give-up anchor.
    let read_state_req = find_index_read_state_outcall(env, lui);
    // Mock a well-formed CBOR read_state response whose certificate is garbage — store-gate must
    // reject (never insert). CBOR: `{ "certificate": h'DEADBEEF' }`
    let reject_body = hex::decode("a16b636572746966696361746544deadbeef").unwrap();
    env.mock_canister_http_response(MockCanisterHttpResponse {
        subnet_id: read_state_req.subnet_id,
        request_id: read_state_req.request_id,
        response: CanisterHttpResponse::CanisterHttpReply(CanisterHttpReply {
            status: 200,
            headers: Vec::new(),
            body: reject_body,
        }),
        additional_responses: Vec::new(),
    });
    tick_many(env, 5);
    assert_eq!(
        cvdr_index_evidence_count(env, lui),
        evidence_before,
        "invalid INDEX certificate must not be stored"
    );

    // Drain any further INDEX outcalls (shared env may have other pending receipts) so
    // PocketIC does not stall on unmocked HTTP while we advance the give-up clock.
    drain_index_read_state_with_reject(env, lui);

    // Past the 24h give-up window: job must stop issuing read_state for this receipt.
    env.advance_time(Duration::from_secs(25 * 60 * 60));
    tick_many(env, 20);
    let pending_read_state: Vec<_> = env
        .get_canister_http()
        .into_iter()
        .filter(|r| r.url.contains("/read_state") && r.url.contains(&lui.to_text()))
        .collect();
    assert!(
        pending_read_state.is_empty(),
        "after 24h give-up, INDEX capture must not keep posting read_state (got {})",
        pending_read_state.len()
    );
    assert_eq!(
        cvdr_index_evidence_count(env, lui),
        evidence_before,
        "give-up leaves INDEX evidence UNAVAILABLE (count unchanged)"
    );
}

/// INDEX capture: HTTP non-200 and a body without `certificate` must not store evidence;
/// the job retries (another `read_state` appears) rather than inserting.
#[test]
fn index_evidence_capture_http_miss_retries_without_store() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;
    let evidence_before = cvdr_index_evidence_count(env, lui);
    let (_, base_frozen, _) = cvdr_metrics(env, lui);

    client::identity::happy_path::delete_user(env, &user_auth, canister_ids.identity);
    tick_many(env, 10);

    let (req, receipt_id, _, _) = find_servable_cvdr_live(env, lui);
    let live = client::http_request(
        env,
        Principal::anonymous(),
        lui,
        &HttpRequest {
            method: "GET".to_string(),
            url: format!("/cvdr_live/{}", hex::encode(receipt_id)),
            headers: Vec::new(),
            body: Vec::new(),
        },
    );
    assert_eq!(live.status_code, 200);
    env.mock_canister_http_response(MockCanisterHttpResponse {
        subnet_id: req.subnet_id,
        request_id: req.request_id,
        response: CanisterHttpResponse::CanisterHttpReply(CanisterHttpReply {
            status: 200,
            headers: Vec::new(),
            body: live.body,
        }),
        additional_responses: Vec::new(),
    });
    tick_many(env, 5);
    assert_eq!(cvdr_metrics(env, lui).1, base_frozen + 1);

    // 1) Non-200 → parse/miss path, no store.
    let r1 = find_index_read_state_outcall(env, lui);
    env.mock_canister_http_response(MockCanisterHttpResponse {
        subnet_id: r1.subnet_id,
        request_id: r1.request_id,
        response: CanisterHttpResponse::CanisterHttpReply(CanisterHttpReply {
            status: 503,
            headers: Vec::new(),
            body: b"unavailable".to_vec(),
        }),
        additional_responses: Vec::new(),
    });
    tick_many(env, 5);
    assert_eq!(cvdr_index_evidence_count(env, lui), evidence_before);

    // 2) 200 but empty certificate blob → miss, no store.
    let r2 = find_index_read_state_outcall(env, lui);
    // CBOR: `{ "certificate": h'' }`
    let empty_cert_body = hex::decode("a16b636572746966696361746540").unwrap();
    env.mock_canister_http_response(MockCanisterHttpResponse {
        subnet_id: r2.subnet_id,
        request_id: r2.request_id,
        response: CanisterHttpResponse::CanisterHttpReply(CanisterHttpReply {
            status: 200,
            headers: Vec::new(),
            body: empty_cert_body,
        }),
        additional_responses: Vec::new(),
    });
    tick_many(env, 5);
    assert_eq!(
        cvdr_index_evidence_count(env, lui),
        evidence_before,
        "empty certificate must not store INDEX evidence"
    );

    // 3) Job retries: another read_state outcall must appear (backoff).
    let _r3 = find_index_read_state_outcall(env, lui);
    drain_index_read_state_with_reject(env, lui);
    assert_eq!(cvdr_index_evidence_count(env, lui), evidence_before);
}

fn find_index_read_state_outcall(
    env: &mut PocketIc,
    local_user_index: CanisterId,
) -> pocket_ic::common::rest::CanisterHttpRequest {
    let lui_text = local_user_index.to_text();
    for _ in 0..40 {
        let candidates: Vec<_> = env
            .get_canister_http()
            .into_iter()
            .filter(|r| r.url.contains("/read_state") && r.url.contains(&lui_text))
            .collect();
        if let Some(req) = candidates.into_iter().next() {
            return req;
        }
        tick_many(env, 2);
        env.advance_time(Duration::from_secs(3));
    }
    panic!("no INDEX /read_state outcall appeared for {lui_text}");
}

fn drain_index_read_state_with_reject(env: &mut PocketIc, local_user_index: CanisterId) {
    let lui_text = local_user_index.to_text();
    let reject_body = hex::decode("a16b636572746966696361746544deadbeef").unwrap();
    for _ in 0..10 {
        let pending: Vec<_> = env
            .get_canister_http()
            .into_iter()
            .filter(|r| r.url.contains("/read_state") && r.url.contains(&lui_text))
            .collect();
        if pending.is_empty() {
            return;
        }
        for req in pending {
            env.mock_canister_http_response(MockCanisterHttpResponse {
                subnet_id: req.subnet_id,
                request_id: req.request_id,
                response: CanisterHttpResponse::CanisterHttpReply(CanisterHttpReply {
                    status: 200,
                    headers: Vec::new(),
                    body: reject_body.clone(),
                }),
                additional_responses: Vec::new(),
            });
        }
        tick_many(env, 3);
    }
}

/// Write the PocketIC end-to-end CVDR fixture for offline CVDR-Verify (V1–V3) to
/// `integration_tests/fixtures/pocketic_e2e_cvdr.json`.
fn export_cvdr_verify_e2e_fixture(env: &PocketIc, lui: CanisterId, receipt_id_hex: &str, live_body: &[u8]) {
    use ic_cbor::CertificateToCbor;
    use ic_certification::{Certificate, LookupResult};

    #[derive(serde::Deserialize)]
    struct Live {
        receipt_body: String,
        witness_cbor: String,
        certificate: Option<String>,
    }
    let live: Live = serde_json::from_slice(live_body).expect("live payload JSON");
    let receipt_body = hex::decode(&live.receipt_body).unwrap();
    let certificate = hex::decode(live.certificate.clone().expect("certificate present in /cvdr_live")).unwrap();

    // Derived exactly as the canister store-gate computed them.
    let receipt_hash = {
        let mut pre = b"OPENCHATZD_RECEIPT_LEAF_V1".to_vec();
        pre.extend_from_slice(&receipt_body);
        sha256::sha256(&pre)
    };
    let cert = Certificate::from_cbor(&certificate).unwrap();
    let tree_root = match cert.tree.lookup_path([b"canister".as_ref(), lui.as_slice(), b"certified_data".as_ref()]) {
        LookupResult::Found(d) => d.to_vec(),
        _ => panic!("no certified_data in fixture cert"),
    };
    let cert_time_ns = match cert.tree.lookup_path([b"time".as_ref()]) {
        LookupResult::Found(t) => {
            let mut r = 0u64;
            let mut s = 0u32;
            for &b in t {
                r |= ((b & 0x7f) as u64) << s;
                if b & 0x80 == 0 {
                    break;
                }
                s += 7;
            }
            r
        }
        _ => panic!("no /time in fixture cert"),
    };
    let root_key = env.root_key().expect("PocketIC NNS root key");

    // Standardized portable frozen-package (spec §4): the SIX package fields at top level (hex) +
    // `version` + optional `root_key_hex`. This is exactly the shape CVDR-Verify's `--package`
    // mode consumes (`FrozenPackage::from_json`); the CLI derives canister_id from the hash-bound
    // body. `root_key_hex` is ignored by the CLI unless `--allow-fixture-root-key` is passed (loud,
    // documented test-only) — a fixture must never silently supply its own trust anchor. The extra
    // human-readable keys (`canister_id`, `receipt_id`, `_comment`) are ignored by the CLI.
    let fixture = serde_json::json!({
        "_comment": "PocketIC end-to-end CVDR fixture for CVDR-Verify V1-V3: a real FrozenCvdrPackage \
                     (genuine PocketIC certificate binding the real receipts-tree root). Verify with \
                     `mktd02-verify --package <this> --allow-fixture-root-key` (PocketIC's NNS root is \
                     not a built-in key). DISTINCT capture from the A1 mainnet fixture (different \
                     subnet + /time); no byte-equality across repos expected. Scratch copy is canonical.",
        "version": 1,
        "encoding": "hex",
        "receipt_body": live.receipt_body,
        "receipt_hash": hex::encode(receipt_hash),
        "tree_root": hex::encode(&tree_root),
        "witness_bytes": live.witness_cbor,
        "certificate_bytes": hex::encode(&certificate),
        "certificate_time": cert_time_ns,
        "root_key_hex": hex::encode(&root_key),
        "canister_id": lui.to_text(),
        "receipt_id": receipt_id_hex,
    });
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("fixtures");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("pocketic_e2e_cvdr.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&fixture).unwrap()).unwrap();
    eprintln!("exported CVDR-Verify e2e fixture -> {}", path.display());
}

/// NEGATIVE: the P2 export / parked-retry path is BANKED, not half-alive. A full v5 deletion
/// must attempt NO receipts-canister `store` c2c (the dedicated canister's stored count never
/// moves) and must never populate the durable parked/export-pending set (so the parked-retry
/// drain has nothing and does not run) — verified as deltas, then re-verified after extra time.
#[test]
#[ignore = "Slice 2/3: asserts released>=1 via finalize. The P2-banked property still holds cert-absent; re-enable/adapt when the store path lands."]
fn p2_export_path_is_banked_not_half_alive() {
    use std::time::Duration;

    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;

    // Baselines (the test env is shared, so assert no NET change from this deletion).
    let receipts_before = receipts_stored(env, canister_ids.receipts);
    let export_pending_before = lui_export_pending(env, lui);

    // Drive a complete v5 deletion.
    delete_and_reach_awaiting(env, canister_ids, &user_auth);
    let pending = pending_certificate(env, lui);
    assert!(matches!(finalize(env, lui, pending.receipt_id, pending.certificate, Vec::new()), finalize_cvdr::Response::Captured));

    // The deletion went entirely through v5 (released CVDR), and the user was uninstalled.
    let (_, released, _) = cvdr_metrics(env, lui);
    assert!(released >= 1, "deletion completed via the v5 CVDR path");
    assert!(module_hash_is_none(env, &user), "user canister uninstalled by the v5 leg");

    // P2 path provably untouched: no store c2c reached the dedicated receipts canister, and the
    // durable parked/export-pending set was never populated.
    assert_eq!(receipts_stored(env, canister_ids.receipts), receipts_before, "v5 leg must attempt NO receipts-canister store c2c");
    assert_eq!(lui_export_pending(env, lui), export_pending_before, "v5 leg must never populate the P2 parked/export-pending set");

    // Let multiple parked-retry intervals worth of time elapse: nothing deferred ever fires.
    env.advance_time(Duration::from_secs(60));
    tick_many(env, 15);
    assert_eq!(receipts_stored(env, canister_ids.receipts), receipts_before, "no deferred/retried export ever fires (parked-retry job absent)");
    assert_eq!(lui_export_pending(env, lui), export_pending_before, "parked/export-pending set stays empty over time");
}

/// Upgrade-survivability: a deletion mid-flight at AwaitingCertificate survives a
/// local_user_index upgrade (the durable draft is stable-backed and the commitment is
/// re-published post-upgrade), then finalizes cleanly.
///
/// Runs on a DEDICATED env (built via `setup_new_env`, never popped from nor returned to the
/// shared pool). The upgrade marks every LUI for a stop/start upgrade via a recurring user_index
/// timer that outlives a test; on a pooled env that timer would later stop a LUI another test is
/// mid-call on (`CanisterStopped`). Isolation removes both the inbound and outbound coupling.
#[test]
#[ignore = "Slice 2/3: the finalize tail. The §4 post_upgrade tree-rebuild + root re-assert has unit coverage; a cert-absent survival assertion can re-enable the front half in Slice 1."]
fn draft_survives_upgrade_then_finalizes() {
    let mut owned_env = crate::setup::setup_new_env(None);
    let TestEnv {
        env,
        canister_ids,
        controller,
    } = &mut owned_env;
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;

    delete_and_reach_awaiting(env, canister_ids, &user_auth);
    let before = pending_certificate(env, lui);
    let (drafts, released, awaiting) = cvdr_metrics(env, lui);
    assert_eq!((drafts, released, awaiting), (1, 0, true), "awaiting before upgrade");

    // Upgrade the local_user_index canister with the draft still in flight. Bump the version so
    // the upgrade ACTUALLY runs (same version is skipped by `should_perform_upgrade`).
    let mut new_wasm = crate::wasms::LOCAL_USER_INDEX.clone();
    new_wasm.version = types::BuildVersion::new(0, 0, 1);
    client::user_index::happy_path::upgrade_local_user_index_canister_wasm(env, *controller, canister_ids.user_index, new_wasm);
    // Deterministically wait for the upgrade to COMPLETE (LUI reports the bumped version, Running).
    wait_for_lui_version(env, lui, types::BuildVersion::new(0, 0, 1));

    // The draft survived; the commitment is re-published so the certificate is available.
    let (drafts, released, awaiting) = cvdr_metrics(env, lui);
    assert_eq!((drafts, released, awaiting), (1, 0, true), "draft survives the upgrade, still awaiting");

    let after = pending_certificate(env, lui);
    assert_eq!(after.receipt_id, before.receipt_id, "same receipt across the upgrade");
    assert!(matches!(finalize(env, lui, after.receipt_id, after.certificate, Vec::new()), finalize_cvdr::Response::Captured));

    let (drafts, released, _) = cvdr_metrics(env, lui);
    assert_eq!((drafts, released), (0, 1), "finalizes cleanly after the upgrade");
    assert!(matches!(fetch_cvdr(env, lui, after.receipt_id), get_cvdr::Response::Success(_)));
}

/// SECURITY (spec §7 rules 3–5, HARD store-gate): a forged (garbage), tampered, or stale
/// certificate submitted to the permissionless backstop must be `Rejected` and must store NOTHING
/// — an untrusted submitter cannot poison the first-wins slot with a bad-signature package. A
/// subsequent valid submission still `Captured`s; the rejected attempts left the receipt intact.
#[test]
fn forged_or_stale_certificate_is_rejected() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;
    let (_, base_r, _) = cvdr_metrics(env, lui);

    delete_and_reach_awaiting(env, canister_ids, &user_auth);
    // The valid backstop snapshot an operator would relay (certificate + matching witness).
    let (receipt_id, certificate, witness) = pending_live_package(env, lui);
    let receipt_hex = hex::encode(receipt_id);

    // (a) Garbage certificate bytes — fails CBOR decode (a valid witness cannot rescue it).
    assert!(matches!(
        finalize(env, lui, receipt_id, vec![0xde, 0xad, 0xbe, 0xef], witness.clone()),
        finalize_cvdr::Response::Rejected(_)
    ));

    // (b) Tampered valid certificate — flip a byte; BLS verification fails.
    let mut tampered = certificate.clone();
    let mid = tampered.len() / 2;
    tampered[mid] ^= 0xFF;
    assert!(matches!(
        finalize(env, lui, receipt_id, tampered, witness.clone()),
        finalize_cvdr::Response::Rejected(_)
    ));

    // No store, no completion: nothing frozen, and the user canister stays uninstalled.
    let (_, released, _) = cvdr_metrics(env, lui);
    assert_eq!(released, base_r, "rejected submissions must not store a frozen package");
    assert!(module_hash_is_none(env, &user), "deletion long completed; rejects change no state");

    // (c) Stale valid certificate — advance past the freshness offset; the captured cert is now
    // too old. The receipt is still AwaitingCertificate (well within the 24h give-up cap).
    env.advance_time(Duration::from_secs(10 * 60));
    assert!(matches!(
        finalize(env, lui, receipt_id, certificate.clone(), witness.clone()),
        finalize_cvdr::Response::Rejected(_)
    ));
    let (_, released, _) = cvdr_metrics(env, lui);
    assert_eq!(released, base_r, "stale cert must not store");

    // A FRESH snapshot (re-read now that time advanced) still finalizes — first valid wins, and the
    // slot survived every rejected attempt.
    let (fresh_cert, fresh_witness) =
        fetch_cvdr_live(env, lui, &receipt_hex).expect("receipt still finalizable; /cvdr_live serves a fresh snapshot");
    assert!(matches!(
        finalize(env, lui, receipt_id, fresh_cert, fresh_witness),
        finalize_cvdr::Response::Captured
    ));
    let (_, released, _) = cvdr_metrics(env, lui);
    assert_eq!(released, base_r + 1, "a fresh valid backstop submission stores exactly one package");

    // Idempotent: a second submission for the finalized receipt is a no-op (rules 6–7).
    let (again_cert, again_witness) = fetch_cvdr_live(env, lui, &receipt_hex).unwrap_or((Vec::new(), Vec::new()));
    assert!(matches!(
        finalize(env, lui, receipt_id, again_cert, again_witness),
        finalize_cvdr::Response::AlreadyFinalized
    ));
    let (_, released, _) = cvdr_metrics(env, lui);
    assert_eq!(released, base_r + 1, "no overwrite — still exactly one package");
}

/// COEXISTENCE (spec §6 self-loop ⇄ §7 backstop, idempotent race): both paths can independently
/// produce a valid package for the same receipt, but the insert-only store means EXACTLY ONE is
/// stored and the other is a no-op — either ordering is correct. Here the backstop lands first;
/// the self-loop's later mocked-outcall continuation then finds the draft already captured and
/// stores nothing (no second frozen package, no error).
#[test]
fn backstop_and_self_loop_store_exactly_one_package() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;
    let (_, base_r, _) = cvdr_metrics(env, lui);

    client::identity::happy_path::delete_user(env, &user_auth, canister_ids.identity);
    tick_many(env, 10);

    // Grab the self-loop's own in-flight outcall (the receipt it is about to finalize) + its payload.
    let (req, receipt_id, certificate, witness) = find_servable_cvdr_live(env, lui);
    let receipt_hex = hex::encode(receipt_id);

    // Backstop wins the slot first.
    assert!(matches!(
        finalize(env, lui, receipt_id, certificate, witness),
        finalize_cvdr::Response::Captured
    ));
    let (_, released_after_backstop, _) = cvdr_metrics(env, lui);
    assert_eq!(released_after_backstop, base_r + 1, "backstop stored exactly one package");

    // Now satisfy the self-loop's in-flight outcall with the SAME live payload. Its continuation
    // re-reads the draft, sees it is no longer AwaitingCertificate, and stores nothing.
    let live = client::http_request(
        env,
        Principal::anonymous(),
        lui,
        &HttpRequest { method: "GET".to_string(), url: format!("/cvdr_live/{receipt_hex}"), headers: Vec::new(), body: Vec::new() },
    );
    env.mock_canister_http_response(MockCanisterHttpResponse {
        subnet_id: req.subnet_id,
        request_id: req.request_id,
        response: CanisterHttpResponse::CanisterHttpReply(CanisterHttpReply { status: 200, headers: Vec::new(), body: live.body }),
        additional_responses: Vec::new(),
    });
    tick_many(env, 5);

    // Exactly one package — the self-loop did NOT store a second (idempotent against the backstop).
    let (_, released_final, _) = cvdr_metrics(env, lui);
    assert_eq!(released_final, base_r + 1, "self-loop is a no-op once the backstop captured; exactly one stored");
}

/// LATE-but-valid (spec §5/§7 rule 8): a cryptographically valid certificate captured OUTSIDE the
/// finalization window is stored by the backstop as `LateFinalized` — a frozen package IS created,
/// but the index never promotes it (the tier stays verifier-derived from the hash-bound fields).
/// Exercises the `FailedStuck` -> backstop remediation path (§7/§10): after the 24h give-up cap the
/// self-loop parks the receipt `FailedStuck`, and an operator can still land it late.
///
/// Runs on a DEDICATED env (like the upgrade tests): this test advances the IC clock by 25h, and a
/// global time-jump on the shared pool would push every other in-flight receipt out of its
/// finalization window — coupling this test into unrelated ones.
#[test]
fn late_valid_certificate_is_stored_as_late_finalized() {
    let mut owned_env = crate::setup::setup_new_env(None);
    let TestEnv { env, canister_ids, .. } = &mut owned_env;
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;
    let (_, base_r, _) = cvdr_metrics(env, lui);

    delete_and_reach_awaiting(env, canister_ids, &user_auth);
    // Discover a servable receipt on this lui while it is still AwaitingCertificate.
    let (_, receipt_id, _, _) = find_servable_cvdr_live(env, lui);
    let receipt_hex = hex::encode(receipt_id);

    // Advance PAST the 24h finalization window / give-up cap. The next self-loop sweep marks the
    // receipt FailedStuck (it never captured an in-window cert). The receipt stays in the tree (§10).
    env.advance_time(Duration::from_secs(25 * 60 * 60));
    tick_many(env, 5);

    // A fresh certificate now has a `/time` ~25h after `receipt_committed_at` — valid, but LATE.
    // `/cvdr_live` still serves the (finalizable) FailedStuck receipt so an operator can fetch it.
    let (certificate, witness) =
        fetch_cvdr_live(env, lui, &receipt_hex).expect("/cvdr_live serves a FailedStuck receipt for backstop remediation");
    assert!(matches!(
        finalize(env, lui, receipt_id, certificate, witness),
        finalize_cvdr::Response::LateFinalized
    ));

    // A frozen package WAS stored (late is stored, not rejected) — exactly one.
    let (_, released, _) = cvdr_metrics(env, lui);
    assert_eq!(released, base_r + 1, "a late-but-valid package is stored (as LateFinalized), not rejected");
}

/// Offline verifier round-trip: take a stored CVDR's bytes and verify V1–V3 with NO live
/// canister — recompute the hash chain (V1), confirm the embedded certificate certifies the
/// commitment (V2), and match the raw module hashes to the deployed release reference (V3).
#[test]
#[ignore = "Slice 2/3: needs a finalized/stored CVDR to read back. Offline V1–V3 recomputation is unchanged; re-enable when the frozen-package store path lands."]
fn offline_verifier_round_trip_from_bytes() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;

    delete_and_reach_awaiting(env, canister_ids, &user_auth);
    let pending = pending_certificate(env, lui);
    assert!(matches!(finalize(env, lui, pending.receipt_id, pending.certificate, Vec::new()), finalize_cvdr::Response::Captured));

    let receipt = match fetch_cvdr(env, lui, pending.receipt_id) {
        get_cvdr::Response::Success(r) => r,
        other => panic!("get_cvdr Success expected, got {other:?}"),
    };

    // ---- V1: recompute the hash chain from the receipt's raw bytes (independent impl) ----
    let h_user_pre = tagged(b"OPENCHATZD_CVDR_H_USER_V1", &[receipt.user_canister_id.as_slice(), &receipt.module_hash_pre]);
    let h_index = tagged(b"OPENCHATZD_CVDR_H_INDEX_V1", &[receipt.index_canister_id.as_slice(), &receipt.executor_module_hash]);
    let seq_be = receipt.deletion_seq.to_be_bytes();
    let commitment = tagged(
        b"OPENCHATZD_CVDR_COMMITMENT_V1",
        &[
            b"OPENCHATZD_CVDR_V1",
            &receipt.record_id,
            &seq_be,
            &h_user_pre,
            &h_index,
            receipt.user_canister_id.as_slice(),
        ],
    );
    assert_eq!(h_user_pre, receipt.h_user_pre, "V1: h_user_pre (target) recomputes from bytes");
    assert_eq!(h_index, receipt.h_index, "V1: h_index (executor) recomputes from bytes");
    assert_eq!(commitment, receipt.commitment, "V1: commitment recomputes from the hash chain");

    // ---- V2: the embedded certificate certifies certified_data == commitment at the index ----
    use ic_cbor::CertificateToCbor;
    use ic_certification::{Certificate, LookupResult};
    let cert = Certificate::from_cbor(&receipt.certificate).expect("embedded certificate parses (V2)");
    let path: [&[u8]; 3] = [b"canister", receipt.index_canister_id.as_slice(), b"certified_data"];
    assert!(
        matches!(cert.tree.lookup_path(path), LookupResult::Found(v) if v == receipt.commitment),
        "V2: embedded certificate certifies the commitment"
    );

    // ---- V3: raw module hashes match the deployed release reference (no live canister) ----
    let user_wasm = std::fs::read(crate::utils::local_bin().join("user.wasm.gz")).expect("read user.wasm.gz");
    let lui_wasm = std::fs::read(crate::utils::local_bin().join("local_user_index.wasm.gz")).expect("read local_user_index.wasm.gz");
    assert_eq!(receipt.module_hash_pre, sha256::sha256(&user_wasm), "V3: target hash == sha256(user.wasm.gz)");
    assert_eq!(receipt.executor_module_hash, sha256::sha256(&lui_wasm), "V3: executor hash == sha256(local_user_index.wasm.gz)");
}

/// Captured executor provenance survives a mid-flight index upgrade. The draft is captured
/// (incl. the executor module hash) BEFORE uninstall; the LUI is then really upgraded
/// (post_upgrade runs); and the finalized receipt carries the CAPTURED executor hash, which
/// `h_index` binds. (The test env ships a single LUI wasm, so pre/post module bytes are equal;
/// the captured-wins-over-live semantics is additionally enforced in code — finalize reads the
/// draft, never live state — and covered by the `h_index_binds_executor_module_hash` unit test.)
#[test]
#[ignore = "Slice 2/3: reads the captured executor hash back from a finalized/stored receipt. Captured-wins semantics has unit coverage (h_index_binds_executor_module_hash)."]
fn captured_executor_hash_survives_mid_flight_upgrade() {
    // Dedicated env (see `draft_survives_upgrade_then_finalizes`) — the upgrade timer must not
    // leak into the shared pool.
    let mut owned_env = crate::setup::setup_new_env(None);
    let TestEnv {
        env,
        canister_ids,
        controller,
    } = &mut owned_env;
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;

    // The executor provenance captured at draft time = sha256 of the currently-deployed LUI wasm.
    let captured_executor = sha256::sha256(&std::fs::read(crate::utils::local_bin().join("local_user_index.wasm.gz")).unwrap());

    delete_and_reach_awaiting(env, canister_ids, &user_auth);
    let before = pending_certificate(env, lui);

    // Really upgrade the LUI mid-flight (version bump forces it; post_upgrade refreshes the live
    // executor hash and re-publishes the pending commitment).
    let mut new_wasm = crate::wasms::LOCAL_USER_INDEX.clone();
    new_wasm.version = types::BuildVersion::new(0, 0, 2);
    client::user_index::happy_path::upgrade_local_user_index_canister_wasm(env, *controller, canister_ids.user_index, new_wasm);
    wait_for_lui_version(env, lui, types::BuildVersion::new(0, 0, 2));

    let after = pending_certificate(env, lui);
    assert_eq!(after.receipt_id, before.receipt_id, "same in-flight deletion across the upgrade");
    assert!(matches!(finalize(env, lui, after.receipt_id, after.certificate, Vec::new()), finalize_cvdr::Response::Captured));

    let receipt = match fetch_cvdr(env, lui, after.receipt_id) {
        get_cvdr::Response::Success(r) => r,
        other => panic!("get_cvdr Success expected, got {other:?}"),
    };
    // The receipt carries the executor hash captured pre-uninstall, and h_index binds it.
    assert_eq!(receipt.executor_module_hash, captured_executor, "receipt records the captured executor hash");
    let h_index = tagged(b"OPENCHATZD_CVDR_H_INDEX_V1", &[receipt.index_canister_id.as_slice(), &receipt.executor_module_hash]);
    assert_eq!(h_index, receipt.h_index, "h_index binds the captured executor hash");
}
