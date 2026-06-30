use candid::CandidType;
use serde::{Deserialize, Serialize};

#[derive(CandidType, Serialize, Deserialize, Debug)]
pub struct Args {
    pub receipt_id: [u8; 32],
    /// The IC `data_certificate()` bytes obtained from `cvdr_data_certificate`. The cert is
    /// itself the authorization: it is only obtainable once the canister published this exact
    /// commitment to `certified_data`, and only the IC (NNS) can produce it.
    pub certificate: Vec<u8>,
}

#[derive(CandidType, Serialize, Deserialize, Debug)]
pub enum Response {
    /// The releasable CVDR was stored and the deletion completed (also returned idempotently
    /// if the receipt was already finalized).
    Success,
    /// No deletion is awaiting a certificate for this `receipt_id` (and none is stored).
    NotPending,
    /// The certificate does not certify this canister's `certified_data == commitment`.
    CertificateMismatch,
}
