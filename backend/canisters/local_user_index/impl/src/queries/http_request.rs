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

    // CVDR public serving (spec §11.1/§11.2): `GET /cvdr/<receipt_id>` PATH FORM ONLY.
    // Query form `/cvdr?receipt_id=...` is NOT served.
    // Available FrozenWire | Available PortablePackageV2 | Pending 202 | Unknown 404 | malformed 400.
    fn get_cvdr_http(receipt_id_hex: &str, state: &RuntimeState) -> HttpResponse {
        let Some(receipt_id) = parse_receipt_id(receipt_id_hex) else {
            return cvdr_json_response(400, CVDR_BAD_REQUEST_BODY.as_bytes().to_vec());
        };

        match crate::queries::get_cvdr::get_cvdr_impl(local_user_index_canister::get_cvdr::Args { receipt_id }, state) {
            local_user_index_canister::get_cvdr::Response::Available(pkg) => cvdr_json_response(200, pkg.to_canonical_json()),
            local_user_index_canister::get_cvdr::Response::Pending(pending) => {
                cvdr_json_response(202, serde_json::to_vec(&pending).expect("PendingInfo serialization"))
            }
            local_user_index_canister::get_cvdr::Response::NotFound => {
                cvdr_json_response(404, CVDR_NOT_FOUND_BODY.as_bytes().to_vec())
            }
        }
    }

    // Self-finalization live route (spec §6): GET /cvdr_live/<receipt_id hex>.
    fn get_cvdr_live(receipt_id_hex: &str, state: &RuntimeState) -> HttpResponse {
        let Some(receipt_id) = hex::decode(receipt_id_hex).ok().and_then(|b| <[u8; 32]>::try_from(b).ok()) else {
            return HttpResponse::not_found();
        };
        let Some(draft) = state.data.cvdr.find_draft_by_receipt_id(&receipt_id) else {
            return HttpResponse::not_found();
        };
        let body = serde_json::to_vec(&CvdrLiveHttp {
            receipt_id: hex::encode(receipt_id),
            receipt_body: hex::encode(draft.receipt_body()),
            witness_cbor: hex::encode(state.data.cvdr_receipt_tree.witness_cbor(&receipt_id)),
            certificate: ic_cdk::api::data_certificate().map(hex::encode),
        })
        .expect("CvdrLiveHttp serialization");
        cvdr_json_response(200, body)
    }

    if let Some(hex_id) = request.url.split('?').next().and_then(|p| p.strip_prefix("/cvdr_live/")) {
        return read_state(|state| get_cvdr_live(hex_id, state));
    }

    // `/cvdr/<receipt_id>` before the query-string router. Cannot shadow `/cvdr_live/`.
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

const CVDR_NOT_FOUND_BODY: &str = r#"{"schema":"openchatzd.cvdr.status","version":1,"status":"not_found"}"#;
const CVDR_BAD_REQUEST_BODY: &str = r#"{"schema":"openchatzd.cvdr.status","version":1,"status":"bad_request"}"#;

/// Strict receipt_id form (spec §11.1): exactly 64 lowercase hex chars.
fn parse_receipt_id(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 || !s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return None;
    }
    hex::decode(s).ok().and_then(|b| <[u8; 32]>::try_from(b).ok())
}

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
