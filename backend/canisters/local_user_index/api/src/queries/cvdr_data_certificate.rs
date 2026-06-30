use candid::CandidType;
use serde::{Deserialize, Serialize};
use types::Empty;

pub type Args = Empty;

#[derive(CandidType, Serialize, Deserialize, Debug)]
pub enum Response {
    /// The IC `data_certificate()` over the currently-pending CVDR commitment, plus the
    /// `receipt_id` / `commitment` it certifies, for the external finalizer to relay back
    /// via `finalize_cvdr`.
    Success(SuccessResult),
    /// No commitment is currently pending, or the certificate is not yet available (e.g.
    /// called in a replicated context rather than as a query).
    NotAvailable,
}

#[derive(CandidType, Serialize, Deserialize, Debug)]
pub struct SuccessResult {
    pub receipt_id: [u8; 32],
    pub commitment: [u8; 32],
    pub certificate: Vec<u8>,
}
