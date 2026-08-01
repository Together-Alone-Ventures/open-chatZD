//! Narrow local Candid shim for the management-canister `http_request` outcall carrying
//! `is_replicated: Option<bool>` (spec §6; A0-proven).
//!
//! The workspace is pinned to ic-cdk 0.18.7 → `ic-management-canister-types 0.3.3`, whose
//! `HttpRequestArgs` predates the `is_replicated` field. Rather than bump the workspace CDK, we
//! mirror only the argument/result records we need and encode `is_replicated` ourselves; the
//! actual IC management canister already supports the field. This is a **local type only** — no
//! workspace/CDK change.
//!
//! Notes:
//! - Candid field names must match the management-canister interface exactly.
//! - `transform` is deliberately OMITTED: it is an optional field and we never use it, so candid
//!   decodes it as `null` on the callee side — this avoids modelling the `func` type entirely.
//! - Cost: `cost_http_request` returns the (replicated, all-nodes) price. A non-replicated call
//!   costs less and the management canister refunds the surplus, so over-attaching is safe.

use candid::{CandidType, Principal};
use ic_cdk::call::Call;
use serde::Deserialize;

#[derive(CandidType, Deserialize, Clone, Debug)]
pub enum HttpMethod {
    #[serde(rename = "get")]
    Get,
    #[serde(rename = "post")]
    Post,
    #[serde(rename = "head")]
    Head,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct HttpHeader {
    pub name: String,
    pub value: String,
}

/// Mirror of the management-canister `http_request` argument, with `is_replicated`. `transform`
/// is intentionally absent (see module docs).
#[derive(CandidType, Clone, Debug)]
struct HttpRequestArgs {
    url: String,
    max_response_bytes: Option<u64>,
    method: HttpMethod,
    headers: Vec<HttpHeader>,
    body: Option<Vec<u8>>,
    /// `Some(false)` => single-replica, non-replicated outcall (spec §6). The point of the shim.
    is_replicated: Option<bool>,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct HttpRequestResult {
    pub status: candid::Nat,
    pub headers: Vec<HttpHeader>,
    #[serde(with = "serde_bytes")]
    pub body: Vec<u8>,
}

/// Make a NON-replicated GET outcall (`is_replicated: Some(false)`) to `url`, capping the
/// response at `max_response_bytes`. Returns the raw result or a stringified failure.
pub async fn non_replicated_get(url: String, max_response_bytes: u64) -> Result<HttpRequestResult, String> {
    non_replicated_http(url, HttpMethod::Get, Vec::new(), None, max_response_bytes).await
}

/// Make a NON-replicated POST outcall with an optional body and headers.
pub async fn non_replicated_post(
    url: String,
    headers: Vec<HttpHeader>,
    body: Vec<u8>,
    max_response_bytes: u64,
) -> Result<HttpRequestResult, String> {
    non_replicated_http(url, HttpMethod::Post, headers, Some(body), max_response_bytes).await
}

async fn non_replicated_http(
    url: String,
    method: HttpMethod,
    headers: Vec<HttpHeader>,
    body: Option<Vec<u8>>,
    max_response_bytes: u64,
) -> Result<HttpRequestResult, String> {
    let body_len = body.as_ref().map(|b| b.len() as u64).unwrap_or(0);
    let header_len: u64 = headers.iter().map(|h| (h.name.len() + h.value.len()) as u64).sum();
    let args = HttpRequestArgs {
        url,
        max_response_bytes: Some(max_response_bytes),
        method,
        headers,
        body,
        is_replicated: Some(false),
    };

    let request_size = args.url.len() as u64 + header_len + body_len;
    let cycles = ic_cdk::api::cost_http_request(request_size, max_response_bytes);

    Call::unbounded_wait(Principal::management_canister(), "http_request")
        .with_arg(&args)
        .with_cycles(cycles)
        .await
        .map_err(|e| format!("http_request outcall failed: {e:?}"))?
        .candid::<HttpRequestResult>()
        .map_err(|e| format!("http_request result decode failed: {e:?}"))
}
