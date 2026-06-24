use serde::{Deserialize, Serialize};
use types::Empty;

pub type Args = Empty;

/// Pre-uninstall export source (P2). Returns this user's FINALIZED CVDR as the
/// canonical serde_json bytes — byte-identical to the P1f download route and the
/// test-hook export — for the `local_user_index` orchestrator to forward verbatim
/// to the receipts canister before uninstall. `NotFinalized` if no finalized
/// receipt exists (e.g. a non-CVDR deletion), in which case there is nothing to
/// export and uninstall may proceed.
#[derive(Serialize, Deserialize, Debug)]
pub enum Response {
    Success(ExportedReceipt),
    NotFinalized,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ExportedReceipt {
    /// 32-byte receipt id (raw bytes).
    pub receipt_id: Vec<u8>,
    /// Canonical finalized-receipt JSON bytes (verbatim, not reshaped).
    pub receipt_json: Vec<u8>,
}
