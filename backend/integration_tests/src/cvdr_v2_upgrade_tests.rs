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

    // 3. R-2 interlock, incoming-wasm side: the receipt is past its uninstall and the old wasm never
    //    stored index evidence for it (PocketIC cannot mint a system-state certificate), so inside
    //    the 24 h evidence window the upgrade is STILL refused — a new wasm must not become the code
    //    a later certificate speaks for. Once the window lapses the receipt is permanently
    //    V3A-UNAVAILABLE and no longer blocks.
    let mut still_blocked = wasms::LOCAL_USER_INDEX.clone();
    still_blocked.version = BuildVersion::new(0, 0, 3);
    let failed_before = failed_upgrades(env, canister_ids.user_index);
    client::user_index::happy_path::upgrade_local_user_index_canister_wasm(
        env,
        controller,
        canister_ids.user_index,
        still_blocked,
    );
    for i in 0..200 {
        drain_http_outcalls(env);
        tick_many(env, 1);
        if i % 4 == 3 {
            env.advance_time(Duration::from_secs(1));
        }
        if failed_upgrades(env, canister_ids.user_index) > failed_before {
            break;
        }
    }
    let refusal = http_text(env, canister_ids.user_index, "/errors");
    assert!(
        refusal.contains("protected deletion→evidence interval")
            && refusal.contains(&format!("{blocking_prefix}(CertificateCaptured)")),
        "terminal receipt without index evidence must block inside the window: {refusal}"
    );
    for _ in 0..50 {
        if wasm_version(env, lui) == (0, 0, 0) {
            break;
        }
        drain_http_outcalls(env);
        tick_many(env, 1);
    }
    assert_eq!(wasm_version(env, lui), (0, 0, 0));
    env.advance_time(Duration::from_secs(25 * 60 * 60));
    tick_many(env, 3);

    // 4. Now the upgrade goes through: the terminal V1 draft decodes (no `record_salt`), the
    //    receipt tree is rebuilt from the stored package, and the V1 package is still served.
    //    The two refused installs above consumed the subnet's `install_code` instruction budget for
    //    this canister ("rate limited … retry after several minutes"), so let rounds elapse and
    //    retry with a fresh version until the install is admitted.
    let mut upgraded_to = (0, 0, 0);
    for attempt in 0..8u32 {
        for _ in 0..60 {
            drain_http_outcalls(env);
            tick_many(env, 1);
        }
        let mut next = wasms::LOCAL_USER_INDEX.clone();
        next.version = BuildVersion::new(0, 0, 4 + attempt);
        let target = (0, 0, 4 + attempt as u64);
        client::user_index::happy_path::upgrade_local_user_index_canister_wasm(env, controller, canister_ids.user_index, next);
        for i in 0..150 {
            drain_http_outcalls(env);
            tick_many(env, 1);
            if i % 4 == 3 {
                env.advance_time(Duration::from_secs(1));
            }
            if wasm_version(env, lui) == target {
                break;
            }
        }
        if wasm_version(env, lui) == target {
            upgraded_to = target;
            break;
        }
    }
    let errors = http_text(env, canister_ids.user_index, "/errors");
    assert_ne!(
        upgraded_to,
        (0, 0, 0),
        "upgrade with only a TERMINAL V1 draft (window lapsed) must succeed; lui cvdr metrics {:?}; user_index errors: {errors}",
        cvdr_metrics(env, lui),
    );
    assert_eq!(
        errors.matches("upgrade refused").count(),
        2,
        "exactly the two expected refusals (pre-V2, then evidence interlock) — none after the window lapsed: {errors}"
    );
    assert!(
        matches!(fetch_cvdr(env, lui, receipt_id), get_cvdr::Response::Available(_)),
        "V1 frozen package remains served after the upgrade (historical)"
    );

    // 5. A fresh deletion on the upgraded index publishes a V2 body.
    let (user2, user2_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user2.local_user_index;
    wait_for_version(
        env,
        lui,
        BuildVersion::new(upgraded_to.0 as u32, upgraded_to.1 as u32, upgraded_to.2 as u32),
    );
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

/// R-2 upgrade interlock on the CURRENT wasm (authoritative `pre_upgrade` side): while a receipt is
/// inside `uninstall → index evidence stored`, the Index cannot be upgraded — the refusal names the
/// receipt, and the OLD wasm keeps serving. Once the 24 h evidence window lapses the same upgrade
/// succeeds and the receipt is still served. (The "evidence stored => unblocked" leg is
/// `stored_index_evidence_unblocks_upgrade_and_draft_finalises` below.)
#[test]
fn receipt_without_index_evidence_blocks_upgrade_until_window_lapses() {
    let mut owned_env = crate::setup::setup_new_env(None);
    let TestEnv {
        env,
        canister_ids,
        controller,
    } = &mut owned_env;
    let controller = *controller;

    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;
    delete_and_reach_awaiting(env, canister_ids, &user, &user_auth);
    let (receipt_id, _, _) = pending_live_package(env, lui);
    let prefix = &hex::encode(receipt_id)[..8];
    assert_eq!(metrics_json(env, lui)["cvdr_upgrade_blockers"].as_u64(), Some(1));

    // 1. Upgrade attempt inside the protected interval => refused by the RUNNING wasm.
    let mut next = wasms::LOCAL_USER_INDEX.clone();
    next.version = BuildVersion::new(0, 0, 1);
    client::user_index::happy_path::upgrade_local_user_index_canister_wasm(env, controller, canister_ids.user_index, next);
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
    assert!(failed_upgrades(env, canister_ids.user_index) > 0, "upgrade must be refused");
    let refusal = http_text(env, canister_ids.user_index, "/errors");
    assert!(
        refusal.contains("upgrade refused")
            && refusal.contains("protected deletion→evidence interval")
            && refusal.contains(&format!("{prefix}(AwaitingCertificate)"))
            && refusal.contains("Operator path"),
        "refusal must be the interlock's, naming the receipt and the operator path: {refusal}"
    );

    // 2. The OLD wasm is still installed AND still serving after the refused upgrade.
    for _ in 0..50 {
        if wasm_version(env, lui) == (0, 0, 0) {
            break;
        }
        drain_http_outcalls(env);
        tick_many(env, 1);
    }
    assert_eq!(wasm_version(env, lui), (0, 0, 0), "old wasm still installed");
    assert!(
        matches!(fetch_cvdr(env, lui, receipt_id), get_cvdr::Response::Pending(_)),
        "old wasm still serves /cvdr for the in-flight receipt"
    );
    assert!(
        http_text(env, lui, &format!("/cvdr_live/{}", hex::encode(receipt_id))).contains("receipt_body"),
        "old wasm still serves /cvdr_live (certified receipt tree intact)"
    );
    let (drafts, _, awaiting) = cvdr_metrics(env, lui);
    assert_eq!((drafts, awaiting), (1, true), "the in-flight deletion is untouched");
    let newcomer = crate::client::register_user(env, canister_ids);
    let other = crate::client::register_user(env, canister_ids);
    client::user::happy_path::send_text_message(env, &newcomer, other.user_id, "still serving", None);

    // 3. Evidence window lapses (receipt stays V3A-UNAVAILABLE) => the same upgrade now succeeds.
    drain_http_outcalls(env);
    env.advance_time(Duration::from_secs(25 * 60 * 60));
    for _ in 0..5 {
        drain_http_outcalls(env);
        tick_many(env, 1);
    }
    assert_eq!(metrics_json(env, lui)["cvdr_upgrade_blockers"].as_u64(), Some(0));
    let mut later = wasms::LOCAL_USER_INDEX.clone();
    later.version = BuildVersion::new(0, 0, 2);
    client::user_index::happy_path::upgrade_local_user_index_canister_wasm(env, controller, canister_ids.user_index, later);
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
        "upgrade after the window lapsed must succeed; user_index errors: {}",
        http_text(env, canister_ids.user_index, "/errors")
    );
    assert!(
        http_text(env, lui, &format!("/cvdr_live/{}", hex::encode(receipt_id))).contains("receipt_body"),
        "receipt tree rebuilt: the receipt is still servable after the upgrade"
    );
}

/// Answer the Index's pending `read_state` outcalls with a REAL certificate: replay the canister's
/// exact request body against this PocketIC instance's own IC HTTP interface
/// (`instances/<id>/api/v2/canister/<cid>/read_state`) and hand the response back as the outcall
/// reply. Every other pending outcall gets a 503 (see [`drain_http_outcalls`]). Returns how many
/// `read_state` outcalls were answered with a 200 from the instance.
fn proxy_read_state_outcalls(env: &mut pocket_ic::PocketIc) -> usize {
    use pocket_ic::common::rest::{CanisterHttpReply, CanisterHttpResponse, MockCanisterHttpResponse};
    let instance_url = env
        .get_server_url()
        .join(&format!("instances/{}/", env.instance_id()))
        .unwrap();
    let client = reqwest::blocking::Client::new();
    let mut proxied = 0;
    for request in env.get_canister_http() {
        let reply = match request.url.split_once("/api/v2/canister/") {
            Some((_, tail)) if tail.ends_with("/read_state") => {
                let endpoint = format!("api/v2/canister/{tail}");
                let response = client
                    .post(instance_url.join(&endpoint).unwrap())
                    .header(reqwest::header::CONTENT_TYPE, "application/cbor")
                    .body(request.body.clone())
                    .send()
                    .expect("PocketIC read_state endpoint reachable");
                let status = response.status().as_u16();
                if status == 200 {
                    proxied += 1;
                }
                CanisterHttpReply {
                    status,
                    headers: Vec::new(),
                    body: response.bytes().unwrap().to_vec(),
                }
            }
            _ => CanisterHttpReply {
                status: 503,
                headers: Vec::new(),
                body: Vec::new(),
            },
        };
        env.mock_canister_http_response(MockCanisterHttpResponse {
            subnet_id: request.subnet_id,
            request_id: request.request_id,
            response: CanisterHttpResponse::CanisterHttpReply(reply),
            additional_responses: Vec::new(),
        });
    }
    proxied
}

/// R-2 POSITIVE path, end to end: the Index fetches a genuine subnet-signed
/// `/canister/<index>/module_hash` certificate (served by this PocketIC instance), verifies it at
/// the store-gate, stores certificate + extracted hash — and with evidence stored the SAME upgrade
/// that is refused without it goes through while the deletion is still awaiting its commitment
/// certificate. The draft then finalises on the upgraded Index and is served with its evidence.
#[test]
fn stored_index_evidence_unblocks_upgrade_and_draft_finalises() {
    let mut owned_env = crate::setup::setup_new_env(None);
    let TestEnv {
        env,
        canister_ids,
        controller,
    } = &mut owned_env;
    let controller = *controller;

    let (user, user_auth) = register_user_and_include_auth(env, canister_ids);
    let lui = user.local_user_index;
    delete_and_reach_awaiting(env, canister_ids, &user, &user_auth);
    let (receipt_id, _, _) = pending_live_package(env, lui);
    let evidence_count = |env: &pocket_ic::PocketIc| metrics_json(env, lui)["cvdr_index_evidence_count"].as_u64().unwrap();
    let blockers = |env: &pocket_ic::PocketIc| metrics_json(env, lui)["cvdr_upgrade_blockers"].as_u64().unwrap();
    let evidence_before = evidence_count(env);
    assert_eq!(blockers(env), 1, "inside the protected interval before evidence is stored");

    // 1. Let the evidence sweep run against a real certificate.
    let mut proxied = 0;
    for i in 0..120 {
        proxied += proxy_read_state_outcalls(env);
        tick_many(env, 1);
        if i % 2 == 1 {
            env.advance_time(Duration::from_secs(1));
        }
        if evidence_count(env) > evidence_before {
            break;
        }
    }
    assert!(
        proxied > 0,
        "the Index must have issued a read_state outcall for its own module_hash"
    );
    assert_eq!(
        evidence_count(env),
        evidence_before + 1,
        "a genuine certificate must pass the store-gate and be stored ({proxied} proxied)"
    );
    assert_eq!(blockers(env), 0, "evidence stored => the receipt no longer blocks upgrades");
    assert_eq!(
        metrics_json(env, lui)["cvdr_index_module_hash_expectation_mismatches"].as_u64(),
        Some(0),
        "certified module hash equals the deploy-supplied expectation"
    );

    // 2. The upgrade goes through with the deletion still awaiting its commitment certificate.
    let (drafts, _, awaiting) = cvdr_metrics(env, lui);
    assert_eq!((drafts, awaiting), (1, true));
    let mut next = wasms::LOCAL_USER_INDEX.clone();
    next.version = BuildVersion::new(0, 0, 1);
    client::user_index::happy_path::upgrade_local_user_index_canister_wasm(env, controller, canister_ids.user_index, next);
    for i in 0..300 {
        drain_http_outcalls(env);
        tick_many(env, 1);
        if i % 4 == 3 {
            env.advance_time(Duration::from_secs(1));
        }
        if wasm_version(env, lui) == (0, 0, 1) {
            break;
        }
    }
    assert_eq!(
        wasm_version(env, lui),
        (0, 0, 1),
        "upgrade must succeed once evidence is stored; user_index errors: {}",
        http_text(env, canister_ids.user_index, "/errors")
    );
    assert_eq!(failed_upgrades(env, canister_ids.user_index), 0, "no refusal on this path");
    assert_eq!(
        evidence_count(env),
        evidence_before + 1,
        "stored evidence survives the upgrade"
    );

    // 3. The draft finalises on the upgraded Index and is served WITH its index evidence.
    let (live_id, certificate, witness) = pending_live_package(env, lui);
    assert_eq!(live_id, receipt_id, "same receipt across the upgrade");
    assert!(matches!(
        finalize(env, lui, receipt_id, certificate, witness),
        finalize_cvdr::Response::Captured
    ));
    let get_cvdr::Response::Available(get_cvdr::AvailablePackage::PortablePackageV3(v3)) = fetch_cvdr(env, lui, receipt_id)
    else {
        panic!("finalised receipt must be served as PortablePackageV3 with its index evidence");
    };
    // R-6: version 3, trust root stamped from Index config (PocketIC root != IC NNS key), the
    // extracted hash equals the Index's live module hash, and Gate B holds — candid and HTTP bodies
    // are byte-identical, with the exact FrozenWire bytes nested.
    assert_eq!(v3.version, 3);
    assert_eq!(v3.trust_root_key_id, get_cvdr::TRUST_ROOT_NON_PRODUCTION);
    let live_module_hash = env
        .canister_status(lui, Some(canister_ids.user_index))
        .unwrap()
        .module_hash
        .unwrap();
    assert_eq!(
        v3.index_code_identity_evidence.index_module_hash, live_module_hash,
        "extracted index_module_hash is the certified module hash of the Index"
    );
    assert!(!v3.index_code_identity_evidence.certificate_bytes.is_empty());
    let http = http_text(env, lui, &format!("/cvdr/{}", hex::encode(receipt_id)));
    assert_eq!(
        http.as_bytes(),
        v3.to_canonical_json().as_slice(),
        "Gate B: HTTP /cvdr body == candid PortablePackageV3 canonical JSON"
    );
    // Export the EXACT served bytes + PocketIC NNS root (out of band: a V3 package carries no root
    // material) as the CVDR-Verify end-to-end fixture for the OpenChat V3 path.
    let dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("fixtures");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pocketic_e2e_portable_v3.json"), http.as_bytes()).unwrap();
    std::fs::write(
        dir.join("pocketic_e2e_portable_v3.root_key.hex"),
        hex::encode(env.root_key().expect("PocketIC NNS root key")),
    )
    .unwrap();
    std::fs::write(
        dir.join("pocketic_e2e_portable_v3.PROVENANCE.txt"),
        format!(
            "PocketIC end-to-end PortablePackageV3 for CVDR-Verify (OpenChatZD suite-v5 V1/V2/V3A).\n\
             Exact bytes of GET /cvdr/{} served by local_user_index {} in\n\
             cvdr_v2_upgrade_tests::stored_index_evidence_unblocks_upgrade_and_draft_finalises.\n\
             Genuine PocketIC certificates (commitment + /module_hash); trust_root_key_id = {}.\n\
             Verify: mktd02-verify --package pocketic_e2e_portable_v3.json --allow-fixture-root-key \\\n\
               --fixture-root-key-hex $(cat pocketic_e2e_portable_v3.root_key.hex)   => validity: PASS\n",
            hex::encode(receipt_id),
            lui,
            get_cvdr::TRUST_ROOT_NON_PRODUCTION
        ),
    )
    .unwrap();
    let http_json: serde_json::Value = serde_json::from_str(&http).unwrap();
    assert_eq!(http_json["version"], 3);
    assert_eq!(http_json["trust_root_key_id"], get_cvdr::TRUST_ROOT_NON_PRODUCTION);
    let nested = hex::decode(http_json["frozen"].as_str().unwrap()).unwrap();
    let frozen: serde_json::Value = serde_json::from_slice(&nested).unwrap();
    assert_eq!(frozen["schema"], "openchatzd.cvdr.frozen_package");
    assert!(
        hex::decode(frozen["receipt_body"].as_str().unwrap())
            .unwrap()
            .starts_with(b"OPENCHATZD_RECEIPT_BODY_V2")
    );
}
