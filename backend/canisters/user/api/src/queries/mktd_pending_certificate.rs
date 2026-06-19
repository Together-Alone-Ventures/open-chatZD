use candid::CandidType;
use serde::{Deserialize, Serialize};
use ts_export::ts_export;
use types::Empty;

pub type Args = Empty;

/// MKTd02 Phase B. **Must be called as a query** — `data_certificate()` is only
/// available in query context. Returns the BLS certificate + receipt binding
/// for the pending receipt, to be passed to Phase C. Returns no PII.
#[ts_export(user, mktd_pending_certificate)]
#[derive(CandidType, Serialize, Deserialize, Debug)]
pub enum Response {
    Success(PendingCertificate),
    /// No receipt is pending finalization, or the runtime returned no certificate.
    NotPending,
}

#[ts_export(user, mktd_pending_certificate)]
#[derive(CandidType, Serialize, Deserialize, Debug)]
pub struct PendingCertificate {
    /// 32-byte pending receipt id.
    pub receipt_id: Vec<u8>,
    /// 32-byte certified commitment currently in certified data.
    pub certified_commitment: Vec<u8>,
    /// BLS certificate blob from `ic0.data_certificate()`.
    pub certificate: Vec<u8>,
}
