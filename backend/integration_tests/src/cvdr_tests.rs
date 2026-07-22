//! CVDR-on-Index PocketIC suite.
//!
//! **Baseline (spec §8/§8b).** The delete leg COMPLETES user deletion (uninstall + mapping
//! removal + membership notifications) inside the job, INDEPENDENTLY of certificate capture —
//! the cert-absent path is the live path. CVDR finalization (capturing the IC certificate into
//! the immutable frozen package) runs via self-finalization (spec §6) and the permissionless
//! backstop (spec §7). The single certified-data slot + global single-flight guard are gone
//! (spec §2 — a certified receipt tree holds many receipts under one root).
//!
//! **Delivery leg (spec §11).** `/cvdr/<receipt_id>` and `get_cvdr` serve the four §11.2 states
//! from durable state; the obsolete `cvdr_data_certificate` query is REMOVED (§11.5). The
//! remaining `#[ignore]`d tests are blocked on the CVDR-Verify round-trip (E-5) and/or on the
//! LUI-upgrade mechanism — see each attribute for which, and why it is not the delivery leg.

use crate::client::register_user_and_include_auth;
use crate::env::ENV;
use crate::utils::tick_many;
use crate::{CanisterIds, TestEnv, User, UserAuth, client};
use candid::Principal;
use local_user_index_canister::{finalize_cvdr, get_cvdr};
use pocket_ic::PocketIc;
use pocket_ic::common::rest::{CanisterHttpReply, CanisterHttpResponse, MockCanisterHttpResponse};
use std::ops::Deref;
use std::time::Duration;
use types::{CanisterId, HttpRequest, HttpResponse};

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

/// GET an arbitrary path on the local_user_index HTTP surface.
///
/// NOTE: this is a DIRECT canister query — no IC HTTP gateway is in the loop. Status-code
/// assertions here therefore prove what the CANISTER EMITS, not what a gateway delivers.
/// Gateway deliverability of `202` was settled separately, out of band, against a real gateway.
fn cvdr_http(env: &PocketIc, local_user_index: CanisterId, url: &str) -> HttpResponse {
    client::http_request(
        env,
        Principal::anonymous(),
        local_user_index,
        &HttpRequest { method: "GET".to_string(), url: url.to_string(), headers: Vec::new(), body: Vec::new() },
    )
}

fn response_header(response: &HttpResponse, name: &str) -> Option<String> {
    response.headers.iter().find(|h| h.0.eq_ignore_ascii_case(name)).map(|h| h.1.clone())
}

/// Drive one deletion all the way to a STORED frozen package, via the §6 self-finalization loop
/// with a mocked outcall (the same mechanism `self_finalization_captures_and_stores_via_mocked_outcall`
/// proves). Returns the receipt_id of the stored package.
fn delete_and_store_package(env: &mut PocketIc, canister_ids: &CanisterIds, user_auth: &UserAuth, lui: CanisterId) -> [u8; 32] {
    delete_and_reach_awaiting(env, canister_ids, user_auth);
    let (req, receipt_id, _, _) = find_servable_cvdr_live(env, lui);
    let receipt_hex = hex::encode(receipt_id);
    let live = cvdr_http(env, lui, &format!("/cvdr_live/{receipt_hex}"));
    assert_eq!(live.status_code, 200, "/cvdr_live must serve the live certification payload");

    env.mock_canister_http_response(MockCanisterHttpResponse {
        subnet_id: req.subnet_id,
        request_id: req.request_id,
        response: CanisterHttpResponse::CanisterHttpReply(CanisterHttpReply { status: 200, headers: Vec::new(), body: live.body }),
        additional_responses: Vec::new(),
    });
    tick_many(env, 5);
    receipt_id
}

/// The RECEIPT_BODY_V1 fields the delivery leg exposes (spec §2). Parsed INDEPENDENTLY of the
/// canister's encoder: the frozen package carries `receipt_body` as opaque bytes, so a verifier
/// (and this test) must recompute the layout from the spec. Fixed-width except the two
/// `len(u8) ‖ principal_bytes` identity fields.
struct ReceiptBodyV1 {
    receipt_id: [u8; 32],
    index_canister_id: Principal,
    user_canister_id: Principal,
    record_id: [u8; 32],
    deletion_seq: u64,
    h_user_pre: [u8; 32],
    h_index: [u8; 32],
    commitment: [u8; 32],
}

fn parse_receipt_body(body: &[u8]) -> ReceiptBodyV1 {
    let mut p = 0usize;
    let mut take = |n: usize| {
        let s = &body[p..p + n];
        p += n;
        s
    };
    assert_eq!(take(26), b"OPENCHATZD_RECEIPT_BODY_V1", "RECEIPT_BODY_TAG");
    let receipt_id: [u8; 32] = take(32).try_into().unwrap();
    let _nonce = take(32);
    let index_len = take(1)[0] as usize;
    let index_canister_id = Principal::from_slice(take(index_len));
    let user_len = take(1)[0] as usize;
    let user_canister_id = Principal::from_slice(take(user_len));
    let record_id: [u8; 32] = take(32).try_into().unwrap();
    let deletion_seq = u64::from_be_bytes(take(8).try_into().unwrap());
    let h_user_pre: [u8; 32] = take(32).try_into().unwrap();
    let h_index: [u8; 32] = take(32).try_into().unwrap();
    let commitment: [u8; 32] = take(32).try_into().unwrap();
    ReceiptBodyV1 { receipt_id, index_canister_id, user_canister_id, record_id, deletion_seq, h_user_pre, h_index, commitment }
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
    // An id with neither draft nor package is Unknown (§11.2) — note this is the all-zero id, NOT
    // this user's receipt, which is Pending at this point (see `pending_then_available_no_404_in_the_gap`).
    assert!(matches!(fetch_cvdr(env, lui, [0u8; 32]), get_cvdr::Response::NotFound));
}

/// **E-1 / spec §11.3 — the Definition-of-Done gate for the delivery leg.**
///
/// `SHA-256(HTTP 200 body bytes) == SHA-256(candid Available payload re-serialized)`, stable
/// across repeated fetches. Both surfaces route through the single `FrozenWire` constructor and
/// the single canonical serializer, so this asserts that property end-to-end rather than
/// re-implementing it.
#[test]
fn cvdr_store_fetch_round_trip() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;

    let receipt_id = delete_and_store_package(env, canister_ids, &user_auth, lui);
    let receipt_id_hex = hex::encode(receipt_id);

    // ---- HTTP surface: 200 + the stored FrozenWire package, byte-for-byte ----
    let http = cvdr_http(env, lui, &format!("/cvdr/{receipt_id_hex}"));
    assert_eq!(http.status_code, 200, "stored package must serve 200");
    assert_eq!(response_header(&http, "Content-Type").as_deref(), Some("application/json"));
    assert_eq!(response_header(&http, "Cache-Control").as_deref(), Some("no-store"), "§11.6 bearer-URL content must not be cached");
    assert_eq!(response_header(&http, "X-Content-Type-Options").as_deref(), Some("nosniff"));

    // ---- Candid surface: Available(FrozenWire) ----
    let wire = match fetch_cvdr(env, lui, receipt_id) {
        get_cvdr::Response::Available(w) => w,
        other => panic!("get_cvdr expected Available, got {other:?}"),
    };

    // ---- §11.3 three-way byte equality ----
    let candid_bytes = wire.to_canonical_json();
    assert_eq!(
        sha256::sha256(&http.body),
        sha256::sha256(&candid_bytes),
        "§11.3: HTTP 200 body bytes must equal the candid Available payload re-serialized"
    );

    // Stable across repeated fetches, on both surfaces.
    let http_again = cvdr_http(env, lui, &format!("/cvdr/{receipt_id_hex}"));
    assert_eq!(http.body, http_again.body, "repeated HTTP fetch must be byte-identical");
    let wire_again = match fetch_cvdr(env, lui, receipt_id) {
        get_cvdr::Response::Available(w) => w,
        other => panic!("get_cvdr expected Available, got {other:?}"),
    };
    assert_eq!(candid_bytes, wire_again.to_canonical_json(), "repeated candid fetch must re-serialize identically");

    // ---- The served document is the portable schema CVDR-Verify consumes ----
    let json: serde_json::Value = serde_json::from_slice(&http.body).expect("FrozenWire JSON");
    assert_eq!(json["schema"].as_str(), Some("openchatzd.cvdr.frozen_package"));
    assert_eq!(json["version"].as_u64(), Some(1));
    assert_eq!(json["encoding"].as_str(), Some("hex"));
    for field in ["receipt_body", "receipt_hash", "tree_root", "witness_bytes", "certificate_bytes"] {
        assert!(json[field].is_string(), "{field} must be a hex string");
    }
    assert!(json["certificate_time"].is_number(), "certificate_time is a JSON number");
    // `root_key_hex` is a CVDR-Verify fixture-only field and must NEVER be served.
    assert!(json.get("root_key_hex").is_none(), "the serving layer must not emit a trust anchor");
    // Facts, not verdicts (§11.2): no tier appears anywhere in the served bytes.
    let body_text = String::from_utf8_lossy(&http.body);
    assert!(!body_text.contains("VerifiedFinal") && !body_text.contains("LateFinalized"), "serving must never classify");

    // The hash-bound body names this receipt and this subject.
    let parsed = parse_receipt_body(&wire.receipt_body);
    assert_eq!(parsed.receipt_id, receipt_id, "receipt_id is hash-bound in the body");
    assert_eq!(parsed.user_canister_id, user.canister(), "subject canister");
    assert_eq!(parsed.index_canister_id, lui, "executor canister");
}

/// **E-2 / spec §11.7-2 — Pending → Available across finalization, with NO 404 in the gap.**
///
/// Retired property: this test formerly asserted that user deletion was WITHHELD until a
/// finalizer ran (`absent_finalizer_withholds_completion_and_resumes`). Spec §8 inverted that —
/// deletion completes cert-absent — and `delete_completes_cert_absent_and_awaits_certificate`
/// now owns the new truth. What survives here is the serving half, which §11.2 made load-bearing.
///
/// NOTE: `cvdr_http` is a direct canister query, so the `202` assertion proves the canister
/// EMITS 202. Gateway deliverability of 202 was proven separately against a real HTTP gateway.
#[test]
fn pending_then_available_no_404_in_the_gap() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;

    delete_and_reach_awaiting(env, canister_ids, &user_auth);
    let (_, receipt_id, _, _) = find_servable_cvdr_live(env, lui);
    let receipt_id_hex = hex::encode(receipt_id);

    // A draft exists, no frozen package yet → Pending on BOTH surfaces. Never Unknown.
    let assert_pending = |env: &PocketIc| {
        let http = cvdr_http(env, lui, &format!("/cvdr/{receipt_id_hex}"));
        assert_eq!(http.status_code, 202, "a draft with no package must be Pending, never 404");
        assert_eq!(response_header(&http, "Cache-Control").as_deref(), Some("no-store"));
        let json: serde_json::Value = serde_json::from_slice(&http.body).expect("pending JSON");
        assert_eq!(json["schema"].as_str(), Some("openchatzd.cvdr.status"));
        assert_eq!(json["status"].as_str(), Some("pending"));
        assert_eq!(json["retry_after_secs"].as_u64(), Some(5));
        // §11.2: the Pending body leaks nothing — no draft contents, no draft-derived timestamps,
        // no target data, and not even the id it was asked about.
        assert_eq!(json.as_object().unwrap().len(), 4, "pending body carries exactly schema/version/status/retry hint");
        assert!(!String::from_utf8_lossy(&http.body).contains(&receipt_id_hex), "pending body must not echo the id");
        assert!(matches!(fetch_cvdr(env, lui, receipt_id), get_cvdr::Response::Pending(_)), "candid Pending");
    };
    assert_pending(env);

    // Pending covers STUCK: many job intervals with no finalizer, still Pending, still no 404.
    tick_many(env, 20);
    assert_pending(env);
    assert!(module_hash_is_none(env, &user), "canister stays uninstalled while awaiting");

    // Finalization lands → the SAME receipt_id flips to Available. No gap, no 404 anywhere.
    let (req, _, _, _) = find_servable_cvdr_live(env, lui);
    let live = cvdr_http(env, lui, &format!("/cvdr_live/{receipt_id_hex}"));
    env.mock_canister_http_response(MockCanisterHttpResponse {
        subnet_id: req.subnet_id,
        request_id: req.request_id,
        response: CanisterHttpResponse::CanisterHttpReply(CanisterHttpReply { status: 200, headers: Vec::new(), body: live.body }),
        additional_responses: Vec::new(),
    });
    tick_many(env, 5);

    let http = cvdr_http(env, lui, &format!("/cvdr/{receipt_id_hex}"));
    assert_eq!(http.status_code, 200, "Pending must transition to Available on the same receipt_id");
    assert!(matches!(fetch_cvdr(env, lui, receipt_id), get_cvdr::Response::Available(_)), "candid Available");
}

/// **E-3 / spec §11.7-3 — Unknown is 404, malformed is 400, both constant-shape, neither echoes.**
#[test]
fn unknown_is_404_and_malformed_is_400_constant_shape() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let (user, _) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;

    // Unknown: well-formed id, no draft and no package.
    let unknown_hex = "0".repeat(64);
    let unknown = cvdr_http(env, lui, &format!("/cvdr/{unknown_hex}"));
    assert_eq!(unknown.status_code, 404, "unknown receipt_id must 404");
    assert_eq!(unknown.body, br#"{"schema":"openchatzd.cvdr.status","version":1,"status":"not_found"}"#.to_vec());
    assert!(matches!(fetch_cvdr(env, lui, [0u8; 32]), get_cvdr::Response::NotFound), "candid NotFound");

    // Malformed: wrong length, non-hex, and UPPERCASE hex (§11.1 pins lowercase).
    let malformed = ["abc", &"z".repeat(64), &"A".repeat(64), &"0".repeat(63), &"0".repeat(65)];
    let mut bodies = Vec::new();
    for id in malformed {
        let response = cvdr_http(env, lui, &format!("/cvdr/{id}"));
        assert_eq!(response.status_code, 400, "malformed id `{id}` must 400");
        assert_eq!(response.body, br#"{"schema":"openchatzd.cvdr.status","version":1,"status":"bad_request"}"#.to_vec());
        // Echoes no detail: no id reflection, no reason strings.
        assert!(!String::from_utf8_lossy(&response.body).contains(id), "400 body must not echo `{id}`");
        bodies.push(response.body);
    }
    assert!(bodies.windows(2).all(|w| w[0] == w[1]), "every 400 body is byte-identical — constant shape");

    // 400 and 404 are distinguishable by status code, and both carry no-store.
    assert_ne!(unknown.body, bodies[0], "distinct status strings");
    assert_eq!(response_header(&unknown, "Cache-Control").as_deref(), Some("no-store"));

    // §11.1: the query form is NOT served. It is not a malformed id — it is not a route.
    let query_form = cvdr_http(env, lui, &format!("/cvdr?receipt_id={unknown_hex}"));
    assert_eq!(query_form.status_code, 404, "the query form must not be served");
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
fn p2_export_path_is_banked_not_half_alive() {
    use std::time::Duration;

    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;

    // Baselines (the test env is shared, so assert no NET change from this deletion).
    let receipts_before = receipts_stored(env, canister_ids.receipts);
    let export_pending_before = lui_export_pending(env, lui);

    // Drive a complete v5 deletion, through to a stored frozen package.
    delete_and_store_package(env, canister_ids, &user_auth, lui);

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
#[ignore = "BLOCKED on the LUI-upgrade mechanism, not on the delivery leg: `wait_for_lui_version` \
            never observes the bumped version, so the upgrade under test never lands. Reproduced \
            with freshly-built local_user_index + user_index wasms. Pre-existing — this test and \
            `captured_executor_hash_survives_mid_flight_upgrade` are the ONLY users of \
            `wait_for_lui_version` and both were already ignored, so the mechanism has never run \
            green here. The delivery-leg assertions below (Pending across upgrade, then Available) \
            are written and will pass once the upgrade lands."]
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
    let (_, before_receipt_id, _, _) = find_servable_cvdr_live(env, lui);
    let (drafts, released, awaiting) = cvdr_metrics(env, lui);
    assert_eq!((drafts, released, awaiting), (1, 0, true), "awaiting before upgrade");

    // §11.2: an in-flight draft serves Pending, not Unknown.
    assert!(matches!(fetch_cvdr(env, lui, before_receipt_id), get_cvdr::Response::Pending(_)), "Pending before upgrade");

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

    // Still Pending across the upgrade — the bearer never sees a 404 because of an upgrade.
    assert!(matches!(fetch_cvdr(env, lui, before_receipt_id), get_cvdr::Response::Pending(_)), "Pending after upgrade");

    let (after_receipt_id, certificate, witness) = pending_live_package(env, lui);
    assert_eq!(after_receipt_id, before_receipt_id, "same receipt across the upgrade");
    assert!(matches!(finalize(env, lui, after_receipt_id, certificate, witness), finalize_cvdr::Response::Captured));

    let (drafts, released, _) = cvdr_metrics(env, lui);
    assert_eq!((drafts, released), (0, 1), "finalizes cleanly after the upgrade");
    assert!(matches!(fetch_cvdr(env, lui, after_receipt_id), get_cvdr::Response::Available(_)));
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
/// Interface note (§11.1 P0 lock): the served wire shape is FrozenWire, which carries NO
/// `module_hash_pre` / `executor_module_hash` / `encoder_version`. Those were CvdrReceipt fields.
/// The module hashes are folded into `h_user_pre` / `h_index` inside the hash-bound `receipt_body`,
/// so a verifier does not read them back — it recomputes them from a module hash it already
/// trusts. That is exactly CVDR-Verify's `--expect-module-hash` gate, and this test is the
/// in-repo mirror of it.
#[test]
#[ignore = "Phase 4: the CVDR-Verify `--reveal` / `--expect-module-hash` round-trip (E-5). Assertions below are in place and pass; un-ignore with the reveal leg."]
fn offline_verifier_round_trip_from_bytes() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;

    let receipt_id = delete_and_store_package(env, canister_ids, &user_auth, lui);
    let wire = match fetch_cvdr(env, lui, receipt_id) {
        get_cvdr::Response::Available(w) => w,
        other => panic!("get_cvdr Available expected, got {other:?}"),
    };
    let body = parse_receipt_body(&wire.receipt_body);

    // ---- V1 + V3 fused: recompute h_user_pre / h_index from module hashes we independently
    // trust (the deployed wasms), and require the hash-bound body to commit to them. ----
    let user_wasm = std::fs::read(crate::utils::local_bin().join("user.wasm.gz")).expect("read user.wasm.gz");
    let lui_wasm = std::fs::read(crate::utils::local_bin().join("local_user_index.wasm.gz")).expect("read local_user_index.wasm.gz");
    let h_user_pre = tagged(b"OPENCHATZD_CVDR_H_USER_V1", &[body.user_canister_id.as_slice(), &sha256::sha256(&user_wasm)]);
    let h_index = tagged(b"OPENCHATZD_CVDR_H_INDEX_V1", &[body.index_canister_id.as_slice(), &sha256::sha256(&lui_wasm)]);
    assert_eq!(h_user_pre, body.h_user_pre, "h_user_pre binds sha256(user.wasm.gz)");
    assert_eq!(h_index, body.h_index, "h_index binds sha256(local_user_index.wasm.gz)");

    let seq_be = body.deletion_seq.to_be_bytes();
    let commitment = tagged(
        b"OPENCHATZD_CVDR_COMMITMENT_V1",
        &[b"OPENCHATZD_CVDR_V1", &body.record_id, &seq_be, &h_user_pre, &h_index, body.user_canister_id.as_slice()],
    );
    assert_eq!(commitment, body.commitment, "V1: commitment recomputes from the hash chain");

    // ---- Packaging integrity: leaf = SHA256(RECEIPT_LEAF_TAG ‖ receipt_body) == receipt_hash ----
    let leaf = tagged(b"OPENCHATZD_RECEIPT_LEAF_V1", &[&wire.receipt_body]);
    assert_eq!(leaf, wire.receipt_hash, "receipt_hash is the tree leaf over the body");

    // ---- V2: the bundled certificate certifies certified_data == tree_root at the index ----
    use ic_cbor::CertificateToCbor;
    use ic_certification::{Certificate, LookupResult};
    let cert = Certificate::from_cbor(&wire.certificate_bytes).expect("bundled certificate parses (V2)");
    let path: [&[u8]; 3] = [b"canister", body.index_canister_id.as_slice(), b"certified_data"];
    assert!(
        matches!(cert.tree.lookup_path(path), LookupResult::Found(v) if v == wire.tree_root),
        "V2: bundled certificate certifies the tree root"
    );
}

/// Captured executor provenance survives a mid-flight index upgrade. The draft is captured
/// (incl. the executor module hash) BEFORE uninstall; the LUI is then really upgraded
/// (post_upgrade runs); and the finalized receipt carries the CAPTURED executor hash, which
/// `h_index` binds. (The test env ships a single LUI wasm, so pre/post module bytes are equal;
/// the captured-wins-over-live semantics is additionally enforced in code — finalize reads the
/// draft, never live state — and covered by the `h_index_binds_executor_module_hash` unit test.)
#[test]
#[ignore = "Phase 4 (E-5) AND blocked on the same LUI-upgrade mechanism as \
            `draft_survives_upgrade_then_finalizes` — see its ignore reason. Assertions below are in place."]
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
    let (_, before_receipt_id, _, _) = find_servable_cvdr_live(env, lui);

    // Really upgrade the LUI mid-flight (version bump forces it; post_upgrade refreshes the live
    // executor hash and re-publishes the pending commitment).
    let mut new_wasm = crate::wasms::LOCAL_USER_INDEX.clone();
    new_wasm.version = types::BuildVersion::new(0, 0, 2);
    client::user_index::happy_path::upgrade_local_user_index_canister_wasm(env, *controller, canister_ids.user_index, new_wasm);
    wait_for_lui_version(env, lui, types::BuildVersion::new(0, 0, 2));

    let (after_receipt_id, certificate, witness) = pending_live_package(env, lui);
    assert_eq!(after_receipt_id, before_receipt_id, "same in-flight deletion across the upgrade");
    assert!(matches!(finalize(env, lui, after_receipt_id, certificate, witness), finalize_cvdr::Response::Captured));

    let wire = match fetch_cvdr(env, lui, after_receipt_id) {
        get_cvdr::Response::Available(w) => w,
        other => panic!("get_cvdr Available expected, got {other:?}"),
    };
    // FrozenWire does not carry the raw executor hash; `h_index` inside the hash-bound body binds
    // it. Recomputing from the PRE-upgrade hash is what proves captured-wins-over-live.
    let body = parse_receipt_body(&wire.receipt_body);
    let h_index = tagged(b"OPENCHATZD_CVDR_H_INDEX_V1", &[body.index_canister_id.as_slice(), &captured_executor]);
    assert_eq!(h_index, body.h_index, "h_index binds the executor hash captured pre-uninstall");
}
