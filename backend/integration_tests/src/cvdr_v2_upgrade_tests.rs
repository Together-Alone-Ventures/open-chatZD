//! Stored-struct upgrade test for the suite-v5 receipt break (Brief B1 R-1 / R-4 / R-2 body):
//! `CvdrDraft` gains `record_salt` and loses `executor_module_hash` / `h_index` / `commitment`;
//! the published body becomes RECEIPT_BODY_V2.
//!
//! A `local_user_index` installed from the Docker-built `c744de1` wasm (V1 drafts) is upgraded to
//! the current wasm:
//! - while a V1 draft is still IN FLIGHT the upgrade must be REFUSED (explicit post_upgrade trap,
//!   old wasm keeps running) — carrying it forward would publish a V2-tagged receipt over a V1,
//!   identifying `record_id`;
//! - once that deletion is finalised under the old wasm, the upgrade must SUCCEED: the retained
//!   terminal V1 draft still decodes, and the stored V1 frozen package is still served.

use crate::client::register_user_and_include_auth;
use crate::cvdr_tests::{
    cvdr_metrics, delete_and_reach_awaiting, fetch_cvdr, finalize, metrics_json, pending_live_package, wait_for_version,
};
use crate::utils::tick_many;
use crate::{TestEnv, client, wasms};
use local_user_index_canister::{finalize_cvdr, get_cvdr};
use std::time::Duration;
use types::{BuildVersion, CanisterId};

/// Reported wasm version, or `(u64::MAX, ..)` while the canister is stopped / mid-upgrade.
fn wasm_version(env: &pocket_ic::PocketIc, canister_id: CanisterId) -> (u64, u64, u64) {
    let request = types::HttpRequest {
        method: "GET".to_string(),
        url: "/metrics".to_string(),
        headers: Vec::new(),
        body: Vec::new(),
    };
    let unavailable = (u64::MAX, u64::MAX, u64::MAX);
    let Ok(bytes) = env.query_call(
        canister_id,
        candid::Principal::anonymous(),
        "http_request",
        candid::encode_one(&request).unwrap(),
    ) else {
        return unavailable;
    };
    let Ok(response) = candid::decode_one::<types::HttpResponse>(&bytes) else {
        return unavailable;
    };
    let Ok(json) = serde_json::from_slice::<serde_json::Value>(&response.body) else {
        return unavailable;
    };
    let v = &json["wasm_version"];
    match (v["major"].as_u64(), v["minor"].as_u64(), v["patch"].as_u64()) {
        (Some(major), Some(minor), Some(patch)) => (major, minor, patch),
        _ => unavailable,
    }
}

/// Answer every pending canister HTTP outcall with a 503. PocketIC never resolves an outcall on its
/// own, and a canister with an open call context cannot STOP — so without this the index would sit
/// in `Stopping` forever and the upgrade under test would never reach `post_upgrade`.
fn drain_http_outcalls(env: &mut pocket_ic::PocketIc) {
    use pocket_ic::common::rest::{CanisterHttpReply, CanisterHttpResponse, MockCanisterHttpResponse};
    for request in env.get_canister_http() {
        env.mock_canister_http_response(MockCanisterHttpResponse {
            subnet_id: request.subnet_id,
            request_id: request.request_id,
            response: CanisterHttpResponse::CanisterHttpReply(CanisterHttpReply {
                status: 503,
                headers: Vec::new(),
                body: Vec::new(),
            }),
            additional_responses: Vec::new(),
        });
    }
}

fn http_text(env: &pocket_ic::PocketIc, canister_id: CanisterId, url: &str) -> String {
    let response = client::http_request(
        env,
        candid::Principal::anonymous(),
        canister_id,
        &types::HttpRequest {
            method: "GET".to_string(),
            url: url.to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        },
    );
    String::from_utf8_lossy(&response.body).into_owned()
}

fn failed_upgrades(env: &pocket_ic::PocketIc, user_index: CanisterId) -> u64 {
    metrics_json(env, user_index)["canister_upgrades_failed"]
        .as_array()
        .expect("canister_upgrades_failed")
        .iter()
        .map(|f| f["count"].as_u64().unwrap_or(1))
        .sum()
}

#[test]
#[ignore = "needs wasms/baseline_c744de1/ (Docker-built at c744de1; docs/dev/v5/BASELINE_c744de1.md §3.2). Run with --ignored."]
fn v1_in_flight_draft_blocks_upgrade_then_terminal_draft_survives_it() {
    let TestEnv {
        mut env,
        canister_ids,
        controller,
    } = crate::setup::setup_env_from_baseline_c744de1();
    let env = &mut env;
    let canister_ids = &canister_ids;

    // A V1 deletion in flight on the OLD index (AwaitingCertificate, V1 body in the certified tree).
    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;
    delete_and_reach_awaiting(env, canister_ids, &user, &user_auth);
    let (drafts, _, awaiting) = cvdr_metrics(env, lui);
    assert_eq!((drafts, awaiting), (1, true), "one V1 draft awaiting its certificate");
    let (pending_receipt_id, _, _) = pending_live_package(env, lui);

    // 1. Upgrade attempt with the V1 draft in flight => refused; the old wasm keeps running.
    let mut v1 = wasms::LOCAL_USER_INDEX.clone();
    v1.version = BuildVersion::new(0, 0, 1);
    client::user_index::happy_path::upgrade_local_user_index_canister_wasm(env, controller, canister_ids.user_index, v1);
    for i in 0..200 {
        drain_http_outcalls(env);
        tick_many(env, 1);
        if i % 4 == 3 {
            env.advance_time(Duration::from_secs(1));
        }
        if failed_upgrades(env, canister_ids.user_index) > 0 {
            break;
        }
    }
    assert!(
        failed_upgrades(env, canister_ids.user_index) > 0,
        "user_index must record the refused local_user_index upgrade"
    );
    for _ in 0..50 {
        if wasm_version(env, lui) == (0, 0, 0) {
            break;
        }
        drain_http_outcalls(env);
        tick_many(env, 1);
    }
    let refusal = http_text(env, canister_ids.user_index, "/errors");
    assert!(
        refusal.contains("upgrade refused") && refusal.contains("pre-V2"),
        "the failure must be the explicit pre-V2 refusal, not an incidental trap: {refusal}"
    );
    let blocking_prefix = &hex::encode(pending_receipt_id)[..8];
    assert!(
        refusal.contains(&format!("{blocking_prefix}(AwaitingCertificate)")) && refusal.contains("Operator path"),
        "refusal must name the blocking receipt prefix + stage and the operator path: {refusal}"
    );
    // The OLD wasm is still serving after the refused upgrade: CVDR routes answer as before.
    assert!(
        matches!(fetch_cvdr(env, lui, pending_receipt_id), get_cvdr::Response::Pending(_)),
        "old wasm still serves /cvdr (Pending) for the in-flight receipt"
    );
    assert_eq!(
        wasm_version(env, lui),
        (0, 0, 0),
        "refused upgrade leaves the old wasm installed"
    );
    let (drafts, _, awaiting) = cvdr_metrics(env, lui);
    assert_eq!((drafts, awaiting), (1, true), "the in-flight V1 deletion is untouched");

    // 2. Finalise that deletion under the wasm that created it (terminal: CertificateCaptured).
    let (receipt_id, certificate, witness) = pending_live_package(env, lui);
    assert!(matches!(
        finalize(env, lui, receipt_id, certificate, witness),
        finalize_cvdr::Response::Captured
    ));
    assert!(matches!(fetch_cvdr(env, lui, receipt_id), get_cvdr::Response::Available(_)));

    // 3. Now the upgrade goes through: the terminal V1 draft decodes (no `record_salt`), the
    //    receipt tree is rebuilt from the stored package, and the V1 package is still served.
    let mut v2 = wasms::LOCAL_USER_INDEX.clone();
    v2.version = BuildVersion::new(0, 0, 2);
    client::user_index::happy_path::upgrade_local_user_index_canister_wasm(env, controller, canister_ids.user_index, v2);
    for i in 0..300 {
        drain_http_outcalls(env);
        tick_many(env, 1);
        if i % 4 == 3 {
            env.advance_time(Duration::from_secs(1));
        }
        if wasm_version(env, lui) == (0, 0, 2) {
            break;
        }
    }
    assert_eq!(
        wasm_version(env, lui),
        (0, 0, 2),
        "upgrade with only a TERMINAL V1 draft must succeed; lui cvdr metrics {:?}; user_index errors: {}",
        cvdr_metrics(env, lui),
        http_text(env, canister_ids.user_index, "/errors")
    );
    assert!(
        matches!(fetch_cvdr(env, lui, receipt_id), get_cvdr::Response::Available(_)),
        "V1 frozen package remains served after the upgrade (historical)"
    );

    // 4. A fresh deletion on the upgraded index publishes a V2 body.
    let (user2, user2_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user2.local_user_index;
    wait_for_version(env, lui, BuildVersion::new(0, 0, 2));
    {
        let prepared = client::local_user_index::happy_path::prepare_account_deletion(env, &user2);
        client::identity::happy_path::delete_user(env, &user2_auth, canister_ids.identity);
        tick_many(env, 10);
        let live = client::http_request(
            env,
            candid::Principal::anonymous(),
            lui,
            &types::HttpRequest {
                method: "GET".to_string(),
                url: format!("/cvdr_live/{}", prepared.receipt_id),
                headers: Vec::new(),
                body: Vec::new(),
            },
        );
        assert_eq!(live.status_code, 200);
        let live: serde_json::Value = serde_json::from_slice(&live.body).unwrap();
        let body = hex::decode(live["receipt_body"].as_str().unwrap()).unwrap();
        assert!(body.starts_with(b"OPENCHATZD_RECEIPT_BODY_V2"));
    }
}
