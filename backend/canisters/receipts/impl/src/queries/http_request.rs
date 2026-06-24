use crate::{RuntimeState, read_state};
use http_request::{Route, build_json_response, encode_logs, extract_route};
use ic_cdk::query;
use types::{HeaderField, HttpRequest, HttpResponse, TimestampMillis};

/// Public READ path (§2): open-by-id, finalized receipts only, served from the
/// durable store as the EXACT stored artifact bytes (§3 — verbatim, no
/// parse-then-reserialize). Mirrors the P1f hardening:
/// - GET-only (any other method → 404, same as a bad id; no panic path),
/// - strict 64-hex id validation,
/// - unknown id → clean 404,
/// - `nosniff` + `no-store` response headers,
/// - no list endpoint, no search-by-principal/user/canister.
///
/// URL: `/receipt?id=<64-hex receipt id>`.
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

    fn get_receipt(receipt_id_hex: &str, state: &RuntimeState) -> HttpResponse {
        let Some(receipt_id) = parse_hex32(receipt_id_hex) else {
            return HttpResponse::not_found();
        };
        match state.data.receipts.get(&receipt_id) {
            // Served verbatim — the stored bytes ARE the user-canister export
            // (byte-identical to the P1f download route / test hook).
            Some(bytes) => HttpResponse {
                status_code: 200,
                headers: vec![
                    HeaderField("Content-Type".to_string(), "application/json".to_string()),
                    HeaderField("X-Content-Type-Options".to_string(), "nosniff".to_string()),
                    HeaderField("Cache-Control".to_string(), "no-store".to_string()),
                    HeaderField(
                        "Content-Disposition".to_string(),
                        format!("attachment; filename=\"mktd-receipt-{receipt_id_hex}.json\""),
                    ),
                ],
                body: bytes,
                streaming_strategy: None,
                upgrade: None,
            },
            None => HttpResponse::not_found(),
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
        Route::Errors(since) => get_errors_impl(since),
        Route::Logs(since) => get_logs_impl(since),
        Route::Traces(since) => get_traces_impl(since),
        Route::Metrics => read_state(get_metrics_impl),
        // GET-only: a non-GET method is rejected the same way as a bad id — 404,
        // no panic path (P1f method-gate parity).
        Route::Other(path, qs) if path == "receipt" && request.method.eq_ignore_ascii_case("GET") => {
            read_state(|state| get_receipt(qs.get("id").map(String::as_str).unwrap_or_default(), state))
        }
        _ => HttpResponse::not_found(),
    }
}
