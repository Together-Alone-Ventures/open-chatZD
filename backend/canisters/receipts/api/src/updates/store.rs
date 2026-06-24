use candid::CandidType;
use serde::{Deserialize, Serialize};

/// Gated store endpoint (§2 WRITE path / §3 store-and-serve). The caller must be
/// in the configured export authority (enforced by the impl guard, NOT a Response
/// variant — an unauthorized caller is rejected at the message boundary).
///
/// `receipt_json` is the EXACT finalized-CVDR JSON bytes the user canister
/// produced (same canonical serde_json rendering as the P1f download route /
/// test hook). The canister stores these bytes verbatim and serves them verbatim;
/// it parses them only to validate (finalized + id binding), never to reshape.
#[derive(CandidType, Serialize, Deserialize, Debug)]
pub struct Args {
    /// 32-byte receipt id (raw bytes, not hex).
    pub receipt_id: Vec<u8>,
    /// Canonical finalized-receipt JSON bytes, stored and served verbatim.
    pub receipt_json: Vec<u8>,
}

#[derive(CandidType, Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum Response {
    /// Newly stored.
    Success,
    /// Idempotent OK: same id already stored with byte-identical content (§2).
    AlreadyExists,
    /// Hard reject: same id already stored with DIFFERENT bytes (§2).
    Conflict,
    /// `receipt_id` is not exactly 32 bytes.
    InvalidReceiptId,
    /// `receipt_json` does not parse as a finalized receipt (no BLS certificate)
    /// or its embedded `receipt_id` does not match the supplied `receipt_id`.
    NotFinalized,
}
