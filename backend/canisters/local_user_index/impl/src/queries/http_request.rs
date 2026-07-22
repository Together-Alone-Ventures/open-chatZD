use crate::{RuntimeState, read_state};
use http_request::{Route, build_json_response, encode_logs, extract_route};
use ic_cdk::query;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;
use types::{
    BuildVersion, CanisterId, CyclesTopUpHumanReadable, HeaderField, HttpRequest, HttpResponse, TimestampMillis, UserId,
};

#[query]
fn http_request(request: HttpRequest) -> HttpResponse {
    fn get_errors_impl(since: Option<TimestampMillis>) -> HttpResponse {
        encode_logs(canister_logger::export_errors(), since.unwrap_or(0))
    }

    fn get_logs_impl(since: Option<TimestampMillis>) -> HttpResponse {
        encode_logs(canister_logger::export_logs(), since.unwrap_or(0))
    }

    fn get_traces_impl(since: Option<TimestampMillis>) -> HttpResponse {
        encode_logs(canister_logger::export_traces(), since.unwrap_or(0))
    }

    fn get_metrics_impl(state: &RuntimeState) -> HttpResponse {
        build_json_response(&state.metrics())
    }

    fn get_top_ups(qs: HashMap<String, String>, state: &RuntimeState) -> HttpResponse {
        let user_id: UserId = CanisterId::from_text(qs.get("canister_id").unwrap()).unwrap().into();

        let Some(user) = state.data.local_users.get(&user_id) else {
            return HttpResponse::not_found();
        };

        let total = user.cycle_top_ups.iter().map(|c| c.amount).sum::<u128>() as f64 / 1_000_000_000_000f64;

        build_json_response(&TopUps {
            total,
            top_ups: user.cycle_top_ups.iter().map(|c| c.into()).collect(),
        })
    }

    fn get_user_canister_versions(state: &RuntimeState) -> HttpResponse {
        let mut map = BTreeMap::new();
        for (user_id, user) in state.data.local_users.iter() {
            let version = map.entry(user.wasm_version).or_insert(UserCanisterVersion {
                version: user.wasm_version,
                count: 0,
                users: Vec::new(),
            });
            version.count += 1;
            if version.users.len() < 100 {
                version.users.push(*user_id);
            }
        }
        let vec: Vec<_> = map.values().collect();
        build_json_response(&vec)
    }

    fn get_remote_user_events(qs: HashMap<String, String>, state: &RuntimeState) -> HttpResponse {
        let skip = qs.get("skip").and_then(|v| usize::from_str(v).ok()).unwrap_or_default();

        build_json_response(
            &state
                .data
                .events_for_remote_users
                .iter()
                .skip(skip)
                .take(1000)
                .collect::<Vec<_>>(),
        )
    }

    // CVDR public serving route (spec §11.1/§11.2): `GET /cvdr/<receipt_id>` on the raw domain.
    // PATH FORM ONLY — the query form `/cvdr?receipt_id=...` is NOT served (§11.1) and falls
    // through to the catch-all 404 below.
    //
    // Four states, all constant-shape, none echoing the supplied id:
    //   Available -> 200 + the stored FrozenWire package, byte-for-byte (§11.3)
    //   Pending   -> 202 + status body (§11.2; 202 deliverability proven end-to-end)
    //   Unknown   -> 404 + constant-shape body
    //   malformed -> 400 + constant-shape body
    fn get_cvdr_http(receipt_id_hex: &str, state: &RuntimeState) -> HttpResponse {
        let Some(receipt_id) = parse_receipt_id(receipt_id_hex) else {
            return cvdr_json_response(400, CVDR_BAD_REQUEST_BODY.as_bytes().to_vec());
        };

        if let Some(package) = state.data.cvdr.get_frozen_package(&receipt_id) {
            // The SAME constructor and the SAME serializer the candid surface uses (§11.3).
            let wire = local_user_index_canister::get_cvdr::FrozenWire::from(&package);
            cvdr_json_response(200, wire.to_canonical_json())
        } else if state.data.cvdr.find_any_draft_by_receipt_id(&receipt_id).is_some() {
            let pending = local_user_index_canister::get_cvdr::PendingInfo::default();
            cvdr_json_response(202, serde_json::to_vec(&pending).expect("PendingInfo serialization"))
        } else {
            cvdr_json_response(404, CVDR_NOT_FOUND_BODY.as_bytes().to_vec())
        }
    }

    // Self-finalization live route (spec §6): GET /cvdr_live/<receipt_id hex>. Raw-domain QUERY;
    // returns {receipt_body, witness, data_certificate()} for the canister's own non-replicated
    // outcall loop. Distinct from the delivery-leg /cvdr serving route. `data_certificate()` is
    // Some() only in a (non-replicated) query context — the property A1 proved on mainnet.
    fn get_cvdr_live(receipt_id_hex: &str, state: &RuntimeState) -> HttpResponse {
        let Some(receipt_id) = hex::decode(receipt_id_hex).ok().and_then(|b| <[u8; 32]>::try_from(b).ok()) else {
            return HttpResponse::not_found();
        };
        let Some(draft) = state.data.cvdr.find_draft_by_receipt_id(&receipt_id) else {
            return HttpResponse::not_found();
        };
        // `no-store` too: `/cvdr_live` is also a bearer-URL surface (§11.6). Its BODY is
        // unchanged — the §6 self-loop parses it byte-for-byte.
        let body = serde_json::to_vec(&CvdrLiveHttp {
            receipt_id: hex::encode(receipt_id),
            receipt_body: hex::encode(draft.receipt_body()),
            witness_cbor: hex::encode(state.data.cvdr_receipt_tree.witness_cbor(&receipt_id)),
            certificate: ic_cdk::api::data_certificate().map(hex::encode),
        })
        .expect("CvdrLiveHttp serialization");
        cvdr_json_response(200, body)
    }

    // P2 remediation (G ruling (b)): NO per-canister HTTP route for parked-export
    // state. An `http_request` GET arrives as an anonymous query via the IC HTTP
    // gateway (this handler has no authenticated `caller`), so it cannot be
    // operator/controller-gated — and G ruled it must not be public. Per-canister
    // visibility lives in the `error!`/`warn!` logs; the aggregate
    // `receipt_export_pending_count` metric below is the only exposed surface
    // (aggregate, no per-canister status — consistent with the public metrics
    // model).
    // `/cvdr_live/<receipt_id>` is a path-segment route (spec §6), handled before the
    // query-string router.
    if let Some(hex_id) = request.url.split('?').next().and_then(|p| p.strip_prefix("/cvdr_live/")) {
        return read_state(|state| get_cvdr_live(hex_id, state));
    }

    // `/cvdr/<receipt_id>` is a path-segment route (spec §11.1), handled before the query-string
    // router. It cannot shadow `/cvdr_live/` — that path starts `/cvdr_` and never `/cvdr/`.
    if let Some(hex_id) = request.url.split('?').next().and_then(|p| p.strip_prefix("/cvdr/")) {
        return read_state(|state| get_cvdr_http(hex_id, state));
    }

    match extract_route(&request.url) {
        Route::Errors(since) => get_errors_impl(since),
        Route::Logs(since) => get_logs_impl(since),
        Route::Traces(since) => get_traces_impl(since),
        Route::Metrics => read_state(get_metrics_impl),
        Route::Other(p, qs) if p == "top_ups" => read_state(|state| get_top_ups(qs, state)),
        Route::Other(p, _) if p == "user_canister_versions" => read_state(get_user_canister_versions),
        Route::Other(p, qs) if p == "remote_user_events" => read_state(|state| get_remote_user_events(qs, state)),
        _ => HttpResponse::not_found(),
    }
}

/// §11.2 constant-shape error bodies. They echo no detail: no id reflection, no reason strings.
/// `400` and `404` are necessarily distinguishable by status code (§11.7-3); the distinct
/// `status` strings are still constant-shape.
const CVDR_NOT_FOUND_BODY: &str = r#"{"schema":"openchatzd.cvdr.status","version":1,"status":"not_found"}"#;
const CVDR_BAD_REQUEST_BODY: &str = r#"{"schema":"openchatzd.cvdr.status","version":1,"status":"bad_request"}"#;

/// Strict `receipt_id` form (spec §11.1): exactly 64 LOWERCASE hex chars. Uppercase hex is
/// rejected so the bearer capability has one canonical spelling. Anything else is `400`.
fn parse_receipt_id(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 || !s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return None;
    }
    hex::decode(s).ok().and_then(|b| <[u8; 32]>::try_from(b).ok())
}

/// Every `/cvdr` response carries `Cache-Control: no-store` (§11.6 — bearer-URL content must not
/// be cached by intermediaries) plus `X-Content-Type-Options: nosniff`. Both are preserved
/// verbatim through the IC HTTP gateway on the raw domain.
fn cvdr_json_response(status_code: u16, body: Vec<u8>) -> HttpResponse {
    HttpResponse {
        status_code,
        headers: vec![
            HeaderField("Content-Type".to_string(), "application/json".to_string()),
            HeaderField("Content-Length".to_string(), body.len().to_string()),
            HeaderField("Cache-Control".to_string(), "no-store".to_string()),
            HeaderField("X-Content-Type-Options".to_string(), "nosniff".to_string()),
        ],
        body,
        streaming_strategy: None,
        upgrade: None,
    }
}

/// Hex-encoded payload of the `/cvdr_live/<receipt_id>` self-finalization route (spec §6):
/// the RECEIPT_BODY_V1 bytes, the IC HashTree CBOR witness, and the IC `data_certificate()`
/// (absent only outside a query context). The canister's own non-replicated outcall parses this
/// and runs the store-gate.
#[derive(Serialize)]
struct CvdrLiveHttp {
    receipt_id: String,
    receipt_body: String,
    witness_cbor: String,
    certificate: Option<String>,
}

#[derive(Serialize)]
struct UserCanisterVersion {
    version: BuildVersion,
    count: u32,
    users: Vec<UserId>,
}

#[derive(Serialize)]
struct TopUps {
    total: f64,
    top_ups: Vec<CyclesTopUpHumanReadable>,
}
