//! C2 (G ruling; CD B+C finding 1): the index-evidence store-gate decides on the certificate's
//! AUTHENTICATED `/time`, the code epoch and absence of prior evidence — never on the wall clock.
//!
//! Real PocketIC certificates throughout (`instances/<id>/api/v2/canister/<cid>/read_state`).
//! (1) freezes the semantic: the outcall is answered with a certificate minted INSIDE the window,
//! delivered AFTER wall-clock 24 h has passed — it must be STORED (and the upgrade block has already
//! released, as designed). (2) a certificate minted after the window is discarded. (3)/(4) are
//! covered at the gate function (`cvdr::evidence_store_gate` unit test) — an upgrade drains in-flight
//! outcalls under the old epoch, so (3) has no live path, and (4) is first-wins on the store.

use crate::client::register_user_and_include_auth;
use crate::cvdr_tests::{delete_and_reach_awaiting, metrics_json};
use crate::utils::tick_many;
use crate::{TestEnv, client};
use pocket_ic::PocketIc;
use pocket_ic::common::rest::{CanisterHttpReply, CanisterHttpRequest, CanisterHttpResponse, MockCanisterHttpResponse};
use std::time::Duration;
use types::CanisterId;

fn metric(env: &PocketIc, lui: CanisterId, name: &str) -> u64 {
    metrics_json(env, lui)[name]
        .as_u64()
        .unwrap_or_else(|| panic!("metric {name}"))
}

fn reply(env: &mut PocketIc, request: &CanisterHttpRequest, status: u16, body: Vec<u8>) {
    env.mock_canister_http_response(MockCanisterHttpResponse {
        subnet_id: request.subnet_id,
        request_id: request.request_id,
        response: CanisterHttpResponse::CanisterHttpReply(CanisterHttpReply {
            status,
            headers: Vec::new(),
            body,
        }),
        additional_responses: Vec::new(),
    });
}

fn drain_all_503(env: &mut PocketIc) {
    for request in env.get_canister_http() {
        reply(env, &request, 503, Vec::new());
    }
}

/// A pending IC canister-HTTP outcall is dropped by the subnet after ~30 s, so a callback cannot lag
/// its issue by hours (let alone survive an upgrade). Both scenarios therefore issue the outcall
/// 10 s BEFORE the wall-clock boundary and deliver the reply 10 s AFTER it — what decides is the
/// certificate's minting time, not when the callback lands.
const WINDOW: Duration = Duration::from_secs(24 * 60 * 60);
const BEFORE_BOUNDARY: Duration = Duration::from_secs(10);
const PAST_BOUNDARY: Duration = Duration::from_secs(20);

/// Answer every early outcall 503 (the sweep backs off and retries), move the clock to just before
/// the wall-clock boundary, then wait for the sweep's next `/module_hash` outcall and hold it.
fn outcall_just_before_boundary(env: &mut PocketIc, lui: CanisterId) -> CanisterHttpRequest {
    drain_all_503(env);
    tick_many(env, 2);
    drain_all_503(env);
    env.advance_time(WINDOW - BEFORE_BOUNDARY);
    assert_eq!(
        metric(env, lui, "cvdr_upgrade_blockers"),
        1,
        "still inside the wall-clock window"
    );
    for i in 0..40 {
        let pending: Vec<_> = env
            .get_canister_http()
            .into_iter()
            .filter(|r| r.url.contains("/read_state"))
            .collect();
        if let Some(r) = pending.into_iter().next() {
            return r;
        }
        tick_many(env, 1);
        if i % 8 == 7 {
            env.advance_time(Duration::from_secs(1));
        }
    }
    panic!("the Index never issued a read_state outcall for its module_hash");
}

fn mint(env: &PocketIc, request: &CanisterHttpRequest) -> Vec<u8> {
    let instance_url = env
        .get_server_url()
        .join(&format!("instances/{}/", env.instance_id()))
        .unwrap();
    let tail = request.url.split_once("/api/v2/canister/").unwrap().1;
    let response = reqwest::blocking::Client::new()
        .post(instance_url.join(&format!("api/v2/canister/{tail}")).unwrap())
        .header(reqwest::header::CONTENT_TYPE, "application/cbor")
        .body(request.body.clone())
        .send()
        .expect("PocketIC read_state endpoint reachable");
    assert_eq!(response.status().as_u16(), 200);
    response.bytes().unwrap().to_vec()
}

/// (1) STORED: certificate minted INSIDE the window, callback lands AFTER wall-clock 24 h.
#[test]
fn in_window_certificate_delivered_after_wall_clock_24h_is_stored() {
    let mut owned_env = crate::setup::setup_new_env(None);
    let TestEnv { env, canister_ids, .. } = &mut owned_env;
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;
    delete_and_reach_awaiting(env, canister_ids, &user, &user_auth);
    let evidence_before = metric(env, lui, "cvdr_index_evidence_count");

    let request = outcall_just_before_boundary(env, lui);
    // Mint NOW: certified /time ≈ uninstall + 24 h − 10 s (inside the window). Hold the reply.
    let body = mint(env, &request);

    // Wall clock crosses the boundary: the upgrade block releases (as designed) …
    env.advance_time(PAST_BOUNDARY);
    tick_many(env, 2);
    assert_eq!(
        metric(env, lui, "cvdr_upgrade_blockers"),
        0,
        "wall-clock release of the interlock"
    );
    assert_eq!(metric(env, lui, "cvdr_index_evidence_count"), evidence_before);

    // … and only now does the callback land, carrying an in-window certified /time.
    reply(env, &request, 200, body);
    for _ in 0..10 {
        tick_many(env, 1);
        if metric(env, lui, "cvdr_index_evidence_count") > evidence_before {
            break;
        }
        drain_all_503(env);
    }
    assert_eq!(
        metric(env, lui, "cvdr_index_evidence_count"),
        evidence_before + 1,
        "C2 (1): a qualifying certificate is STORED although the callback arrived after wall-clock 24 h"
    );
    assert_eq!(metric(env, lui, "cvdr_index_module_hash_expectation_mismatches"), 0);
}

/// (2) DISCARDED: same outcall, but the certificate is minted AFTER the boundary — its /time is
/// outside the window — even though the outcall was issued inside it.
#[test]
fn certificate_minted_outside_window_is_discarded() {
    let mut owned_env = crate::setup::setup_new_env(None);
    let TestEnv { env, canister_ids, .. } = &mut owned_env;
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;
    delete_and_reach_awaiting(env, canister_ids, &user, &user_auth);
    let evidence_before = metric(env, lui, "cvdr_index_evidence_count");

    let request = outcall_just_before_boundary(env, lui);
    env.advance_time(PAST_BOUNDARY);
    tick_many(env, 2);
    // Minted now: certified /time ≈ uninstall + 24 h + 10 s — outside the window.
    let late_body = mint(env, &request);
    reply(env, &request, 200, late_body);
    for _ in 0..10 {
        tick_many(env, 1);
        drain_all_503(env);
    }
    assert_eq!(
        metric(env, lui, "cvdr_index_evidence_count"),
        evidence_before,
        "C2 (2): a certificate whose /time is after uninstall + 24 h must be discarded, never stored"
    );
    let logs = client::http_request(
        env,
        candid::Principal::anonymous(),
        lui,
        &types::HttpRequest {
            method: "GET".to_string(),
            url: "/logs".to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        },
    );
    let logs = String::from_utf8_lossy(&logs.body);
    assert!(
        logs.contains("index_cert_time_after_window"),
        "discard must be logged with reason index_cert_time_after_window: {logs}"
    );
}
