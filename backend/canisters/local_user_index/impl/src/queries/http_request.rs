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

    // v5 CVDR public fetch route: `/cvdr?receipt_id=<64 hex chars>`. Keyed by the unguessable
    // receipt_id only (the public capability); the record_id path is restricted and NOT served.
    fn get_cvdr_http(qs: HashMap<String, String>, state: &RuntimeState) -> HttpResponse {
        let Some(receipt_id) = qs.get("receipt_id").and_then(|h| hex::decode(h).ok()).and_then(|b| {
            let arr: Result<[u8; 32], _> = b.try_into();
            arr.ok()
        }) else {
            return HttpResponse::not_found();
        };

        match state.data.cvdr.get_by_receipt_id(&receipt_id) {
            Some(cvdr) => build_json_response(&CvdrHttp::from(&cvdr)),
            None => HttpResponse::not_found(),
        }
    }

    // P2 remediation (G ruling (b)): NO per-canister HTTP route for parked-export
    // state. An `http_request` GET arrives as an anonymous query via the IC HTTP
    // gateway (this handler has no authenticated `caller`), so it cannot be
    // operator/controller-gated — and G ruled it must not be public. Per-canister
    // visibility lives in the `error!`/`warn!` logs; the aggregate
    // `receipt_export_pending_count` metric below is the only exposed surface
    // (aggregate, no per-canister status — consistent with the public metrics
    // model).
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

/// Hex-encoded JSON view of a released CVDR for the public `/cvdr` route (hashes + certificate
/// as hex strings rather than raw byte arrays).
#[derive(Serialize)]
struct CvdrHttp {
    encoder_version: String,
    receipt_id: String,
    record_id: String,
    deletion_seq: u64,
    user_canister_id: CanisterId,
    index_canister_id: CanisterId,
    module_hash_pre: String,
    executor_module_hash: String,
    h_user_pre: String,
    h_index: String,
    commitment: String,
    certificate: String,
    created_at: TimestampMillis,
    finalized_at: TimestampMillis,
}

impl From<&crate::model::cvdr::ReleasedCvdr> for CvdrHttp {
    fn from(c: &crate::model::cvdr::ReleasedCvdr) -> Self {
        CvdrHttp {
            encoder_version: c.encoder_version.clone(),
            receipt_id: hex::encode(c.receipt_id),
            record_id: hex::encode(c.record_id),
            deletion_seq: c.deletion_seq,
            user_canister_id: c.user_canister_id,
            index_canister_id: c.index_canister_id,
            module_hash_pre: hex::encode(&c.module_hash_pre),
            executor_module_hash: hex::encode(&c.executor_module_hash),
            h_user_pre: hex::encode(c.h_user_pre),
            h_index: hex::encode(c.h_index),
            commitment: hex::encode(c.commitment),
            certificate: hex::encode(&c.certificate),
            created_at: c.created_at,
            finalized_at: c.finalized_at,
        }
    }
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
