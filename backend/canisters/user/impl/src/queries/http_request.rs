use crate::model::streak::Streak;
use crate::{RuntimeState, read_state};
use http_request::{AvatarRoute, Route, build_json_response, encode_logs, extract_route, get_document};
use ic_cdk::query;
use itertools::Itertools;
use types::{ChitEventType, HeaderField, HttpRequest, HttpResponse, TimestampMillis};

#[query]
fn http_request(request: HttpRequest) -> HttpResponse {
    fn get_avatar_impl(route: AvatarRoute, state: &RuntimeState) -> HttpResponse {
        get_document(route.blob_id, state.data.avatar.as_ref(), "avatar")
    }

    fn get_profile_background_impl(id: Option<u128>, state: &RuntimeState) -> HttpResponse {
        get_document(id, state.data.profile_background.as_ref(), "profile_background")
    }

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

    fn get_swaps(state: &RuntimeState) -> HttpResponse {
        let swaps: Vec<_> = state.data.token_swaps.iter().sorted_unstable_by_key(|s| s.started).collect();

        build_json_response(&swaps)
    }

    fn daily_claims(state: &RuntimeState) -> HttpResponse {
        let (chit_events, _) = state.data.chit_events.events(None, None, 0, 500, false);
        let claims: Vec<_> = chit_events
            .into_iter()
            .filter_map(|e| match e.reason {
                ChitEventType::DailyClaim => Some((e.timestamp, 0)),
                ChitEventType::DailyClaimReinstated => Some((e.timestamp, 1)),
                ChitEventType::StreakInsuranceClaim => Some((e.timestamp, 2)),
                _ => None,
            })
            .map(|(ts, claim_type)| {
                let offset = state.data.streak.utc_offset_mins_at_ts(ts);
                (Streak::timestamp_to_offset_day(ts, offset), claim_type, ts, offset)
            })
            .collect();

        build_json_response(&claims)
    }

    // CVDR receipt download: serves the finalized engine receipt as the engine's
    // canonical serde_json rendering (hex byte arrays, principal strings) — the
    // exact shape the MKTD_RECEIPT_EXPORT_PATH test hook writes and CVDR-Verify
    // accepts. Reuses `mktd02::get_receipt` (same retrieval as `mktd_get_receipt`)
    // and `build_json_response` (engine serde serialisation) — no reshaping, no
    // engine struct change. URL: `/mktd_receipt?id=<64-hex receipt id>`.
    fn get_mktd_receipt(receipt_id_hex: &str) -> HttpResponse {
        let Some(receipt_id) = parse_hex32(receipt_id_hex) else {
            return HttpResponse::not_found();
        };
        match mktd02::get_receipt(&receipt_id) {
            // Only finalized receipts (BLS certificate embedded) are downloadable.
            Some(receipt) if receipt.bls_certificate.is_some() => {
                let mut response = build_json_response(&receipt);
                response.headers.push(HeaderField(
                    "Content-Disposition".to_string(),
                    format!("attachment; filename=\"mktd-receipt-{receipt_id_hex}.json\""),
                ));
                response
            }
            _ => HttpResponse::not_found(),
        }
    }

    fn parse_hex32(hex: &str) -> Option<[u8; 32]> {
        let hex = hex.trim();
        if hex.len() != 64 {
            return None;
        }
        let mut out = [0u8; 32];
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
        }
        Some(out)
    }

    match extract_route(&request.url) {
        Route::Avatar(route) => read_state(|state| get_avatar_impl(route, state)),
        Route::ProfileBackground(id) => read_state(|state| get_profile_background_impl(id, state)),
        Route::Errors(since) => get_errors_impl(since),
        Route::Logs(since) => get_logs_impl(since),
        Route::Traces(since) => get_traces_impl(since),
        Route::Metrics => read_state(get_metrics_impl),
        Route::Other(path, _) if path == "swaps" => read_state(get_swaps),
        Route::Other(path, _) if path == "daily_claims" => read_state(daily_claims),
        // GET-only: a non-GET method (POST/PUT/…) is rejected the same way as a
        // bad id — 404, no panic path — keeping the receipt route read-only and
        // consistent with the other negative-route cases.
        Route::Other(path, qs) if path == "mktd_receipt" && request.method.eq_ignore_ascii_case("GET") => {
            get_mktd_receipt(qs.get("id").map(String::as_str).unwrap_or_default())
        }
        _ => HttpResponse::not_found(),
    }
}
