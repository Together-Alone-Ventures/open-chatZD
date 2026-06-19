use candid::CandidType;
use serde::{Deserialize, Serialize};
use ts_export::ts_export;

/// MKTd02 Phase C (owner-guarded). The owner-guard IS the host authorization
/// that must run before the engine's `finalize_receipt_after_host_authorization`
/// (which performs no caller check). Embeds the Phase-B certificate into the
/// pending receipt and releases the finalization lock.
#[ts_export(user, mktd_finalize_deletion)]
#[derive(CandidType, Serialize, Deserialize, Debug)]
pub struct Args {
    /// 32-byte receipt id from Phase A / Phase B.
    pub receipt_id: Vec<u8>,
    /// BLS certificate captured from Phase B (`mktd_pending_certificate`).
    pub certificate: Vec<u8>,
}

#[ts_export(user, mktd_finalize_deletion)]
#[derive(CandidType, Serialize, Deserialize, Debug)]
pub enum Response {
    Success,
    /// Surfaces the engine's finalization rejection verbatim: NoPendingReceipt,
    /// ReceiptIdMismatch, AlreadyFinalized, ReceiptNotFound, EncodingFailed,
    /// or InvalidReceiptId (wrong length).
    Error(String),
}
