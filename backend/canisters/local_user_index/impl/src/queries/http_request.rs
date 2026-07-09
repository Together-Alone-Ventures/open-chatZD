use crate::{RuntimeState, read_state};
use http_request::{Route, build_json_response, encode_logs, extract_route};
use ic_cdk::query;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;
use types::{BuildVersion, CanisterId, CyclesTopUpHumanReadable, HttpRequest, HttpResponse, TimestampMillis, UserId};

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

    // CVDR public serving route `/cvdr?receipt_id=<hex>` — the frozen-package delivery leg
    // (spec §4 Delivery: bearer-route shape + exposure policy) is a later slice. The legacy
    // ReleasedCvdr store this served is stripped (never written since the finalization rework);
    // until the delivery leg lands, this returns NotFound. Dev-branch interim, same class as the
    // finalize_cvdr stub.
    fn get_cvdr_http(_qs: HashMap<String, String>, _state: &RuntimeState) -> HttpResponse {
        HttpResponse::not_found()
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
        build_json_response(&CvdrLiveHttp {
            receipt_id: hex::encode(receipt_id),
            receipt_body: hex::encode(draft.receipt_body()),
            witness_cbor: hex::encode(state.data.cvdr_receipt_tree.witness_cbor(&receipt_id)),
            certificate: ic_cdk::api::data_certificate().map(hex::encode),
        })
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

    match extract_route(&request.url) {
        Route::Errors(since) => get_errors_impl(since),
        Route::Logs(since) => get_logs_impl(since),
        Route::Traces(since) => get_traces_impl(since),
        Route::Metrics => read_state(get_metrics_impl),
        Route::Other(p, qs) if p == "top_ups" => read_state(|state| get_top_ups(qs, state)),
        Route::Other(p, _) if p == "user_canister_versions" => read_state(get_user_canister_versions),
        Route::Other(p, qs) if p == "remote_user_events" => read_state(|state| get_remote_user_events(qs, state)),
        Route::Other(p, qs) if p == "cvdr" => read_state(|state| get_cvdr_http(qs, state)),
        _ => HttpResponse::not_found(),
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
