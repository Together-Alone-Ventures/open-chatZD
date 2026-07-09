use ic_cdk::query;
use local_user_index_canister::cvdr_data_certificate::{Response::*, *};

// SLICE 1 — INERT (spec §8b). This was the external-finalizer polling query: it returned the IC
// `data_certificate()` bound to the single pending receipt via the global single-flight guard
// (`cvdr_awaiting_receipt_id`). Spec §2 removes that guard (the certified receipt tree holds many
// receipts under one root), and spec §6 replaces this polling interface with the Slice 2
// self-finalization route — the canister's own raw-domain `GET /cvdr/<receipt_id>` query returning
// `{receipt, witness, data_certificate()}`. Until that lands, this endpoint returns `NotAvailable`.
// Interim, dev-branch-only; never ships.
#[query]
fn cvdr_data_certificate(_args: Args) -> Response {
    NotAvailable
}
