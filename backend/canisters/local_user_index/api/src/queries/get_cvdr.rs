use candid::CandidType;
use serde::{Deserialize, Serialize};
use types::{CanisterId, TimestampMillis};

#[derive(CandidType, Serialize, Deserialize, Debug)]
pub struct Args {
    /// The unguessable public fetch capability. (The `record_id` path is NOT exposed here
    /// in build-v1 — `record_id` is derivable from the public UserId, so it is restricted.)
    pub receipt_id: [u8; 32],
}

#[derive(CandidType, Serialize, Deserialize, Debug)]
pub enum Response {
    Success(CvdrReceipt),
    NotFound,
}

/// The releasable CVDR (V1 field set + V2 certificate + V3 release reference). Mirrors the
/// canister-internal `ReleasedCvdr`; an external CVDR-Verify re-checks the NNS signature over
/// `certificate` and matches `commitment` to (record_id, deletion_seq, h_index, h_user_pre).
#[derive(CandidType, Serialize, Deserialize, Debug, Clone)]
pub struct CvdrReceipt {
    pub encoder_version: String,
    pub receipt_id: [u8; 32],
    pub record_id: [u8; 32],
    pub deletion_seq: u64,
    pub user_canister_id: CanisterId,
    /// This index's own principal (executor) — recomputes `h_index` with `executor_module_hash`.
    pub index_canister_id: CanisterId,
    /// Raw TARGET (user canister) pre-uninstall module hash — recomputes `h_user_pre`.
    pub module_hash_pre: Vec<u8>,
    /// Raw EXECUTOR (index) module hash captured pre-uninstall — recomputes `h_index`.
    pub executor_module_hash: Vec<u8>,
    pub h_user_pre: [u8; 32],
    pub h_index: [u8; 32],
    pub commitment: [u8; 32],
    pub certificate: Vec<u8>,
    pub created_at: TimestampMillis,
    pub finalized_at: TimestampMillis,
}
