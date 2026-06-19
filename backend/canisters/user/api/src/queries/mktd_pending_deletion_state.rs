use candid::CandidType;
use serde::{Deserialize, Serialize};
use ts_export::ts_export;
use types::Empty;

pub type Args = Empty;

/// MKTd02 pending-finalization-state query (S8), for P3 recovery detection.
/// Read-only; reports whether a post-Phase-A / pre-finalize receipt exists and
/// its receipt id. Leaks no raw principal / raw record_id (the receipt id is a
/// hash).
#[ts_export(user, mktd_pending_deletion_state)]
#[derive(CandidType, Serialize, Deserialize, Debug)]
pub struct Response {
    /// True while the finalization lock is held (Phase A done, Phase C pending).
    pub pending: bool,
    /// Whether the heap PII has been tombstoned (set in Phase A).
    pub tombstoned: bool,
    /// 32-byte pending receipt id, if a receipt is pending.
    pub receipt_id: Option<Vec<u8>>,
}
