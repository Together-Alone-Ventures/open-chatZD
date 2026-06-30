//! v5 CVDR-on-Index PocketIC suite.
//!
//! Exercises the local_user_index delete leg end-to-end against a live replica:
//!   - the forward-only delete job (capture -> uninstall -> publish certified commitment),
//!   - the external-finalizer relay (`cvdr_data_certificate` query -> `finalize_cvdr` update),
//!   - the released-CVDR store/fetch (`get_cvdr` query + `/cvdr` http route),
//!   - the single certified-data slot guard (concurrent deletes serialize),
//!   - upgrade-survivability (a draft mid-flight survives a local_user_index upgrade).
//!
//! The certificate is produced by PocketIC itself (it certifies `certified_data` after a
//! tick, exactly as on mainnet); `finalize_cvdr` binds it structurally to the pending
//! commitment. This mirrors the user-canister MKTd finalize flow proven in
//! `mktd_deletion_tests`, but on the index.

use crate::client::register_user_and_include_auth;
use crate::env::ENV;
use crate::utils::tick_many;
use crate::{CanisterIds, TestEnv, User, UserAuth, client};
use candid::Principal;
use local_user_index_canister::{cvdr_data_certificate, finalize_cvdr, get_cvdr};
use pocket_ic::PocketIc;
use std::ops::Deref;
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

/// Advance time + ticks until a pending certificate appears (optionally one whose receipt
/// differs from `exclude`). The job processes one deletion per fire and re-arms on the retry
/// interval, so each round advances past it. Panics with the current metrics if none appears.
fn await_pending(env: &mut PocketIc, local_user_index: CanisterId, exclude: Option<[u8; 32]>) -> cvdr_data_certificate::SuccessResult {
    use std::time::Duration;
    for _ in 0..20 {
        tick_many(env, 3);
        if let cvdr_data_certificate::Response::Success(p) =
            client::local_user_index::cvdr_data_certificate(env, Principal::anonymous(), local_user_index, &Empty {})
        {
            if exclude != Some(p.receipt_id) && !p.certificate.is_empty() {
                return p;
            }
        }
        env.advance_time(Duration::from_secs(35));
    }
    let (drafts, released, awaiting) = cvdr_metrics(env, local_user_index);
    panic!("no pending certificate (excluding {exclude:?}) appeared; drafts={drafts} released={released} awaiting={awaiting}");
}

fn finalize(env: &mut PocketIc, local_user_index: CanisterId, receipt_id: [u8; 32], certificate: Vec<u8>) -> finalize_cvdr::Response {
    client::local_user_index::finalize_cvdr(
        env,
        Principal::anonymous(),
        local_user_index,
        &finalize_cvdr::Args { receipt_id, certificate },
    )
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

/// Finalize-leg happy path: delete -> AwaitingCertificate -> finalize -> completion. The user
/// canister is uninstalled by the leg, the commitment is certified, and finalize stores the
/// releasable CVDR.
#[test]
fn finalize_leg_happy_path() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;

    // Baseline: the env pool reuses envs across tests, so assert DELTAS, not absolute counts.
    let (base_d, base_r, _) = cvdr_metrics(env, lui);

    delete_and_reach_awaiting(env, canister_ids, &user_auth);

    // Uninstalled before the certificate is captured; a draft is awaiting, nothing newly released.
    assert!(module_hash_is_none(env, &user), "user canister must be uninstalled by the leg");
    let (drafts, released, awaiting) = cvdr_metrics(env, lui);
    assert_eq!((drafts, released, awaiting), (base_d + 1, base_r, true), "one draft awaiting, none newly released");

    let pending = pending_certificate(env, lui);
    let response = finalize(env, lui, pending.receipt_id, pending.certificate);
    assert!(matches!(response, finalize_cvdr::Response::Success), "finalize expected Success, got {response:?}");

    let (drafts, released, awaiting) = cvdr_metrics(env, lui);
    assert_eq!((drafts, released, awaiting), (base_d, base_r + 1, false), "draft cleared, one newly released, slot freed");
}

/// Store/fetch round-trip: after finalize the released CVDR is fetchable by `receipt_id` via
/// both the query and the `/cvdr` http route, and the fetch is byte-stable (deterministic).
#[test]
fn cvdr_store_fetch_round_trip() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;

    delete_and_reach_awaiting(env, canister_ids, &user_auth);
    let pending = pending_certificate(env, lui);
    assert!(matches!(finalize(env, lui, pending.receipt_id, pending.certificate), finalize_cvdr::Response::Success));

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
    assert!(matches!(finalize(env, lui, pending.receipt_id, pending.certificate), finalize_cvdr::Response::Success));
    let (drafts, released, awaiting) = cvdr_metrics(env, lui);
    assert_eq!((drafts, released, awaiting), (base_d, base_r + 1, false), "late finalize completes it");
}

/// Single-slot guard: two concurrent deletions must serialize on the one certified-data slot.
/// The slot is structurally single (`Option<receipt_id>`), so at most one deletion is
/// AwaitingCertificate at any moment; a second receipt only becomes available AFTER the first
/// is finalized and frees the slot. Both end up released with distinct receipts.
#[test]
fn concurrent_deletes_serialize_on_single_slot() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let ((user_a, auth_a), (user_b, auth_b)) = two_users_same_lui(env, canister_ids);
    assert_eq!(user_a.local_user_index, user_b.local_user_index, "test requires a shared local_user_index");
    let lui = user_b.local_user_index;
    let (base_d, base_r, _) = cvdr_metrics(env, lui);

    // Both users share a local_user_index. Delete both.
    client::identity::happy_path::delete_user(env, &auth_a, canister_ids.identity);
    client::identity::happy_path::delete_user(env, &auth_b, canister_ids.identity);

    // Phase 1: exactly one deletion reaches the certified-data slot.
    let first = await_pending(env, lui, None);

    // While the first holds the slot, the second cannot publish: the only pending certificate
    // is still the first's (the single-slot guard serialises them).
    match client::local_user_index::cvdr_data_certificate(env, Principal::anonymous(), lui, &Empty {}) {
        cvdr_data_certificate::Response::Success(p) => {
            assert_eq!(p.receipt_id, first.receipt_id, "only the slot-holder's certificate is available while it holds the slot")
        }
        other => panic!("expected the first receipt still pending, got {other:?}"),
    }

    // Finalize the first; this frees the slot.
    assert!(matches!(finalize(env, lui, first.receipt_id, first.certificate), finalize_cvdr::Response::Success));

    // Phase 2: the second deletion now claims the freed slot, with a distinct receipt.
    let second = await_pending(env, lui, Some(first.receipt_id));
    assert_ne!(second.receipt_id, first.receipt_id, "the second deletion has a distinct receipt");
    assert!(matches!(finalize(env, lui, second.receipt_id, second.certificate), finalize_cvdr::Response::Success));

    let (drafts, released, awaiting) = cvdr_metrics(env, lui);
    assert_eq!((drafts, released, awaiting), (base_d, base_r + 2, false), "both serialized deletions released; slot free");
    assert!(matches!(fetch_cvdr(env, lui, first.receipt_id), get_cvdr::Response::Success(_)));
    assert!(matches!(fetch_cvdr(env, lui, second.receipt_id), get_cvdr::Response::Success(_)));
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

    // Drive a complete v5 deletion.
    delete_and_reach_awaiting(env, canister_ids, &user_auth);
    let pending = pending_certificate(env, lui);
    assert!(matches!(finalize(env, lui, pending.receipt_id, pending.certificate), finalize_cvdr::Response::Success));

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
    assert!(matches!(finalize(env, lui, after.receipt_id, after.certificate), finalize_cvdr::Response::Success));

    let (drafts, released, _) = cvdr_metrics(env, lui);
    assert_eq!((drafts, released), (0, 1), "finalizes cleanly after the upgrade");
    assert!(matches!(fetch_cvdr(env, lui, after.receipt_id), get_cvdr::Response::Success(_)));
}

/// SECURITY: a forged (garbage), tampered, or stale certificate must be rejected with
/// `CertificateMismatch` and must NOT store a CVDR or run `complete_deletion`. A subsequent
/// valid certificate still finalizes — bad attempts leave the slot intact.
#[test]
fn forged_or_stale_certificate_is_rejected() {
    use std::time::Duration;

    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;
    let (base_d, base_r, _) = cvdr_metrics(env, lui);

    delete_and_reach_awaiting(env, canister_ids, &user_auth);
    let pending = pending_certificate(env, lui);

    // (a) Garbage bytes — fails CBOR parse.
    assert!(matches!(
        finalize(env, lui, pending.receipt_id, vec![0xde, 0xad, 0xbe, 0xef]),
        finalize_cvdr::Response::CertificateMismatch
    ));

    // (b) Tampered valid cert — flip a byte; CBOR/BLS verification fails.
    let mut tampered = pending.certificate.clone();
    let mid = tampered.len() / 2;
    tampered[mid] ^= 0xFF;
    assert!(matches!(
        finalize(env, lui, pending.receipt_id, tampered),
        finalize_cvdr::Response::CertificateMismatch
    ));

    // No store, no completion: still pending, slot held, nothing released, still uninstalled.
    let (drafts, released, awaiting) = cvdr_metrics(env, lui);
    assert_eq!((drafts, released, awaiting), (base_d + 1, base_r, true), "rejected certs must not store or complete");
    assert!(matches!(fetch_cvdr(env, lui, pending.receipt_id), get_cvdr::Response::NotFound));
    assert!(module_hash_is_none(env, &user), "deletion not completed (no rollback either)");

    // (c) Stale valid cert — advance past the max offset; the captured cert is now too old.
    env.advance_time(Duration::from_secs(10 * 60));
    assert!(matches!(
        finalize(env, lui, pending.receipt_id, pending.certificate.clone()),
        finalize_cvdr::Response::CertificateMismatch
    ));
    assert!(matches!(fetch_cvdr(env, lui, pending.receipt_id), get_cvdr::Response::NotFound), "stale cert must not store");

    // A fresh, valid certificate still finalizes — the slot survived the rejected attempts.
    let fresh = pending_certificate(env, lui);
    assert!(matches!(finalize(env, lui, fresh.receipt_id, fresh.certificate), finalize_cvdr::Response::Success));
    assert!(matches!(fetch_cvdr(env, lui, fresh.receipt_id), get_cvdr::Response::Success(_)));
}

/// Offline verifier round-trip: take a stored CVDR's bytes and verify V1–V3 with NO live
/// canister — recompute the hash chain (V1), confirm the embedded certificate certifies the
/// commitment (V2), and match the raw module hashes to the deployed release reference (V3).
#[test]
fn offline_verifier_round_trip_from_bytes() {
    let mut wrapper = ENV.deref().get();
    let TestEnv { env, canister_ids, .. } = wrapper.env();
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;

    delete_and_reach_awaiting(env, canister_ids, &user_auth);
    let pending = pending_certificate(env, lui);
    assert!(matches!(finalize(env, lui, pending.receipt_id, pending.certificate), finalize_cvdr::Response::Success));

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
    assert!(matches!(finalize(env, lui, after.receipt_id, after.certificate), finalize_cvdr::Response::Success));

    let receipt = match fetch_cvdr(env, lui, after.receipt_id) {
        get_cvdr::Response::Success(r) => r,
        other => panic!("get_cvdr Success expected, got {other:?}"),
    };
    // The receipt carries the executor hash captured pre-uninstall, and h_index binds it.
    assert_eq!(receipt.executor_module_hash, captured_executor, "receipt records the captured executor hash");
    let h_index = tagged(b"OPENCHATZD_CVDR_H_INDEX_V1", &[receipt.index_canister_id.as_slice(), &receipt.executor_module_hash]);
    assert_eq!(h_index, receipt.h_index, "h_index binds the captured executor hash");
}
