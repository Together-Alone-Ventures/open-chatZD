use candid::CandidType;
use serde::{Deserialize, Serialize};

/// Permissionless backstop submission (spec §7). The caller relays a `{certificate, witness}`
/// snapshot captured from the canister's own `GET /cvdr_live/<receipt_id>` query route.
///
/// Both bytes are load-bearing and must be the SAME snapshot: the certificate pins a *historical*
/// certified receipt-tree root (its `certified_data`), and the witness must reconstruct to THAT
/// root while revealing `receipt_id -> receipt_hash`. The witness cannot be regenerated in-canister
/// at submission time — the live tree has advanced past the certificate's root as later receipts
/// were inserted — so it must be submitted alongside the certificate. `receipt_hash` and the root
/// are index-derived (recomputed from the durable draft / taken from the certificate), so they are
/// not part of the submission.
#[derive(CandidType, Serialize, Deserialize, Debug)]
pub struct Args {
    pub receipt_id: [u8; 32],
    /// IC certificate CBOR (`data_certificate()` captured from `/cvdr_live`). The cert is itself
    /// the authorization: only the IC (NNS) can produce it, and only over this canister's
    /// certified receipt-tree root.
    pub certificate: Vec<u8>,
    /// IC `HashTree` CBOR witness for `["receipts", receipt_id]`, from the SAME `/cvdr_live`
    /// snapshot as `certificate` (see the note above on why it must be submitted, not regenerated).
    pub witness: Vec<u8>,
}

/// Backstop verdict (spec §7). The stable API surface is THIS variant set — the taxonomy callers
/// may match on. `Captured`/`LateFinalized` both mean "frozen package stored"; the in-window vs
/// late TIER a verifier ultimately reports (`VerifiedFinal` / `LateFinalized`) is derived offline
/// by CVDR-Verify from the package's own hash-bound fields, and is never asserted by the index —
/// `Captured` here is the index-side capture marker (`DraftStage::CertificateCaptured`), NOT a
/// `VerifiedFinal` claim.
#[derive(CandidType, Serialize, Deserialize, Debug)]
pub enum Response {
    /// Verified AND in-window: the frozen package was stored (spec §7 rules 1–6; §5 in-window).
    Captured,
    /// Verified but LATE (valid, outside the finalization window): stored as a late package, and it
    /// can only ever read as `LateFinalized` — never promoted to `VerifiedFinal` (spec §7 rule 8).
    LateFinalized,
    /// No receipt is in a finalizable state (`AwaitingCertificate` / `FailedStuck`) for this
    /// `receipt_id` (spec §7 rule 1).
    NotPending,
    /// A frozen package already exists for this receipt: first valid package wins and is never
    /// overwritten (spec §7 rules 6–7). This submission was a no-op; the receipt is finalized.
    AlreadyFinalized,
    /// The submission failed verification and NOTHING was stored (spec §7 rules 2–5; the store is
    /// gated on full BLS→NNS→delegation→range + witness + window checks).
    ///
    /// The `text` is a DIAGNOSTIC reason (e.g. `certificate_signature_invalid`,
    /// `witness_leaf_ne_receipt_hash`) for logs/operators — it is NOT a stable API and callers MUST
    /// NOT match on its contents. The stable taxonomy is the variant set itself.
    Rejected(String),
}
