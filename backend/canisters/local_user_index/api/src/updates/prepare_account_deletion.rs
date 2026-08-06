use candid::CandidType;
use oc_error_codes::OCError;
use serde::{Deserialize, Serialize};
use ts_export::ts_export;
use types::Empty;

pub type Args = Empty;

#[ts_export(local_user_index, prepare_account_deletion)]
#[derive(CandidType, Serialize, Deserialize, Debug, Clone)]
pub struct SuccessResult {
    /// 64 lowercase hex chars — bearer `receipt_id` (spec §11.1).
    pub receipt_id: String,
    /// Canonical RevealWire JSON (`openchatzd.cvdr.reveal_package`).
    pub reveal_wire_json: String,
}

/// Spec §11.4: issue `{ receipt_id, RevealWire }` and create a prepared draft.
/// Re-issue is allowed while still `Prepared`; once deletion commits, reveal is once-only.
#[ts_export(local_user_index, prepare_account_deletion)]
#[derive(CandidType, Serialize, Deserialize, Debug)]
pub enum Response {
    Success(SuccessResult),
    /// Deletion already committed (or past prepare) — reveal cannot be re-issued.
    AlreadyCommitted,
    /// User canister could not supply targets / status (transient).
    UserCanisterUnavailable(String),
    Error(OCError),
}
