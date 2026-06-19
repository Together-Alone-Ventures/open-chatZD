use candid::CandidType;
use serde::{Deserialize, Serialize};
use ts_export::ts_export;
use types::Empty;

pub type Args = Empty;

/// MKTd02 Phase A (owner-guarded). Tombstones the user-canister PII, publishes
/// the certified commitment, takes the finalization lock, and returns the
/// pending receipt id. After this returns the canister is in the
/// point-of-no-return pending state (D8); call `mktd_pending_certificate`
/// (Phase B) then `mktd_finalize_deletion` (Phase C).
#[ts_export(user, mktd_execute_deletion)]
#[derive(CandidType, Serialize, Deserialize, Debug)]
pub enum Response {
    /// 32-byte receipt id of the pending receipt.
    Success(Vec<u8>),
    /// Engine rejected Phase A (e.g. already tombstoned / not initialised).
    Error(String),
}
