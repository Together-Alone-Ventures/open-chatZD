//! OpenChatZD CVDR-on-Index store + commitment primitives (design v5).
//!
//! The local_user_index is the canister that authoritatively de-references a user
//! (removes the `global_users` / `local_users` mappings and the user canister's code).
//! v5 makes that de-reference *witnessable*: at the delete seam the index captures a
//! durable draft, uninstalls the user canister, publishes a single IC-certified
//! commitment over the deletion, waits for an external finalizer to hand back the IC
//! `data_certificate()`, then stores a releasable CVDR keyed by an unguessable
//! `receipt_id`.
//!
//! ## Receipt formulas (PINNED — Brief B1 rulings R-1 / R-2 / R-4 / R-5; bump the tag for ANY change)
//! All hashes are domain-separated SHA-256 over a fixed, length-stable preimage:
//! - `record_id   = SHA-256(RECORD_ID_TAG_V2 || record_salt(32) || canonical UserId principal bytes)`
//!   — NON-IDENTIFYING (R-1): `record_salt` is 32 `raw_rand` bytes sampled once at prepare,
//!   persisted in the draft, never regenerated, never in the public package; it reaches the user
//!   only in RevealWire. Without the salt a public `UserId` cannot be joined to a receipt.
//! - `h_user_pre  = SHA-256(H_USER_TAG  || user_canister_principal || module_hash_pre)`
//!   — Index-recorded observation of the target canister's pre-uninstall module hash:
//!   integrity-bound by the certified receipt relation, NOT independently attested (R-2 addendum).
//! - `receipt_id  = SHA-256(RECEIPT_ID_TAG || record_id || deletion_seq(8,BE) || nonce)`
//!   — canonically derived, stable and independently recomputable from the receipt fields (R-5);
//!   the 32-byte `nonce` (LUI rng, generated once before ID formation, persisted unchanged) is the
//!   fetch capability.
//! - `leaf        = SHA-256(RECEIPT_LEAF_TAG || RECEIPT_BODY_V2)` — the OpenChat deletion-event
//!   value: `body fields -> leaf -> ReceiptTree inclusion -> certified root` (R-4).
//!
//! No deployer-supplied module hash enters any preimage (R-2). Index code identity is carried by
//! the authenticated `/canister/<LUI>/module_hash` certificate in the evidence layer, never by the
//! body.
//!
//! RETIRED, historical only, never reused: `OPENCHATZD_RECORD_ID_USER_V1`,
//! `OPENCHATZD_CVDR_H_INDEX_V1`, `OPENCHATZD_CVDR_COMMITMENT_V1`, `OPENCHATZD_RECEIPT_BODY_V1`, and
//! the encoder version string `OPENCHATZD_CVDR_V1`.
//!
//! ## Certificate — the IC `data_certificate()` bytes are stored verbatim in the
//!         [`FrozenCvdrPackage`] (`certificate_bytes`), so an external verifier (CVDR-Verify) can
//!         re-check the NNS signature over the certified receipt-tree root itself. The index
//!         verifies the certificate in full BEFORE storing (store-gate); it never claims
//!         `VerifiedFinal`.

use crate::memory::{
    Memory, get_cvdr_draft_memory, get_cvdr_frozen_log_data_memory, get_cvdr_frozen_log_index_memory,
    get_cvdr_frozen_primary_memory, get_cvdr_frozen_secondary_memory,
};
use candid::{CandidType, Principal};
use constants::DAY_IN_MS;
use ic_certification::{AsHashTree, RbTree, labeled, labeled_hash};
use ic_stable_structures::storable::Bound;
use ic_stable_structures::{StableBTreeMap, StableLog, Storable};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use types::{CanisterId, TimestampMillis, UserId};

/// PINNED domain tag for the non-identifying record_id (R-1).
const RECORD_ID_TAG_V2: &[u8] = b"OPENCHATZD_RECORD_ID_USER_V2";
/// PINNED domain tag for h_user_pre.
const H_USER_TAG: &[u8] = b"OPENCHATZD_CVDR_H_USER_V1";
/// PINNED domain tag for the unguessable receipt_id.
const RECEIPT_ID_TAG: &[u8] = b"OPENCHATZD_CVDR_RECEIPT_V1";
/// PINNED domain tag for the receipt-tree leaf (spec §2).
const RECEIPT_LEAF_TAG: &[u8] = b"OPENCHATZD_RECEIPT_LEAF_V1";
/// PINNED domain tag for the RECEIPT_BODY_V2 preimage (R-4). Being the first bytes of the leaf
/// preimage it also domain-separates V2 leaves from V1 leaves under the unchanged leaf tag.
const RECEIPT_BODY_TAG: &[u8] = b"OPENCHATZD_RECEIPT_BODY_V2";
/// PINNED domain tag for TARGETS_COMMITMENT_V1 (spec §2).
const TARGETS_COMMITMENT_TAG: &[u8] = b"OPENCHATZD_TARGETS_COMMITMENT_V1";
/// Byte-string label for the receipt-tree path `["receipts", receipt_id]` (spec §2).
const RECEIPTS_LABEL: &[u8] = b"receipts";

/// Domain tags and version strings RETIRED by the suite-v5 rulings (R-1 / R-2 / R-4). Historical
/// only — no live tag may ever equal one of these.
#[cfg(test)]
const RETIRED_TAGS: [&[u8]; 5] = [
    b"OPENCHATZD_RECORD_ID_USER_V1",
    b"OPENCHATZD_CVDR_H_INDEX_V1",
    b"OPENCHATZD_CVDR_COMMITMENT_V1",
    b"OPENCHATZD_RECEIPT_BODY_V1",
    b"OPENCHATZD_CVDR_V1",
];

pub type Hash = [u8; 32];

/// `SHA-256(tag || parts...)` over a length-stable preimage (all parts are fixed-width
/// hashes / principals / big-endian integers, so no separators are required).
fn tagged(tag: &[u8], parts: &[&[u8]]) -> Hash {
    let mut preimage: Vec<u8> = Vec::with_capacity(tag.len() + parts.iter().map(|p| p.len()).sum::<usize>());
    preimage.extend_from_slice(tag);
    for part in parts {
        preimage.extend_from_slice(part);
    }
    sha256::sha256(&preimage)
}

/// `record_id = SHA-256(RECORD_ID_TAG_V2 || record_salt(32) || canonical UserId principal bytes)`
/// (R-1). Non-identifying: recomputable only by a holder of the per-deletion `record_salt`
/// (the user, via RevealWire). Never `caller()`.
pub fn record_id_v2(record_salt: &[u8; 32], user_id: UserId) -> Hash {
    let principal: Principal = user_id.into();
    tagged(RECORD_ID_TAG_V2, &[record_salt, principal.as_slice()])
}

pub fn h_user_pre(user_canister_id: CanisterId, module_hash_pre: &[u8]) -> Hash {
    tagged(H_USER_TAG, &[user_canister_id.as_slice(), module_hash_pre])
}

pub fn receipt_id_for(record_id: &Hash, deletion_seq: u64, nonce: &[u8; 32]) -> Hash {
    tagged(RECEIPT_ID_TAG, &[record_id, &deletion_seq.to_be_bytes(), nonce])
}

/// `TARGETS_COMMITMENT_V1` (spec §2, FROZEN). Salted commitment over the cleanup targets so
/// the public receipt carries only `count + salted-hash` (user-held reveal package = salt +
/// sorted list, delivered in a later slice). Deterministic recompute: targets sorted ascending
/// by raw principal bytes; `serialized = concat over sorted targets of len(u8) ‖ principal_bytes`;
/// `commitment = SHA-256(TARGETS_COMMITMENT_TAG ‖ salt ‖ serialized)`. `count = 0` is valid and
/// the commitment is still computed. `salt` is fresh `raw_rand` per deletion, never reused.
pub fn targets_commitment(salt: &[u8; 32], targets: &[CanisterId]) -> Hash {
    let mut sorted: Vec<&CanisterId> = targets.iter().collect();
    sorted.sort_by(|a, b| a.as_slice().cmp(b.as_slice()));
    let mut serialized: Vec<u8> = Vec::new();
    for t in sorted {
        let bytes = t.as_slice();
        // Principals are <= 29 bytes, so the length prefix fits in a u8 (frozen encoding).
        serialized.push(bytes.len() as u8);
        serialized.extend_from_slice(bytes);
    }
    tagged(TARGETS_COMMITMENT_TAG, &[salt, &serialized])
}

/// `RECEIPT_BODY_V2` (R-4: `RECEIPT_BODY_V1 − {commitment, h_index}`; layout PINNED). A versioned
/// fixed-width tag-concatenation (NOT CBOR). Binds every field a verifier needs, in this exact
/// order:
///
/// `RECEIPT_BODY_TAG ‖ receipt_id(32) ‖ nonce(32) ‖ index_canister_id(len(u8)‖bytes) ‖`
/// `user_canister_id(len(u8)‖bytes) ‖ record_id(32) ‖ deletion_seq(u64 BE) ‖ h_user_pre(32) ‖`
/// `uninstall_completed_at(u64 BE) ‖ receipt_committed_at(u64 BE) ‖`
/// `targets_count(u32 BE) ‖ targets_commitment(32)`.
///
/// `nonce` is `receipt_id`'s only derivation input not otherwise present (record_id + deletion_seq
/// appear below). Principals are `len(u8) ‖ raw bytes` (G adjacency ruling: two adjacent
/// variable-length principals must be self-delimiting, else distinct id pairs could concatenate
/// identically — a second-preimage surface on the identity fields; unifies with
/// TARGETS_COMMITMENT_V1). Integers are fixed-width big-endian. Timestamps are IC consensus
/// **nanoseconds**; the load-bearing `receipt_committed_at` is hash-bound (window-rule anti-spoof).
/// `delete_requested_at` is excluded (context only); `certificate_time` arrives later. CVDR-Verify
/// must recompute this exact layout.
#[allow(clippy::too_many_arguments)]
pub fn receipt_body_v2(
    receipt_id: &Hash,
    nonce: &[u8; 32],
    index_canister_id: CanisterId,
    user_canister_id: CanisterId,
    record_id: &Hash,
    deletion_seq: u64,
    h_user_pre: &Hash,
    uninstall_completed_at_ns: u64,
    receipt_committed_at_ns: u64,
    targets_count: u32,
    targets_commitment: &Hash,
) -> Vec<u8> {
    let mut body: Vec<u8> = Vec::new();
    body.extend_from_slice(RECEIPT_BODY_TAG);
    body.extend_from_slice(receipt_id);
    body.extend_from_slice(nonce);
    // Principals: len(u8) ‖ raw bytes (G adjacency ruling — two adjacent variable-length
    // principals must be self-delimiting; identical encoding to TARGETS_COMMITMENT_V1).
    for principal in [index_canister_id, user_canister_id] {
        let bytes = principal.as_slice();
        body.push(bytes.len() as u8);
        body.extend_from_slice(bytes);
    }
    body.extend_from_slice(record_id);
    body.extend_from_slice(&deletion_seq.to_be_bytes());
    body.extend_from_slice(h_user_pre);
    body.extend_from_slice(&uninstall_completed_at_ns.to_be_bytes());
    body.extend_from_slice(&receipt_committed_at_ns.to_be_bytes());
    body.extend_from_slice(&targets_count.to_be_bytes());
    body.extend_from_slice(targets_commitment);
    body
}

/// Receipt-tree leaf value (spec §2): `SHA-256(RECEIPT_LEAF_TAG ‖ receipt_body)`. This equals
/// `FrozenCvdrPackage.receipt_hash` and the value revealed at the witness leaf.
pub fn receipt_leaf(receipt_body: &[u8]) -> Hash {
    tagged(RECEIPT_LEAF_TAG, &[receipt_body])
}

/// Max age of a submitted certificate: its BLS-signed snapshot time must be within this of
/// `now`. Defence-in-depth against ancient-certificate replay (the commitment binding below
/// already defeats cross-deletion replay, since a stale cert certifies a stale commitment).
const CERT_MAX_OFFSET_MS: u64 = 5 * 60 * 1_000;
const NANOS_PER_MILLI: u128 = 1_000_000;
/// Allowed gap between `receipt_committed_at` and the certificate `/time` for the
/// self-finalization store-gate (spec §5/§6, 24 h provisional). A verified cert outside this
/// window is NOT stored by the self-loop (a late capture is the backstop's `LateFinalized`, §7).
const ALLOWED_FINALIZATION_WINDOW_NS: u64 = 24 * 60 * 60 * 1_000_000_000;

/// A verified IC certificate's payload: this canister's certified `certified_data`, plus the
/// certificate `/time` (nanoseconds).
#[derive(Debug, PartialEq, Eq)]
pub struct VerifiedCert {
    pub certified_data: Vec<u8>,
    pub cert_time_ns: u64,
}

/// Distinct reason a certificate FAILED [`verify_certificate`]. Replaces the earlier
/// `Option::None` collapse so callers (and the A1 tamper matrix) can assert WHICH check failed —
/// the honest-taxonomy fix (D2). These are internal diagnostics; the public backstop surface maps
/// them to a single opaque `Rejected(text)` (spec §7 — the variant set is the stable taxonomy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertRejectReason {
    /// The bytes are not a decodable IC certificate (CBOR structure invalid).
    CborDecodeFailed,
    /// BLS signature verification against the NNS root (via the delegation chain) failed —
    /// forged/tampered signature, or an invalid delegation key.
    SignatureInvalid,
    /// The certificate does not cover `self_canister_id` (delegation range miss).
    CanisterNotInRange,
    /// Independent store-gate: canister_ranges leaf missing / empty resolved set.
    CanisterRangesMissing,
    /// Independent store-gate: ranges/delegation CBOR malformed, nested delegation, bad shard.
    CanisterRangesMalformed,
    /// The certificate `/time` is outside the freshness offset of `now` (replay / clock skew).
    Stale,
    /// Verified, but no `certified_data` leaf for this canister was present.
    CertifiedDataMissing,
    /// Verified, but no `/time` leaf was present (or it failed to decode).
    TimeMissing,
    /// Any other verification failure (malformed DER key, too many delegations, …).
    OtherVerificationFailure,
}

impl CertRejectReason {
    /// Stable diagnostic string (logs / `Rejected(text)` payload). NOT a stable API to match on.
    pub fn as_str(self) -> &'static str {
        match self {
            CertRejectReason::CborDecodeFailed => "certificate_cbor_decode_failed",
            CertRejectReason::SignatureInvalid => "certificate_signature_invalid",
            CertRejectReason::CanisterNotInRange => "certificate_canister_not_in_range",
            CertRejectReason::CanisterRangesMissing => "certificate_canister_ranges_missing",
            CertRejectReason::CanisterRangesMalformed => "certificate_canister_ranges_malformed",
            CertRejectReason::Stale => "certificate_stale",
            CertRejectReason::CertifiedDataMissing => "certificate_certified_data_missing",
            CertRejectReason::TimeMissing => "certificate_time_missing",
            CertRejectReason::OtherVerificationFailure => "certificate_verification_failed",
        }
    }
}

fn map_range_auth_error(err: crate::model::cvdr_canister_ranges::RangeAuthError) -> CertRejectReason {
    use crate::model::cvdr_canister_ranges::RangeAuthError as E;
    match err {
        E::NotInRange => CertRejectReason::CanisterNotInRange,
        E::RangesMissing => CertRejectReason::CanisterRangesMissing,
        E::Malformed => CertRejectReason::CanisterRangesMalformed,
    }
}

/// Map the upstream verification error to our distinct reason taxonomy.
fn map_cert_verification_error(err: &ic_certificate_verification::CertificateVerificationError) -> CertRejectReason {
    use ic_certificate_verification::CertificateVerificationError as E;
    match err {
        E::SignatureVerificationFailed => CertRejectReason::SignatureInvalid,
        E::PrincipalOutOfRange { .. } | E::SubnetCanisterIdRangesNotFound { .. } | E::SubnetPublicKeyNotFound { .. } => {
            CertRejectReason::CanisterNotInRange
        }
        E::TimeTooFarInTheFuture { .. } | E::TimeTooFarInThePast { .. } | E::TimeDecodingFailed { .. } => {
            CertRejectReason::Stale
        }
        E::MissingTimePathInTree { .. } => CertRejectReason::TimeMissing,
        E::CborDecodingFailed(_) => CertRejectReason::CborDecodeFailed,
        E::DerKeyLengthMismatch { .. } | E::DerPrefixMismatch { .. } | E::CertificateHasTooManyDelegations => {
            CertRejectReason::OtherVerificationFailure
        }
    }
}

/// SECURITY-CRITICAL, reused verbatim by the self-finalization store-gate (§6) and the
/// permissionless backstop (§7). Full on-chain verification of the IC `data_certificate()`:
/// BLS signature -> subnet delegation chain -> NNS `ic_root_key`, confirms `self_canister_id` is
/// in the delegated subnet's ranges, and checks the certificate time is within `CERT_MAX_OFFSET_MS`
/// of `now`. On success returns this canister's certified `certified_data` and the certificate
/// `/time`; on ANY failure returns the distinct [`CertRejectReason`] (unsigned / forged /
/// wrong-subnet / stale / malformed).
pub fn verify_certificate(
    certificate: &[u8],
    self_canister_id: Principal,
    ic_root_key: &[u8],
    now: TimestampMillis,
) -> Result<VerifiedCert, CertRejectReason> {
    use ic_cbor::CertificateToCbor;
    use ic_certificate_verification::VerifyCertificate;
    use ic_certification::{Certificate, LookupResult};

    let cert = Certificate::from_cbor(certificate).map_err(|_| CertRejectReason::CborDecodeFailed)?;
    let now_nanos = (now as u128).saturating_mul(NANOS_PER_MILLI);
    let max_offset_nanos = (CERT_MAX_OFFSET_MS as u128).saturating_mul(NANOS_PER_MILLI);
    cert.verify(self_canister_id.as_slice(), ic_root_key, &now_nanos, &max_offset_nanos)
        .map_err(|e| map_cert_verification_error(&e))?;
    // Independent fail-closed range check (Stef B1): do not rely on vacuous
    // upstream sharded-range behaviour after BLS/delegation verify.
    crate::model::cvdr_canister_ranges::assert_delegation_range_containment(&cert, self_canister_id)
        .map_err(map_range_auth_error)?;

    let certified_data =
        match cert
            .tree
            .lookup_path([b"canister".as_ref(), self_canister_id.as_slice(), b"certified_data".as_ref()])
        {
            LookupResult::Found(d) => d.to_vec(),
            _ => return Err(CertRejectReason::CertifiedDataMissing),
        };
    let cert_time_ns = match cert.tree.lookup_path([b"time".as_ref()]) {
        LookupResult::Found(t) => leb128_u64(t),
        _ => return Err(CertRejectReason::TimeMissing),
    };
    Ok(VerifiedCert {
        certified_data,
        cert_time_ns,
    })
}

#[derive(Debug, PartialEq, Eq)]
pub struct VerifiedIndexModuleHash {
    pub module_hash: Vec<u8>,
    pub cert_time_ns: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexEvidenceRejectReason {
    Certificate(CertRejectReason),
    ModuleHashMissing,
    /// The certificate `/time` predates the receipt's `uninstall_completed_at` — it cannot speak
    /// for the code that performed the deletion.
    CertTimeBeforeUninstall,
}

impl IndexEvidenceRejectReason {
    pub fn as_str(self) -> &'static str {
        match self {
            IndexEvidenceRejectReason::Certificate(r) => r.as_str(),
            IndexEvidenceRejectReason::ModuleHashMissing => "index_module_hash_path_missing",
            IndexEvidenceRejectReason::CertTimeBeforeUninstall => "index_cert_time_before_uninstall",
        }
    }
}

/// Store-gate for INDEX code-identity evidence (R-2): BLS → NNS → range → exact path
/// `/canister/<self>/module_hash` → `/time`. Returns the EXTRACTED module hash (the value stored as
/// `index_module_hash`) and the certificate time. `not_before_ns` is the receipt's
/// `uninstall_completed_at`: a certificate older than the deletion says nothing about the code that
/// performed it. With the upgrade interlock (no upgrade between uninstall and evidence capture) the
/// certified hash is the code identity of the deleting Index.
pub fn verify_index_module_hash_evidence(
    certificate: &[u8],
    self_canister_id: Principal,
    ic_root_key: &[u8],
    now: TimestampMillis,
    not_before_ns: Option<u64>,
) -> Result<VerifiedIndexModuleHash, IndexEvidenceRejectReason> {
    use ic_cbor::CertificateToCbor;
    use ic_certificate_verification::VerifyCertificate;
    use ic_certification::{Certificate, LookupResult};

    let cert = Certificate::from_cbor(certificate)
        .map_err(|_| IndexEvidenceRejectReason::Certificate(CertRejectReason::CborDecodeFailed))?;
    let now_nanos = (now as u128).saturating_mul(NANOS_PER_MILLI);
    let max_offset_nanos = (CERT_MAX_OFFSET_MS as u128).saturating_mul(NANOS_PER_MILLI);
    cert.verify(self_canister_id.as_slice(), ic_root_key, &now_nanos, &max_offset_nanos)
        .map_err(|e| IndexEvidenceRejectReason::Certificate(map_cert_verification_error(&e)))?;
    crate::model::cvdr_canister_ranges::assert_delegation_range_containment(&cert, self_canister_id)
        .map_err(|e| IndexEvidenceRejectReason::Certificate(map_range_auth_error(e)))?;

    let module_hash = match cert
        .tree
        .lookup_path([b"canister".as_ref(), self_canister_id.as_slice(), b"module_hash".as_ref()])
    {
        LookupResult::Found(d) => d.to_vec(),
        _ => return Err(IndexEvidenceRejectReason::ModuleHashMissing),
    };
    let cert_time_ns = match cert.tree.lookup_path([b"time".as_ref()]) {
        LookupResult::Found(t) => leb128_u64(t),
        _ => return Err(IndexEvidenceRejectReason::Certificate(CertRejectReason::TimeMissing)),
    };
    if let Some(not_before) = not_before_ns {
        check_index_cert_not_before_uninstall(cert_time_ns, not_before)?;
    }
    Ok(VerifiedIndexModuleHash {
        module_hash,
        cert_time_ns,
    })
}

pub(crate) fn check_index_cert_not_before_uninstall(
    cert_time_ns: u64,
    uninstall_completed_at_ns: u64,
) -> Result<(), IndexEvidenceRejectReason> {
    if cert_time_ns < uninstall_completed_at_ns {
        Err(IndexEvidenceRejectReason::CertTimeBeforeUninstall)
    } else {
        Ok(())
    }
}

/// R-6: the trust-root id a certificate verified under the Index's configured root key is stamped
/// with. Decided from CONFIGURATION (the replica-provided root key this Index verifies against),
/// never from the certificate being stored and never from a caller.
pub fn trust_root_key_id_for(configured_ic_root_key: &[u8]) -> &'static str {
    use local_user_index_canister::get_cvdr::{TRUST_ROOT_MAINNET, TRUST_ROOT_NON_PRODUCTION};
    if configured_ic_root_key == constants::IC_ROOT_KEY {
        TRUST_ROOT_MAINNET
    } else {
        TRUST_ROOT_NON_PRODUCTION
    }
}

/// How long after the uninstall index evidence may still be captured (and therefore how long a
/// receipt without evidence can block an upgrade). Past it the receipt is permanently
/// V3A-UNAVAILABLE, so blocking longer would protect nothing.
pub const INDEX_EVIDENCE_WINDOW_NS: u64 = 24 * 60 * 60 * 1_000_000_000;

/// A receipt that still NEEDS index evidence and can still GET it (R-2 interlock predicate input).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvidencePending {
    pub receipt_id: Hash,
    pub stage: DraftStage,
    /// `uninstall_completed_at` (ns) — the evidence not-before anchor and window start.
    pub uninstall_completed_at: u64,
}

/// The operator-facing refusal raised while receipts are inside the protected interval
/// `uninstall → index evidence stored` (`None` when nothing blocks).
pub fn evidence_upgrade_refusal(blockers: &[EvidencePending]) -> Option<String> {
    const MAX_LISTED: usize = 20;
    if blockers.is_empty() {
        return None;
    }
    let listed: Vec<String> = blockers
        .iter()
        .take(MAX_LISTED)
        .map(|b| format!("{}({:?})", receipt_id_prefix(&b.receipt_id), b.stage))
        .collect();
    let more = blockers.len().saturating_sub(MAX_LISTED);
    Some(format!(
        "upgrade refused: {} CVDR receipt(s) are inside the protected deletion→evidence interval \
         (uninstalled, authenticated Index module-hash evidence not yet stored): [{}]{}. \
         Operator path: keep the CURRENT wasm installed; the evidence sweep stores the \
         /canister/<index>/module_hash certificate within seconds-to-minutes (check \
         `cvdr_index_evidence_count` / `cvdr_upgrade_blockers` in /metrics), or the 24 h evidence \
         window lapses; then upgrade again.",
        blockers.len(),
        listed.join(", "),
        if more > 0 { format!(" (+{more} more)") } else { String::new() },
    ))
}

/// Distinct reason the store-gate ([`verify_finalization_package`]) REJECTED a submission (spec §7
/// rules 2–5). A reject NEVER creates a frozen package (HARD SECURITY RULE). Internal diagnostics;
/// the backstop maps these to `Rejected(text)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinalizeRejectReason {
    /// The certificate itself failed verification (see [`CertRejectReason`]).
    Certificate(CertRejectReason),
    /// The certified `certified_data` was not a 32-byte receipt-tree root.
    CertifiedDataNot32Bytes,
    /// The witness bytes did not decode as an IC `HashTree`.
    WitnessDecodeFailed,
    /// The witness reconstructs to a root other than the certificate's `certified_data` (spec §9.2).
    WitnessRootNeCertifiedData,
    /// The witness leaf at `["receipts", receipt_id]` is absent or `!= receipt_hash` (spec §9.1).
    WitnessLeafNeReceiptHash,
    /// The certificate `/time` predates `receipt_committed_at` — impossible for an honest capture.
    CertTimeBeforeReceiptCommitted,
}

impl FinalizeRejectReason {
    /// Stable diagnostic string (logs / `Rejected(text)` payload). NOT a stable API to match on.
    pub fn as_str(self) -> &'static str {
        match self {
            FinalizeRejectReason::Certificate(r) => r.as_str(),
            FinalizeRejectReason::CertifiedDataNot32Bytes => "certified_data_not_32_bytes",
            FinalizeRejectReason::WitnessDecodeFailed => "witness_cbor_decode_failed",
            FinalizeRejectReason::WitnessRootNeCertifiedData => "witness_root_ne_certified_data",
            FinalizeRejectReason::WitnessLeafNeReceiptHash => "witness_leaf_ne_receipt_hash",
            FinalizeRejectReason::CertTimeBeforeReceiptCommitted => "cert_time_before_receipt_committed",
        }
    }
}

/// Verdict of the store-gate. Verified packages split by the §5 window into [`InWindow`] and
/// [`Late`]; the tier is DERIVED from `cert_time_ns` vs `receipt_committed_at` (both hash-bound in
/// the package), never stamped. The §6 self-loop stores only `InWindow` (a late capture is the §7
/// backstop's job); the §7 backstop stores both, reporting `Captured` vs `LateFinalized`.
///
/// [`InWindow`]: FinalizeVerdict::InWindow
/// [`Late`]: FinalizeVerdict::Late
pub enum FinalizeVerdict {
    /// Verified AND in-window: store as an in-window package. `tree_root` = the certified root the
    /// witness proves against (the certificate's `certified_data`); `cert_time_ns` = cert `/time`.
    InWindow { cert_time_ns: u64, tree_root: [u8; 32] },
    /// Verified but LATE (gap `> ALLOWED_FINALIZATION_WINDOW_NS`): cryptographically valid, outside
    /// the window. NEVER promoted — a verifier reads it as `LateFinalized` (spec §5/§7 rule 8).
    Late { cert_time_ns: u64, tree_root: [u8; 32] },
    /// Rejected. A failed verification NEVER creates a frozen package (HARD SECURITY RULE). The
    /// self-loop's retry bookkeeping still advances (that is the retry mechanism, not a state change
    /// on failure). Carries the distinct [`FinalizeRejectReason`].
    Reject(FinalizeRejectReason),
}

/// Store-gate (HARD SECURITY RULE — G): FULL on-chain verification BEFORE store. Verifies the
/// certificate via [`verify_certificate`] (BLS -> NNS delegation, range covers self, time), then
/// binds it to OUR receipt: the certificate's `certified_data` is taken as the certified tree root
/// (receipts are append-only, so any root from receipt-commit onward proves the receipt); the
/// returned witness must reconstruct to that root; the witness leaf at `["receipts", receipt_id]`
/// must equal `receipt_hash`; and the certificate time must not predate `receipt_committed_at`.
/// A verified package is then split by the §5 window: gap `<= ALLOWED_FINALIZATION_WINDOW_NS` =>
/// [`InWindow`], else [`Late`]. ANY verification failure => [`Reject`] with a distinct reason
/// (discard + retry / diagnostic, never store), so an untrusted single node cannot poison the
/// first-wins slot with a forged-signature package.
///
/// [`InWindow`]: FinalizeVerdict::InWindow
/// [`Late`]: FinalizeVerdict::Late
/// [`Reject`]: FinalizeVerdict::Reject
#[allow(clippy::too_many_arguments)]
pub fn verify_finalization_package(
    certificate: &[u8],
    witness_cbor: &[u8],
    receipt_id: &Hash,
    receipt_hash: &Hash,
    self_canister_id: Principal,
    ic_root_key: &[u8],
    now: TimestampMillis,
    receipt_committed_at_ns: u64,
) -> FinalizeVerdict {
    use ic_certification::{HashTree, LookupResult};

    let v = match verify_certificate(certificate, self_canister_id, ic_root_key, now) {
        Ok(v) => v,
        Err(reason) => return FinalizeVerdict::Reject(FinalizeRejectReason::Certificate(reason)),
    };
    // The certificate's certified_data IS the certified receipt-tree root (spec §9 rules 2/3).
    let Ok(tree_root): Result<[u8; 32], _> = v.certified_data.as_slice().try_into() else {
        return FinalizeVerdict::Reject(FinalizeRejectReason::CertifiedDataNot32Bytes);
    };
    let Ok(witness) = serde_cbor::from_slice::<HashTree>(witness_cbor) else {
        return FinalizeVerdict::Reject(FinalizeRejectReason::WitnessDecodeFailed);
    };
    if witness.digest() != tree_root {
        return FinalizeVerdict::Reject(FinalizeRejectReason::WitnessRootNeCertifiedData);
    }
    match witness.lookup_path([RECEIPTS_LABEL, receipt_id.as_slice()]) {
        LookupResult::Found(leaf) if leaf == receipt_hash.as_slice() => {}
        _ => return FinalizeVerdict::Reject(FinalizeRejectReason::WitnessLeafNeReceiptHash),
    }
    if v.cert_time_ns < receipt_committed_at_ns {
        return FinalizeVerdict::Reject(FinalizeRejectReason::CertTimeBeforeReceiptCommitted);
    }
    if v.cert_time_ns.saturating_sub(receipt_committed_at_ns) > ALLOWED_FINALIZATION_WINDOW_NS {
        FinalizeVerdict::Late {
            cert_time_ns: v.cert_time_ns,
            tree_root,
        }
    } else {
        FinalizeVerdict::InWindow {
            cert_time_ns: v.cert_time_ns,
            tree_root,
        }
    }
}

/// Decode an unsigned LEB128 integer (the certificate `/time` leaf encoding).
fn leb128_u64(bytes: &[u8]) -> u64 {
    let mut result: u64 = 0;
    let mut shift: u32 = 0;
    for &b in bytes {
        result |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
        if shift >= 64 {
            break;
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Stage of an in-flight deletion draft (forward-only; never rolls back).
// ---------------------------------------------------------------------------

#[derive(CandidType, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DraftStage {
    /// Pre-uninstall: H_user_pre + targets + record_id captured, draft persisted.
    /// Legacy mid-flight only — new deletions enter via [`DraftStage::Prepared`].
    Captured,
    /// `uninstall_code` confirmed (no module). The receipt may now be published.
    Uninstalled,
    /// Receipt published into the certified tree + user deletion completed (spec §8). The
    /// self-finalization loop (spec §6) is capturing the certificate. No single-slot guard —
    /// many drafts may be here at once.
    AwaitingCertificate,
    /// A store-gate (self-loop §6 in-window, or backstop §7 in-window) verified an in-window
    /// certificate and stored the frozen package. Terminal success; no longer re-driven. Maps to
    /// the backstop `Captured` response — this is an index-side CAPTURE marker, NOT a
    /// `VerifiedFinal` claim (the tier stays verifier-derived).
    CertificateCaptured,
    /// The §7 backstop stored a LATE-but-valid certificate (outside the §5 window). Terminal;
    /// index-side ops marker for a late capture. The stored package still reads as `LateFinalized`
    /// to a verifier — never promoted (spec §7 rule 8).
    LateFinalized,
    /// No verified in-window certificate captured before the retry cap / 24 h (spec §6). The
    /// receipt remains in the tree (never removed, §10) and the permissionless backstop (§7) can
    /// still land it — as `CertificateCaptured` (if a fresh cert lands in-window) or `LateFinalized`.
    FailedStuck,
    /// Spec §11.4 prepare: salt + targets + `receipt_id` captured; RevealWire handed to the user.
    /// Not finalizable; not served by `/cvdr_live`. Appended last so candid ordinals of older
    /// variants stay stable across upgrades.
    Prepared,
}

/// In-flight durable deletion draft, keyed by `user_canister_id`. Survives a
/// local_user_index upgrade (stable-backed, `#[serde(skip)]`) so recovery is purely
/// forward (resume from the persisted stage) with no upgrade-trapping heap lock.
#[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
pub struct CvdrDraft {
    pub user_id: UserId,
    pub user_canister_id: CanisterId,
    /// This index's own principal (the executor), bound into the receipt body.
    pub index_canister_id: CanisterId,
    /// R-1: 32 `raw_rand` bytes sampled ONCE at prepare, persisted across retries, never
    /// regenerated at finalisation, never in the public package; handed to the user in RevealWire.
    /// `None` only on a draft written by a pre-V2 wasm (see [`CvdrDraft::is_legacy_pre_v2`]) or a
    /// scrubbed terminal draft. Optional so that pre-V2 stored drafts — including the terminal ones
    /// retained as bookkeeping — still DECODE (Candid rejects a missing non-optional field).
    pub record_salt: Option<[u8; 32]>,
    /// `record_id_v2(record_salt, user_id)` — non-identifying.
    pub record_id: Hash,
    pub deletion_seq: u64,
    /// 32 LUI-rng bytes; makes `receipt_id` unguessable. Captured once, never changed.
    pub nonce: [u8; 32],
    pub receipt_id: Hash,
    /// Pre-uninstall module hash of the TARGET user canister (the destroyed code); empty only
    /// if the canister was already module-less when captured.
    pub module_hash_pre: Vec<u8>,
    /// Index-recorded observation of the target's pre-uninstall module hash (R-2 addendum):
    /// integrity-bound by the certified receipt relation, not independently attested.
    pub h_user_pre: Hash,
    /// 32 `raw_rand` bytes committed into `targets_commitment` (spec §2 TARGETS_COMMITMENT_V1).
    /// Captured at prepare; NEVER reused across deletions. Handed to the user in RevealWire
    /// before irreversible delete (spec §11.4).
    pub salt: [u8; 32],
    /// Group/community targets for `NotifyOfUserDeleted`, captured BEFORE uninstall so
    /// recovery never re-reads the destroyed canister. Also the reveal-package target list and
    /// the `targets_commitment` preimage.
    pub canisters_to_notify: Vec<CanisterId>,
    /// IC consensus time (ns) at `uninstall_code` success (spec §5, load-bearing). 0 until set.
    pub uninstall_completed_at: u64,
    /// IC consensus time (ns) recorded in the SAME message as the tree insert +
    /// `certified_data_set(root)` (spec §5 window anchor; spec §3 atomicity). 0 until published.
    pub receipt_committed_at: u64,
    /// Self-finalization (spec §6) attempt counter, drives the retry backoff schedule.
    pub finalize_attempt: u32,
    /// IC time (ns) of the last self-finalization attempt; with `finalize_attempt` it determines
    /// when the next attempt is due. 0 = never attempted.
    pub finalize_last_attempt_at: u64,
    pub created_at: TimestampMillis,
    pub attempt: u32,
    pub stage: DraftStage,
}

impl CvdrDraft {
    /// `targets_commitment` over this draft's salt + captured targets (spec §2).
    pub fn targets_commitment(&self) -> Hash {
        targets_commitment(&self.salt, &self.canisters_to_notify)
    }

    /// The `RECEIPT_BODY_V2` bytes for this draft. Requires `uninstall_completed_at` and
    /// `receipt_committed_at` to be set (i.e. called at/after publish).
    pub fn receipt_body(&self) -> Vec<u8> {
        receipt_body_v2(
            &self.receipt_id,
            &self.nonce,
            self.index_canister_id,
            self.user_canister_id,
            &self.record_id,
            self.deletion_seq,
            &self.h_user_pre,
            self.uninstall_completed_at,
            self.receipt_committed_at,
            self.canisters_to_notify.len() as u32,
            &self.targets_commitment(),
        )
    }

    /// The tree leaf / `receipt_hash` for this draft (spec §2): `SHA-256(RECEIPT_LEAF_TAG ‖ body)`.
    pub fn receipt_hash(&self) -> Hash {
        receipt_leaf(&self.receipt_body())
    }

    /// Scrub the retained draft of everything sensitive once the certificate is captured (spec §6
    /// store → CertificateCaptured), honouring the adopted privacy default ("nothing sensitive
    /// retained beyond the draft's lifetime"): zero the `salt` and drop the raw cleanup-target list
    /// — the very things `targets_commitment` exists to keep off the server. The user already holds
    /// the reveal package (delivered in-session at deletion — a delivery-leg precondition). The
    /// bookkeeping fields (stage, attempts, timestamps, ids, hashes) are kept for observability /
    /// idempotency; the frozen package (not the draft) is the durable record. After scrubbing, this
    /// draft can no longer recompute `receipt_body`/`receipt_hash`/`targets_commitment` — nothing
    /// does so for a CertificateCaptured draft (the sweep skips it; `/cvdr_live` serves only
    /// AwaitingCertificate; the tree rebuild uses the frozen store's stored `receipt_hash`).
    pub fn scrub_sensitive(&mut self) {
        self.salt = [0u8; 32];
        self.record_salt = None;
        self.canisters_to_notify = Vec::new();
    }

    /// FINALIZABLE = a certificate can still be captured for this receipt: `AwaitingCertificate`
    /// (self-loop in flight) or `FailedStuck` (self-loop gave up, backstop can still land it, §7).
    /// These retain the salt + target list, so `receipt_body`/`receipt_hash` still recompute.
    /// `CertificateCaptured`/`LateFinalized` are terminal (package stored, draft scrubbed) and NOT
    /// finalizable; `Prepared`/`Captured`/`Uninstalled` have no receipt in the tree yet
    /// (`Prepared` is also explicitly non-finalizable per §11.4).
    pub fn is_finalizable(&self) -> bool {
        matches!(self.stage, DraftStage::AwaitingCertificate | DraftStage::FailedStuck)
    }

    /// Canonical RevealWire JSON bytes (CVDR-Verify `openchatzd.cvdr.reveal_package`).
    pub fn reveal_wire_json(&self) -> Vec<u8> {
        let record_salt = self.record_salt.expect(
            "RevealWire needs record_salt: only V2 un-scrubbed drafts are revealed (pre-V2 drafts are refused at upgrade)",
        );
        reveal_wire_canonical_json(&self.salt, &record_salt, &self.canisters_to_notify)
    }

    /// A draft written by a pre-V2 wasm (V1 body, identifying `record_id`, no `record_salt`) that
    /// has not reached a terminal stage. Such a draft cannot be carried forward: recomputing its
    /// leaf under V2 would publish a V2-tagged receipt over a V1 `record_id`. Terminal drafts are
    /// scrubbed bookkeeping (their frozen package is the record) and are never legacy-blocking.
    pub fn is_legacy_pre_v2(&self) -> bool {
        self.record_salt.is_none() && !self.is_terminal()
    }

    /// R-2 interlock: this draft's receipt is past the uninstall, so the code that ran the deletion
    /// must stay installed until index evidence is stored. `Prepared` / legacy `Captured` have not
    /// uninstalled anything; every later stage — terminal ones included, because a package can be
    /// stored before its evidence — is protected.
    pub fn is_past_uninstall(&self) -> bool {
        self.uninstall_completed_at > 0 && !matches!(self.stage, DraftStage::Prepared | DraftStage::Captured)
    }

    /// Terminal = the frozen package is stored and the draft is scrubbed bookkeeping.
    pub fn is_terminal(&self) -> bool {
        matches!(self.stage, DraftStage::CertificateCaptured | DraftStage::LateFinalized)
    }
}

/// Spec §11.4 / CVDR-Verify reveal schema id.
pub const REVEAL_SCHEMA_ID: &str = "openchatzd.cvdr.reveal_package";
/// 2 = adds `record_salt` (R-1). Version 1 (targets salt only) is historical.
pub const REVEAL_VERSION: u64 = 2;
pub const REVEAL_ENCODING: &str = "hex";

/// Prepared-but-not-deleted drafts TTL-purge after this (spec §11.4); aligns with the 24 h window.
pub const PREPARED_DRAFT_TTL_MS: TimestampMillis = DAY_IN_MS;

#[derive(Serialize)]
struct CanonicalRevealWire<'a> {
    schema: &'a str,
    version: u64,
    encoding: &'a str,
    salt: String,
    record_salt: String,
    targets: Vec<String>,
}

/// Canonical RevealWire JSON, field order `schema, version, encoding, salt, record_salt, targets`:
/// targets-salt hex, record-salt hex (the user's authorised-linkage secret for `record_id`, R-1),
/// and targets as principal text sorted by raw bytes (same order as `targets_commitment`).
pub fn reveal_wire_canonical_json(salt: &[u8; 32], record_salt: &[u8; 32], targets: &[CanisterId]) -> Vec<u8> {
    let mut sorted: Vec<&CanisterId> = targets.iter().collect();
    sorted.sort_by(|a, b| a.as_slice().cmp(b.as_slice()));
    let canonical = CanonicalRevealWire {
        schema: REVEAL_SCHEMA_ID,
        version: REVEAL_VERSION,
        encoding: REVEAL_ENCODING,
        salt: hex::encode(salt),
        record_salt: hex::encode(record_salt),
        targets: sorted.iter().map(|t| t.to_text()).collect(),
    };
    serde_json::to_vec(&canonical).expect("RevealWire canonical serialization")
}

// ---------------------------------------------------------------------------
// Certified receipt tree (spec §2) — heap-resident, rebuilt in post_upgrade.
// ---------------------------------------------------------------------------

/// The certified receipt tree: an `RbTree` keyed by `receipt_id`, whose value at each leaf is
/// the 32-byte `receipt_hash` (so "witness leaf == receipt_hash", spec §6/§9). The whole map is
/// nested under the byte-string label `"receipts"`, giving the frozen path `["receipts", id]`.
/// Heap-resident (IC `certified_data` is cleared on upgrade); `post_upgrade` rebuilds it from
/// the frozen store + in-flight drafts and re-asserts `certified_data_set(root)`.
#[derive(Default)]
pub struct ReceiptTree {
    inner: RbTree<Vec<u8>, Vec<u8>>,
}

impl ReceiptTree {
    /// Insert (or re-insert, idempotent) `receipt_id -> receipt_hash`. Append-only in practice
    /// (receipts are never removed — spec §10).
    pub fn insert(&mut self, receipt_id: &Hash, receipt_hash: &Hash) {
        self.inner.insert(receipt_id.to_vec(), receipt_hash.to_vec());
    }

    /// Certified root over the path `["receipts", receipt_id]`:
    /// `labeled_hash("receipts", inner_root)`. This is the value to publish via `certified_data_set`.
    pub fn root(&self) -> Hash {
        labeled_hash(RECEIPTS_LABEL, &self.inner.root_hash())
    }

    /// Witness for `receipt_id` as IC `HashTree` CBOR bytes, to be stored **byte-for-byte** in
    /// the frozen package (spec §2). Reveals `Leaf(receipt_hash)` under `["receipts", receipt_id]`;
    /// all sibling receipts are pruned. Never re-encoded or regenerated after capture.
    pub fn witness_cbor(&self, receipt_id: &Hash) -> Vec<u8> {
        let witness = labeled(RECEIPTS_LABEL, self.inner.witness(receipt_id));
        // CLAIM (CD to confirm): this is byte-identical to ic-certification's own HashTree CBOR
        // encoding — we drive the SAME `serde_cbor` over the SAME `HashTree` Serialize impl that
        // ic-certification uses internally, so CVDR-Verify decodes it as a stock IC HashTree.
        serde_cbor::to_vec(&witness).expect("receipt-tree HashTree CBOR encode")
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Frozen CVDR package (spec §4) — the immutable stored artifact.
// ---------------------------------------------------------------------------

/// The frozen certificate package (spec §4, HARD — fields/order/types FROZEN; immutable once
/// stored). The official CVDR: served verbatim, never regenerated. Written by the
/// self-finalization store path (Slice 2) / backstop (Slice 3); Slice 1 builds the storage layer.
#[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
pub struct FrozenCvdrPackage {
    /// `RECEIPT_BODY_V1` fixed-width concatenation (spec §2).
    pub receipt_body: Vec<u8>,
    /// `SHA-256(RECEIPT_LEAF_TAG ‖ receipt_body)` — the tree leaf.
    pub receipt_hash: [u8; 32],
    /// Certified receipt-tree root at capture.
    pub tree_root: [u8; 32],
    /// IC `HashTree` CBOR, verbatim from capture.
    pub witness_bytes: Vec<u8>,
    /// IC certificate CBOR, verbatim.
    pub certificate_bytes: Vec<u8>,
    /// Nanoseconds, from the certificate `/time`.
    pub certificate_time: u64,
}

/// §11.3: single constructor from the stored package — never a re-projection of draft fields.
impl From<&FrozenCvdrPackage> for local_user_index_canister::get_cvdr::FrozenWire {
    fn from(p: &FrozenCvdrPackage) -> Self {
        use local_user_index_canister::get_cvdr::{FROZEN_SCHEMA_ID, WIRE_ENCODING, WIRE_VERSION};
        Self {
            schema: FROZEN_SCHEMA_ID.to_string(),
            version: WIRE_VERSION,
            encoding: WIRE_ENCODING.to_string(),
            receipt_body: p.receipt_body.clone(),
            receipt_hash: p.receipt_hash,
            tree_root: p.tree_root,
            witness_bytes: p.witness_bytes.clone(),
            certificate_bytes: p.certificate_bytes.clone(),
            certificate_time: p.certificate_time,
        }
    }
}

/// The operator-facing refusal raised by `post_upgrade` while pre-V2 drafts are in flight (`None`
/// when nothing blocks). Names each blocking receipt by its §11.6 prefix + stage and the way out.
pub fn pre_v2_upgrade_refusal(blockers: &[(String, DraftStage)]) -> Option<String> {
    const MAX_LISTED: usize = 20;
    if blockers.is_empty() {
        return None;
    }
    let listed: Vec<String> = blockers
        .iter()
        .take(MAX_LISTED)
        .map(|(prefix, stage)| format!("{prefix}({stage:?})"))
        .collect();
    let more = blockers.len().saturating_sub(MAX_LISTED);
    Some(format!(
        "upgrade refused: {} in-flight CVDR draft(s) were written by a pre-V2 wasm (RECEIPT_BODY_V1): [{}]{}. \
         Operator path: keep the CURRENT wasm installed; let each listed receipt finalise via the \
         self-finalisation loop, or relay its /cvdr_live package to the permissionless `finalize_cvdr` \
         backstop (Prepared drafts expire after 24 h; Captured/Uninstalled drafts complete via the delete \
         job first); then upgrade again.",
        blockers.len(),
        listed.join(", "),
        if more > 0 { format!(" (+{more} more)") } else { String::new() },
    ))
}

/// §11.6: log at most this truncated prefix (first 8 hex chars = 4 bytes).
pub fn receipt_id_prefix(receipt_id: &Hash) -> String {
    hex::encode(&receipt_id[..4])
}

impl Storable for FrozenCvdrPackage {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("FrozenCvdrPackage encode"))
    }
    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(&self).expect("FrozenCvdrPackage encode")
    }
    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(&bytes).expect("FrozenCvdrPackage decode")
    }
    const BOUND: Bound = Bound::Unbounded;
}

/// Outcome of a frozen-package insert. Insert-only: a second insert for the same `receipt_id`
/// is rejected (immutability, spec §4).
#[derive(Debug, PartialEq, Eq)]
pub enum FrozenInsertError {
    AlreadyExists,
    LogFull,
}

// ---------------------------------------------------------------------------
// Stable storage
// ---------------------------------------------------------------------------

/// 32-byte fixed key (receipt_id).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Key32([u8; 32]);
/// 40-byte fixed key (record_id || deletion_seq big-endian) for the secondary index.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Key40([u8; 40]);

impl Storable for Key32 {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Borrowed(&self.0)
    }
    fn into_bytes(self) -> Vec<u8> {
        self.0.to_vec()
    }
    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        let mut a = [0u8; 32];
        a.copy_from_slice(&bytes);
        Key32(a)
    }
    const BOUND: Bound = Bound::Bounded {
        max_size: 32,
        is_fixed_size: true,
    };
}

impl Storable for Key40 {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Borrowed(&self.0)
    }
    fn into_bytes(self) -> Vec<u8> {
        self.0.to_vec()
    }
    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        let mut a = [0u8; 40];
        a.copy_from_slice(&bytes);
        Key40(a)
    }
    const BOUND: Bound = Bound::Bounded {
        max_size: 40,
        is_fixed_size: true,
    };
}

fn index_key(record_id: &Hash, deletion_seq: u64) -> Key40 {
    let mut k = [0u8; 40];
    k[..32].copy_from_slice(record_id);
    k[32..].copy_from_slice(&deletion_seq.to_be_bytes());
    Key40(k)
}

impl Storable for CvdrDraft {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("CvdrDraft encode"))
    }
    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(&self).expect("CvdrDraft encode")
    }
    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(&bytes).expect("CvdrDraft decode")
    }
    const BOUND: Bound = Bound::Unbounded;
}

/// Frozen-package store (spec §4): immutable blobs in a `StableLog` (slots 8/9) + a primary
/// index `receipt_id -> log offset` (slot 10) + an access-gated secondary index
/// `(record_id, deletion_seq) -> receipt_id` (slot 11). Insert-only; survives upgrades.
pub struct FrozenPackageStore {
    log: StableLog<FrozenCvdrPackage, Memory, Memory>,
    primary: StableBTreeMap<Key32, u64, Memory>,
    secondary: StableBTreeMap<Key40, Key32, Memory>,
}

impl FrozenPackageStore {
    fn new() -> Self {
        FrozenPackageStore {
            log: StableLog::init(get_cvdr_frozen_log_index_memory(), get_cvdr_frozen_log_data_memory()),
            primary: StableBTreeMap::init(get_cvdr_frozen_primary_memory()),
            secondary: StableBTreeMap::init(get_cvdr_frozen_secondary_memory()),
        }
    }

    /// Insert-only (spec §4, immutable). A second insert for the same `receipt_id` is rejected —
    /// the frozen package is never overwritten, mutated, or removed.
    pub fn insert(
        &mut self,
        receipt_id: Hash,
        record_id: Hash,
        deletion_seq: u64,
        package: FrozenCvdrPackage,
    ) -> Result<(), FrozenInsertError> {
        if self.primary.contains_key(&Key32(receipt_id)) {
            return Err(FrozenInsertError::AlreadyExists);
        }
        let offset = self.log.append(&package).map_err(|_| FrozenInsertError::LogFull)?;
        self.primary.insert(Key32(receipt_id), offset);
        self.secondary.insert(index_key(&record_id, deletion_seq), Key32(receipt_id));
        Ok(())
    }

    /// Public path: fetch a stored package by its unguessable `receipt_id`.
    pub fn get_by_receipt_id(&self, receipt_id: &Hash) -> Option<FrozenCvdrPackage> {
        let offset = self.primary.get(&Key32(*receipt_id))?;
        self.log.get(offset)
    }

    /// Gated path: `(record_id, deletion_seq) -> package` (record_id is public-derivable, so not
    /// a public route).
    #[allow(dead_code)]
    pub fn get_by_record(&self, record_id: &Hash, deletion_seq: u64) -> Option<FrozenCvdrPackage> {
        let receipt_id = self.secondary.get(&index_key(record_id, deletion_seq))?;
        self.get_by_receipt_id(&receipt_id.0)
    }

    pub fn count(&self) -> u64 {
        self.primary.len()
    }

    /// `(receipt_id, receipt_hash)` for every stored package — used by `post_upgrade` to rebuild
    /// the heap receipt tree (spec §4 upgrade rule).
    pub fn receipt_leaves(&self) -> Vec<(Hash, Hash)> {
        self.primary
            .iter()
            .filter_map(|e| {
                let receipt_id = e.key().0;
                let package = self.log.get(e.value())?;
                Some((receipt_id, package.receipt_hash))
            })
            .collect()
    }
}

/// Durable, upgrade-surviving CVDR stores: the in-flight draft map (by user_canister_id) and the
/// frozen-package store (spec §4). Both are stable-backed and `#[serde(skip)]`, so a
/// local_user_index upgrade never loses an in-flight deletion or a frozen package.
///
/// The legacy single-CVDR `ReleasedCvdr` store + secondary index (memory slots 5/6) were removed
/// with the finalization rework — the frozen package (spec §4) is the stored artifact, and its
/// public serving is the delivery leg (later slice). Slots 5/6 are retired (kept reserved in the
/// memory map so slot numbering never shifts).
#[derive(Serialize, Deserialize)]
pub struct CvdrStore {
    #[serde(skip, default = "init_drafts")]
    drafts: StableBTreeMap<Principal, CvdrDraft, Memory>,
    #[serde(skip, default = "init_frozen")]
    frozen: FrozenPackageStore,
    #[serde(skip, default = "init_index_evidence")]
    index_evidence: crate::model::cvdr_index_evidence::IndexCodeIdentityStore,
}

impl Default for CvdrStore {
    fn default() -> Self {
        CvdrStore {
            drafts: init_drafts(),
            frozen: init_frozen(),
            index_evidence: init_index_evidence(),
        }
    }
}

fn init_drafts() -> StableBTreeMap<Principal, CvdrDraft, Memory> {
    StableBTreeMap::init(get_cvdr_draft_memory())
}
fn init_frozen() -> FrozenPackageStore {
    FrozenPackageStore::new()
}
fn init_index_evidence() -> crate::model::cvdr_index_evidence::IndexCodeIdentityStore {
    crate::model::cvdr_index_evidence::IndexCodeIdentityStore::new()
}

impl CvdrStore {
    // ---- draft lifecycle (in-flight, keyed by user_canister_id) ----

    pub fn upsert_draft(&mut self, draft: CvdrDraft) {
        // R-1 invariant, enforced at the single write path: `record_salt` is cleared ONLY by
        // `scrub_sensitive` on a terminal draft. A non-terminal draft without it could never have
        // its `record_id` recomputed by the user — fail the message rather than persist it.
        assert!(
            draft.record_salt.is_some() || draft.is_terminal(),
            "CVDR invariant: non-terminal draft {} has no record_salt",
            receipt_id_prefix(&draft.receipt_id)
        );
        self.drafts.insert(draft.user_canister_id, draft);
    }

    pub fn get_draft(&self, user_canister_id: &CanisterId) -> Option<CvdrDraft> {
        self.drafts.get(user_canister_id)
    }

    pub fn remove_draft(&mut self, user_canister_id: &CanisterId) -> Option<CvdrDraft> {
        self.drafts.remove(user_canister_id)
    }

    pub fn draft_count(&self) -> u64 {
        self.drafts.len()
    }

    /// Drafts written by a pre-V2 wasm that are still in flight (see [`CvdrDraft::is_legacy_pre_v2`]).
    #[cfg(test)]
    pub fn legacy_pre_v2_draft_count(&self) -> u64 {
        self.drafts.iter().filter(|e| e.value().is_legacy_pre_v2()).count() as u64
    }

    /// `(receipt_id prefix, stage)` of every in-flight pre-V2 draft — what blocks an upgrade.
    pub fn legacy_pre_v2_blockers(&self) -> Vec<(String, DraftStage)> {
        self.drafts
            .iter()
            .map(|e| e.value())
            .filter(|d| d.is_legacy_pre_v2())
            .map(|d| (receipt_id_prefix(&d.receipt_id), d.stage))
            .collect()
    }

    /// Every in-flight draft. Used post-upgrade to resume deletions the volatile delete
    /// queue lost (the user was popped pre-upgrade) and to rebuild the certified receipt tree.
    pub fn all_drafts(&self) -> Vec<CvdrDraft> {
        self.drafts.iter().map(|e| e.value()).collect()
    }

    // ---- frozen-package store (spec §4) ----

    /// Insert-only store of a frozen package (spec §4). Rejects a duplicate `receipt_id`
    /// (immutability). Written by Slice 2's self-finalization / Slice 3's backstop.
    pub fn insert_frozen_package(
        &mut self,
        receipt_id: Hash,
        record_id: Hash,
        deletion_seq: u64,
        package: FrozenCvdrPackage,
    ) -> Result<(), FrozenInsertError> {
        self.frozen.insert(receipt_id, record_id, deletion_seq, package)
    }

    pub fn get_frozen_package(&self, receipt_id: &Hash) -> Option<FrozenCvdrPackage> {
        self.frozen.get_by_receipt_id(receipt_id)
    }

    pub fn frozen_package_count(&self) -> u64 {
        self.frozen.count()
    }

    /// `(receipt_id, receipt_hash)` for every frozen package — for the post_upgrade tree rebuild.
    pub fn frozen_receipt_leaves(&self) -> Vec<(Hash, Hash)> {
        self.frozen.receipt_leaves()
    }

    /// Receipts past their uninstall, without stored index evidence, whose evidence window is still
    /// open at `now_ns`. This ONE predicate drives both the upgrade interlock (`pre_upgrade` and
    /// `post_upgrade`) and — filtered by the code epoch — the evidence sweep. Computed from durable
    /// draft fields only (drafts are retained after terminal capture), never from heap state.
    pub fn evidence_pending(&self, now_ns: u64) -> Vec<EvidencePending> {
        self.drafts
            .iter()
            .map(|e| e.value())
            .filter(|d| d.is_past_uninstall())
            .filter(|d| now_ns.saturating_sub(d.uninstall_completed_at) <= INDEX_EVIDENCE_WINDOW_NS)
            .filter(|d| !self.index_evidence.contains(&d.receipt_id))
            .map(|d| EvidencePending {
                receipt_id: d.receipt_id,
                stage: d.stage,
                uninstall_completed_at: d.uninstall_completed_at,
            })
            .collect()
    }

    /// The sweep's work list: pending receipts whose uninstall happened under the CURRENTLY
    /// installed code (`uninstall_completed_at >= code_epoch_started_at_ns`). A receipt uninstalled
    /// under an earlier wasm is never given evidence by a later one — that certificate would name
    /// the wrong code. (With the interlock in place such receipts only exist for upgrades from a
    /// wasm that predates it, or after the 24 h window lapsed.)
    pub fn evidence_capturable(&self, now_ns: u64, code_epoch_started_at_ns: u64) -> Vec<EvidencePending> {
        self.evidence_pending(now_ns)
            .into_iter()
            .filter(|p| p.uninstall_completed_at >= code_epoch_started_at_ns)
            .collect()
    }

    pub fn insert_index_evidence(
        &mut self,
        receipt_id: Hash,
        evidence: crate::model::cvdr_index_evidence::IndexCodeIdentityEvidence,
    ) -> Result<(), crate::model::cvdr_index_evidence::IndexEvidenceInsertError> {
        // R-2: evidence is captured from `Uninstalled` onward, usually BEFORE the frozen package
        // exists — it attaches to a post-uninstall draft or to a stored package.
        let known = self.frozen.get_by_receipt_id(&receipt_id).is_some()
            || self.drafts.iter().any(|e| {
                let d = e.value();
                d.receipt_id == receipt_id && d.is_past_uninstall()
            });
        if !known {
            return Err(crate::model::cvdr_index_evidence::IndexEvidenceInsertError::ReceiptMissing);
        }
        self.index_evidence.insert(receipt_id, evidence)
    }

    pub fn get_index_evidence(
        &self,
        receipt_id: &Hash,
    ) -> Option<crate::model::cvdr_index_evidence::IndexCodeIdentityEvidence> {
        self.index_evidence.get(receipt_id)
    }

    pub fn has_index_evidence(&self, receipt_id: &Hash) -> bool {
        self.index_evidence.contains(receipt_id)
    }

    pub fn index_evidence_count(&self) -> u64 {
        self.index_evidence.count()
    }

    // ---- awaiting-certificate drafts (tree design: many may be in-flight; no single slot) ----

    /// Count of in-flight drafts still awaiting a certificate. Replaces the removed single-slot
    /// guard for metrics/observability (spec §1/§2 — the global single-flight guard is gone).
    pub fn awaiting_certificate_count(&self) -> u64 {
        self.drafts
            .iter()
            .filter(|e| e.value().stage == DraftStage::AwaitingCertificate)
            .count() as u64
    }

    /// Every draft still `AwaitingCertificate` — the self-finalization sweep's work list (spec §6).
    pub fn drafts_awaiting_certificate(&self) -> Vec<CvdrDraft> {
        self.drafts
            .iter()
            .map(|e| e.value())
            .filter(|d| d.stage == DraftStage::AwaitingCertificate)
            .collect()
    }

    /// Look up a FINALIZABLE draft by its `receipt_id` (drafts are keyed by `user_canister_id`, so
    /// this scans). Used by the `/cvdr_live/<receipt_id>` fetch route: the §6 self-loop fetches its
    /// own `AwaitingCertificate` receipts, and (spec §7/§10 operator remediation) an operator
    /// fetches a `FailedStuck` receipt's package to relay to the backstop — both are `finalizable`.
    /// A `CertificateCaptured`/`LateFinalized` draft is EXCLUDED: it is scrubbed of the fields
    /// `receipt_body` needs and already has a stored frozen package, so `/cvdr_live` 404s for it.
    pub fn find_draft_by_receipt_id(&self, receipt_id: &Hash) -> Option<CvdrDraft> {
        self.drafts
            .iter()
            .map(|e| e.value())
            .find(|d| &d.receipt_id == receipt_id && d.is_finalizable())
    }

    /// Any draft for `receipt_id` (including scrubbed post-capture). Used by §11.2 Pending
    /// when no frozen package exists yet — Available is checked first so scrubbed+frozen
    /// never stays Pending.
    pub fn find_any_draft_by_receipt_id(&self, receipt_id: &Hash) -> Option<CvdrDraft> {
        self.drafts.iter().map(|e| e.value()).find(|d| &d.receipt_id == receipt_id)
    }

    /// Scrub + drop prepared drafts older than [`PREPARED_DRAFT_TTL_MS`] (spec §11.4).
    /// After purge the `receipt_id` serves Unknown.
    pub fn purge_expired_prepared(&mut self, now: TimestampMillis) -> u64 {
        let expired: Vec<CanisterId> = self
            .drafts
            .iter()
            .filter(|e| {
                let d = e.value();
                d.stage == DraftStage::Prepared && now.saturating_sub(d.created_at) > PREPARED_DRAFT_TTL_MS
            })
            .map(|e| *e.key())
            .collect();
        let n = expired.len() as u64;
        for id in expired {
            if let Some(mut d) = self.drafts.remove(&id) {
                d.scrub_sensitive();
            }
        }
        n
    }

    /// Look up a draft by `receipt_id` that is in a FINALIZABLE state — `AwaitingCertificate` or
    /// `FailedStuck` — for the §7 backstop (rule 1) and the `/cvdr_live` operator-fetch route.
    /// Both states retain the salt + target list needed to recompute `receipt_hash`/`receipt_body`
    /// (only `CertificateCaptured`/`LateFinalized` drafts are scrubbed, and those already have a
    /// stored frozen package). A captured/scrubbed draft is deliberately EXCLUDED. Alias of
    /// [`find_draft_by_receipt_id`]; kept as a distinct name so the backstop reads intent-first.
    pub fn find_finalizable_draft_by_receipt_id(&self, receipt_id: &Hash) -> Option<CvdrDraft> {
        self.find_draft_by_receipt_id(receipt_id)
    }

    /// Count of receipts that reached a terminal self-finalization state, for metrics.
    pub fn finalization_terminal_counts(&self) -> (u64, u64) {
        let mut captured = 0;
        let mut stuck = 0;
        for e in self.drafts.iter() {
            match e.value().stage {
                DraftStage::CertificateCaptured => captured += 1,
                DraftStage::FailedStuck => stuck += 1,
                _ => {}
            }
        }
        (captured, stuck)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(b: u8) -> CanisterId {
        Principal::from_slice(&[b; 10])
    }

    const TEST_RECORD_SALT: [u8; 32] = [0x5A; 32];

    /// Test shorthand: `record_id_v2` under a fixed salt.
    fn record_id_for(user_id: UserId) -> Hash {
        record_id_v2(&TEST_RECORD_SALT, user_id)
    }

    /// R-1: exact formula `SHA-256(RECORD_ID_TAG_V2 ‖ record_salt(32) ‖ principal bytes)`.
    #[test]
    fn record_id_v2_matches_tagged_salted_sha256() {
        let uid: UserId = p(7).into();
        let principal: Principal = uid.into();
        let salt = [0xC3u8; 32];
        let mut pre = Vec::new();
        pre.extend_from_slice(b"OPENCHATZD_RECORD_ID_USER_V2");
        pre.extend_from_slice(&salt);
        pre.extend_from_slice(principal.as_slice());
        assert_eq!(record_id_v2(&salt, uid), sha256::sha256(&pre));
    }

    /// R-1: non-identifying — the same UserId under two salts gives unrelated record_ids, and the
    /// retired unsalted V1 derivation (what anyone holding a public UserId could compute) never
    /// reproduces a V2 record_id.
    #[test]
    fn record_id_v2_is_not_joinable_from_the_user_id_alone() {
        let uid: UserId = p(7).into();
        let principal: Principal = uid.into();
        let a = record_id_v2(&[1u8; 32], uid);
        let b = record_id_v2(&[2u8; 32], uid);
        assert_ne!(a, b, "record_salt must affect record_id");
        let mut v1 = b"OPENCHATZD_RECORD_ID_USER_V1".to_vec();
        v1.extend_from_slice(principal.as_slice());
        assert_ne!(a, sha256::sha256(&v1));
        let mut v2_unsalted = b"OPENCHATZD_RECORD_ID_USER_V2".to_vec();
        v2_unsalted.extend_from_slice(principal.as_slice());
        assert_ne!(a, sha256::sha256(&v2_unsalted));
    }

    /// R-1/R-2/R-4: retired tags are historical only — no live tag may equal one.
    #[test]
    fn retired_tags_are_never_live() {
        let live: [&[u8]; 6] = [
            RECORD_ID_TAG_V2,
            H_USER_TAG,
            RECEIPT_ID_TAG,
            RECEIPT_LEAF_TAG,
            RECEIPT_BODY_TAG,
            TARGETS_COMMITMENT_TAG,
        ];
        for tag in live {
            assert!(
                !RETIRED_TAGS.contains(&tag),
                "live tag {:?} is retired",
                std::str::from_utf8(tag)
            );
        }
        assert_eq!(RECEIPT_BODY_TAG, b"OPENCHATZD_RECEIPT_BODY_V2");
        assert_eq!(RECEIPT_LEAF_TAG, b"OPENCHATZD_RECEIPT_LEAF_V1", "leaf tag unchanged (R-4)");
    }

    #[test]
    fn receipt_id_unguessable_depends_on_nonce() {
        let rid = record_id_for(p(1).into());
        let a = receipt_id_for(&rid, 5, &[0u8; 32]);
        let b = receipt_id_for(&rid, 5, &[1u8; 32]);
        assert_ne!(a, b, "nonce must affect receipt_id");
    }

    // ---- CVDR finalization rework (spec §2/§4) ----

    /// spec §2 TARGETS_COMMITMENT_V1: deterministic (sorted), salted, and matches the frozen formula.
    #[test]
    fn targets_commitment_is_sorted_salted_and_formula_exact() {
        let salt = [9u8; 32];
        let (a, b, c) = (p(3), p(1), p(2));
        let base = targets_commitment(&salt, &[a, b, c]);
        // sort ascending by raw principal bytes -> order-independent input
        assert_eq!(base, targets_commitment(&salt, &[c, a, b]), "must be order-independent");
        // salt-dependent
        assert_ne!(
            base,
            targets_commitment(&[8u8; 32], &[a, b, c]),
            "salt must affect commitment"
        );
        // count 0 is valid; commitment still computed (non-trivial)
        assert_ne!(targets_commitment(&salt, &[]), [0u8; 32]);
        // exact frozen formula: SHA256(TAG || salt || concat(len(u8) || principal_bytes) sorted)
        let mut sorted = vec![a, b, c];
        sorted.sort_by(|x, y| x.as_slice().cmp(y.as_slice()));
        let mut serialized = Vec::new();
        for t in &sorted {
            serialized.push(t.as_slice().len() as u8);
            serialized.extend_from_slice(t.as_slice());
        }
        let mut pre = Vec::new();
        pre.extend_from_slice(TARGETS_COMMITMENT_TAG);
        pre.extend_from_slice(&salt);
        pre.extend_from_slice(&serialized);
        assert_eq!(base, sha256::sha256(&pre));
    }

    /// leaf = SHA256(RECEIPT_LEAF_TAG || RECEIPT_BODY_V2); pins the exact V2 field
    /// ORDER/widths, and confirms the load-bearing timestamps are bound INSIDE the leaf (window
    /// rule anti-spoof).
    #[test]
    fn receipt_leaf_matches_formula_and_binds_timestamps() {
        let record_id = record_id_for(p(1).into());
        let hu = h_user_pre(p(2), &[9u8; 32]);
        let salt = [4u8; 32];
        let targets = vec![p(8), p(6)];
        let tc = targets_commitment(&salt, &targets);
        let nonce = [2u8; 32];
        let index_id = p(3);
        let user_id = p(2);
        let receipt_id = receipt_id_for(&record_id, 5, &nonce);
        let body = receipt_body_v2(
            &receipt_id,
            &nonce,
            index_id,
            user_id,
            &record_id,
            5,
            &hu,
            111,
            222,
            targets.len() as u32,
            &tc,
        );

        // leaf formula, computed independently
        let mut pre = Vec::new();
        pre.extend_from_slice(RECEIPT_LEAF_TAG);
        pre.extend_from_slice(&body);
        assert_eq!(receipt_leaf(&body), sha256::sha256(&pre));

        // EXACT V2 byte layout (R-4), recomputed independently — pins the frozen order. The tag is
        // spelled out so a silent tag edit fails here too.
        let mut expected = Vec::new();
        expected.extend_from_slice(b"OPENCHATZD_RECEIPT_BODY_V2");
        expected.extend_from_slice(&receipt_id);
        expected.extend_from_slice(&nonce);
        // principals are len(u8) ‖ raw bytes (G adjacency ruling)
        expected.push(index_id.as_slice().len() as u8);
        expected.extend_from_slice(index_id.as_slice());
        expected.push(user_id.as_slice().len() as u8);
        expected.extend_from_slice(user_id.as_slice());
        expected.extend_from_slice(&record_id);
        expected.extend_from_slice(&5u64.to_be_bytes());
        expected.extend_from_slice(&hu);
        expected.extend_from_slice(&111u64.to_be_bytes());
        expected.extend_from_slice(&222u64.to_be_bytes());
        expected.extend_from_slice(&(targets.len() as u32).to_be_bytes());
        expected.extend_from_slice(&tc);
        assert_eq!(body, expected, "RECEIPT_BODY_V2 exact frozen byte layout");
        // V2 = V1 − {h_index(32), commitment(32)}: fixed part is tag(26) + 32+32 + 2×(1+10) + 32 + 8 + 32 + 8+8 + 4 + 32.
        assert_eq!(body.len(), 26 + 64 + 22 + 32 + 8 + 32 + 16 + 4 + 32);

        // receipt_committed_at (window anchor) must change the leaf
        let body_ct = receipt_body_v2(
            &receipt_id,
            &nonce,
            index_id,
            user_id,
            &record_id,
            5,
            &hu,
            111,
            999,
            targets.len() as u32,
            &tc,
        );
        assert_ne!(
            receipt_leaf(&body),
            receipt_leaf(&body_ct),
            "receipt_committed_at must bind into the leaf"
        );
        // uninstall_completed_at must change the leaf
        let body_un = receipt_body_v2(
            &receipt_id,
            &nonce,
            index_id,
            user_id,
            &record_id,
            5,
            &hu,
            333,
            222,
            targets.len() as u32,
            &tc,
        );
        assert_ne!(
            receipt_leaf(&body),
            receipt_leaf(&body_un),
            "uninstall_completed_at must bind into the leaf"
        );
    }

    /// spec §2/§9: the witness decodes as an IC HashTree, reconstructs the certified root, and
    /// reveals Leaf(receipt_hash) at path ["receipts", receipt_id]; sibling receipts are pruned.
    #[test]
    fn receipt_tree_witness_reconstructs_root_and_reveals_receipt_hash() {
        use ic_certification::{HashTree, LookupResult};
        let mut tree = ReceiptTree::default();
        let rid_a = receipt_id_for(&record_id_for(p(1).into()), 1, &[1u8; 32]);
        let rid_b = receipt_id_for(&record_id_for(p(2).into()), 2, &[2u8; 32]);
        let hash_a: Hash = [0xAA; 32];
        tree.insert(&rid_a, &hash_a);
        tree.insert(&rid_b, &[0xBB; 32]);

        let root = tree.root();
        let witness_bytes = tree.witness_cbor(&rid_a);
        let witness: HashTree = serde_cbor::from_slice(&witness_bytes).expect("witness decodes as IC HashTree");

        // rule 2: witness root == certified root
        assert_eq!(witness.digest(), root, "witness must reconstruct the certified root");
        // rule 1: witness leaf == receipt_hash, under the frozen path
        match witness.lookup_path([RECEIPTS_LABEL, rid_a.as_slice()]) {
            LookupResult::Found(v) => assert_eq!(v, hash_a.as_slice(), "witness leaf must equal receipt_hash"),
            other => panic!("expected Found(receipt_hash), got {other:?}"),
        }
    }

    /// Shared known-good witness-encoding anchor (spec §2/§9): CD's independent scratch vector —
    /// the ic-certification docs example tree (leaves "hello"/"world"/"good"/"morning"). These are
    /// CD's EXACT CBOR bytes, reproduced byte-for-byte on ic-certification 3.1.0 and 3.2.0. Decoding
    /// them with the SAME `serde_cbor` + `HashTree` path we use for real witnesses must reconstruct
    /// CD's expected digest. This three-way anchors the witness encoding (upstream crate ⇄ CD's runs
    /// ⇄ our decode) so the encoding is not on trust from any single side.
    #[test]
    fn witness_decode_matches_cd_known_good_example_tree() {
        use ic_certification::HashTree;

        // CD's scratch vector, verbatim: CBOR of the IC Interface-Spec certification example tree.
        const CD_EXAMPLE_TREE_CBOR: &str = "8301830183024161830183018302417882034568656c6c6f810083024179820345776f726c6483024162820344676f6f648301830241638100830241648203476d6f726e696e67";
        // CD's expected reconstructed root digest for that tree (identical on crate 3.1.0 / 3.2.0).
        const CD_EXPECTED_DIGEST: &str = "eb5c5b2195e62d996b84c9bcc8259d19a83786a2f59e0878cec84c811f669aa0";

        let cbor = hex::decode(CD_EXAMPLE_TREE_CBOR).unwrap();
        let tree: HashTree = serde_cbor::from_slice(&cbor).expect("CD's example tree must decode as an IC HashTree");
        assert_eq!(
            hex::encode(tree.digest()),
            CD_EXPECTED_DIGEST,
            "our decode of CD's known-good bytes must reconstruct CD's expected digest"
        );
    }

    /// spec §4: the frozen-package store is insert-only — a second insert for the same receipt_id
    /// is rejected and the stored package is never overwritten.
    #[test]
    fn frozen_package_store_is_insert_only() {
        let mut s = CvdrStore::default();
        let record_id = record_id_for(p(1).into());
        let receipt_id = receipt_id_for(&record_id, 1, &[3u8; 32]);
        let pkg = FrozenCvdrPackage {
            receipt_body: vec![1, 2, 3],
            receipt_hash: [4u8; 32],
            tree_root: [5u8; 32],
            witness_bytes: vec![6, 7],
            certificate_bytes: vec![8, 9],
            certificate_time: 1234,
        };
        assert_eq!(s.insert_frozen_package(receipt_id, record_id, 1, pkg.clone()), Ok(()));
        // second insert with the SAME receipt_id is rejected (immutability)
        let mut pkg2 = pkg.clone();
        pkg2.certificate_time = 9999;
        assert_eq!(
            s.insert_frozen_package(receipt_id, record_id, 1, pkg2),
            Err(FrozenInsertError::AlreadyExists)
        );
        // stored package is unchanged, and counted once
        assert_eq!(s.get_frozen_package(&receipt_id).unwrap().certificate_time, 1234);
        assert_eq!(s.frozen_package_count(), 1);
        // rebuild source for post_upgrade: (receipt_id, receipt_hash)
        assert_eq!(s.frozen_receipt_leaves(), vec![(receipt_id, [4u8; 32])]);
    }

    #[test]
    fn index_evidence_store_is_insert_only_via_cvdr_store() {
        use crate::model::cvdr_index_evidence::{IndexCodeIdentityEvidence, IndexEvidenceInsertError};

        let mut s = CvdrStore::default();
        let record_id = record_id_for(p(1).into());
        let receipt_id = receipt_id_for(&record_id, 1, &[3u8; 32]);
        let evidence = IndexCodeIdentityEvidence::new(vec![0xaa, 0xbb], vec![0x1d; 32], "mainnet".to_string());
        assert_eq!(
            s.insert_index_evidence(receipt_id, evidence.clone()),
            Err(IndexEvidenceInsertError::ReceiptMissing)
        );

        let pkg = FrozenCvdrPackage {
            receipt_body: vec![1, 2, 3],
            receipt_hash: [4u8; 32],
            tree_root: [5u8; 32],
            witness_bytes: vec![6, 7],
            certificate_bytes: vec![8, 9],
            certificate_time: 1234,
        };
        assert_eq!(s.insert_frozen_package(receipt_id, record_id, 1, pkg), Ok(()));
        assert_eq!(s.insert_index_evidence(receipt_id, evidence.clone()), Ok(()));
        assert!(s.has_index_evidence(&receipt_id));
        assert_eq!(s.index_evidence_count(), 1);
        assert_eq!(
            s.insert_index_evidence(
                receipt_id,
                IndexCodeIdentityEvidence::new(vec![0xff], vec![0x1d; 32], "mainnet".to_string())
            ),
            Err(IndexEvidenceInsertError::AlreadyExists)
        );
        assert_eq!(s.get_index_evidence(&receipt_id), Some(evidence));
    }

    #[test]
    fn frozen_alone_does_not_imply_index_evidence() {
        let mut s = CvdrStore::default();
        let record_id = record_id_for(p(2).into());
        let receipt_id = receipt_id_for(&record_id, 2, &[9u8; 32]);
        let pkg = FrozenCvdrPackage {
            receipt_body: vec![1],
            receipt_hash: [2u8; 32],
            tree_root: [3u8; 32],
            witness_bytes: vec![4],
            certificate_bytes: vec![5],
            certificate_time: 1,
        };
        assert_eq!(s.insert_frozen_package(receipt_id, record_id, 2, pkg), Ok(()));
        assert!(!s.has_index_evidence(&receipt_id));
        assert_eq!(s.get_index_evidence(&receipt_id), None);
        assert_eq!(s.index_evidence_count(), 0);
    }

    #[test]
    fn index_evidence_requires_matching_receipt_frozen_package() {
        use crate::model::cvdr_index_evidence::{IndexCodeIdentityEvidence, IndexEvidenceInsertError};

        let mut s = CvdrStore::default();
        let record_a = record_id_for(p(3).into());
        let receipt_a = receipt_id_for(&record_a, 1, &[1u8; 32]);
        let receipt_b = receipt_id_for(&record_a, 2, &[2u8; 32]);
        let pkg = FrozenCvdrPackage {
            receipt_body: vec![1],
            receipt_hash: [2u8; 32],
            tree_root: [3u8; 32],
            witness_bytes: vec![4],
            certificate_bytes: vec![5],
            certificate_time: 1,
        };
        assert_eq!(s.insert_frozen_package(receipt_a, record_a, 1, pkg), Ok(()));
        assert_eq!(
            s.insert_index_evidence(
                receipt_b,
                IndexCodeIdentityEvidence::new(vec![0x11], vec![0x1d; 32], "mainnet".to_string())
            ),
            Err(IndexEvidenceInsertError::ReceiptMissing)
        );
        assert!(!s.has_index_evidence(&receipt_a));
        assert!(!s.has_index_evidence(&receipt_b));
    }

    #[test]
    fn index_evidence_empty_cert_rejected_after_frozen_exists() {
        use crate::model::cvdr_index_evidence::{IndexCodeIdentityEvidence, IndexEvidenceInsertError};

        let mut s = CvdrStore::default();
        let record_id = record_id_for(p(4).into());
        let receipt_id = receipt_id_for(&record_id, 1, &[4u8; 32]);
        let pkg = FrozenCvdrPackage {
            receipt_body: vec![1],
            receipt_hash: [2u8; 32],
            tree_root: [3u8; 32],
            witness_bytes: vec![4],
            certificate_bytes: vec![5],
            certificate_time: 1,
        };
        assert_eq!(s.insert_frozen_package(receipt_id, record_id, 1, pkg), Ok(()));
        assert_eq!(
            s.insert_index_evidence(
                receipt_id,
                IndexCodeIdentityEvidence::new(vec![], vec![0x1d; 32], "mainnet".to_string())
            ),
            Err(IndexEvidenceInsertError::EmptyCertificate)
        );
        assert!(!s.has_index_evidence(&receipt_id));
    }

    /// R-2 interlock predicate — truth table over stage × evidence × window × code epoch. ONE
    /// predicate feeds `pre_upgrade`, `post_upgrade` and the evidence sweep.
    #[test]
    fn evidence_pending_truth_table_drives_interlock_and_sweep() {
        use crate::model::cvdr_index_evidence::IndexCodeIdentityEvidence;
        const T0: u64 = 1_000_000_000_000; // uninstall time (ns)
        let draft = |n: u8, stage: DraftStage, uninstalled_at: u64| CvdrDraft {
            user_canister_id: p(n),
            user_id: p(n).into(),
            receipt_id: [n; 32],
            uninstall_completed_at: uninstalled_at,
            record_salt: if matches!(stage, DraftStage::CertificateCaptured | DraftStage::LateFinalized) {
                None
            } else {
                Some([0xC3; 32])
            },
            ..v2_draft(stage, Some([0xC3; 32]))
        };
        let mut s = CvdrStore::default();
        s.upsert_draft(draft(1, DraftStage::Prepared, 0)); // nothing uninstalled yet
        s.upsert_draft(draft(2, DraftStage::Captured, 0)); // legacy pre-uninstall stage
        s.upsert_draft(draft(3, DraftStage::Uninstalled, T0));
        s.upsert_draft(draft(4, DraftStage::AwaitingCertificate, T0));
        s.upsert_draft(draft(5, DraftStage::FailedStuck, T0));
        s.upsert_draft(draft(6, DraftStage::CertificateCaptured, T0)); // package can precede evidence
        s.upsert_draft(draft(7, DraftStage::LateFinalized, T0));

        let ids = |v: Vec<EvidencePending>| {
            let mut ids: Vec<u8> = v.iter().map(|b| b.receipt_id[0]).collect();
            ids.sort();
            ids
        };
        // inside the window, no evidence: every post-uninstall receipt blocks; Prepared/Captured never do
        assert_eq!(ids(s.evidence_pending(T0 + 1)), vec![3, 4, 5, 6, 7]);
        assert_eq!(
            ids(s.evidence_pending(T0 + INDEX_EVIDENCE_WINDOW_NS)),
            vec![3, 4, 5, 6, 7],
            "window is inclusive"
        );
        // evidence stored for a DRAFT-only receipt (no frozen package needed any more) => unblocks it
        assert_eq!(
            s.insert_index_evidence(
                [4; 32],
                IndexCodeIdentityEvidence::new(vec![0xaa], vec![0x1d; 32], "mainnet".to_string())
            ),
            Ok(())
        );
        assert_eq!(ids(s.evidence_pending(T0 + 1)), vec![3, 5, 6, 7]);
        // ... but never for a receipt that has not uninstalled, or an unknown one
        for unknown in [[1u8; 32], [2u8; 32], [0xEE; 32]] {
            assert_eq!(
                s.insert_index_evidence(
                    unknown,
                    IndexCodeIdentityEvidence::new(vec![0xaa], vec![0x1d; 32], "mainnet".to_string())
                ),
                Err(crate::model::cvdr_index_evidence::IndexEvidenceInsertError::ReceiptMissing)
            );
        }
        // window lapsed => nothing blocks (the receipt stays V3A-UNAVAILABLE; blocking protects nothing)
        assert!(s.evidence_pending(T0 + INDEX_EVIDENCE_WINDOW_NS + 1).is_empty());
        // code epoch: receipts uninstalled BEFORE the current wasm started are neither captured nor blocking
        assert_eq!(
            ids(s.evidence_capturable(T0 + 1, 0)),
            vec![3, 5, 6, 7],
            "epoch 0 = conservative"
        );
        assert_eq!(
            ids(s.evidence_capturable(T0 + 1, T0)),
            vec![3, 5, 6, 7],
            "uninstalled at epoch start counts"
        );
        assert!(s.evidence_capturable(T0 + 2, T0 + 1).is_empty());
    }

    /// The interlock refusal names each blocker (prefix + stage) and the operator path.
    #[test]
    fn evidence_upgrade_refusal_lists_blockers_and_operator_path() {
        assert_eq!(evidence_upgrade_refusal(&[]), None);
        let blocker = |n: u8, stage| EvidencePending {
            receipt_id: [n; 32],
            stage,
            uninstall_completed_at: 1,
        };
        let msg = evidence_upgrade_refusal(&[
            blocker(0xa7, DraftStage::AwaitingCertificate),
            blocker(0x0b, DraftStage::Uninstalled),
        ])
        .unwrap();
        for needle in [
            "upgrade refused: 2 CVDR receipt(s)",
            "protected deletion→evidence interval",
            "a7a7a7a7(AwaitingCertificate)",
            "0b0b0b0b(Uninstalled)",
            "Operator path",
            "cvdr_upgrade_blockers",
            "then upgrade again",
        ] {
            assert!(msg.contains(needle), "refusal must contain `{needle}`: {msg}");
        }
        let many: Vec<_> = (0..23).map(|i| blocker(i, DraftStage::FailedStuck)).collect();
        assert!(evidence_upgrade_refusal(&many).unwrap().contains("(+3 more)"));
    }

    // ---- Slice 2: §6 store-gate verification, proven with REAL mainnet A1 certificate bytes ----

    /// The reused BLS -> subnet delegation -> NNS -> `certified_data` verification path accepts a
    /// GENUINE mainnet certificate (captured from the A1 spike canister on subnet nl6hn…) and
    /// rejects each tamper class with its OWN distinct [`CertRejectReason`] — the D2 honest-taxonomy
    /// fix (the earlier `Option::None` collapse is gone; each check is now separately assertable).
    /// The OpenChat root/witness/receipt_hash binding is exercised by the next test + PocketIC (a
    /// cert over OUR root needs a real signature).
    #[test]
    fn verify_certificate_accepts_real_mainnet_cert_and_rejects_tampering_with_distinct_reasons() {
        use ic_cbor::CertificateToCbor;
        use ic_certification::{Certificate, LookupResult};

        const CERT: &[u8] = include_bytes!("testdata/A1_certificate.bin");
        let self_id = Principal::from_text("iplx4-aiaaa-aaaap-quuuq-cai").unwrap();

        // The fixture is days old; verify at its OWN `/time` (freshness is `now`-relative).
        let cert = Certificate::from_cbor(CERT).unwrap();
        let time_ns = match cert.tree.lookup_path([b"time".as_ref()]) {
            LookupResult::Found(t) => leb128_u64(t),
            _ => panic!("no /time in fixture cert"),
        };
        let now_ms = time_ns / 1_000_000;

        // POSITIVE: real subnet BLS + delegation + certified_data extraction + time.
        let v = verify_certificate(CERT, self_id, constants::IC_ROOT_KEY, now_ms)
            .expect("a genuine mainnet certificate must verify against the NNS root");
        assert_eq!(
            hex::encode(&v.certified_data),
            "0e2812673391d7f7b6f3ab4edb45024988e91e55ba58b2b0f6137f4d6ea1d8d3",
            "certified_data == the A1 canister's certified root"
        );
        assert_eq!(v.cert_time_ns, time_ns, "cert /time round-trips");

        // INDEX path: commitment certificates do not carry `/canister/.../module_hash`.
        assert_eq!(
            verify_index_module_hash_evidence(CERT, self_id, constants::IC_ROOT_KEY, now_ms, None),
            Err(IndexEvidenceRejectReason::ModuleHashMissing)
        );
        assert_eq!(
            verify_index_module_hash_evidence(&[0xde, 0xad, 0xbe, 0xef], self_id, constants::IC_ROOT_KEY, now_ms, None),
            Err(IndexEvidenceRejectReason::Certificate(CertRejectReason::CborDecodeFailed))
        );

        // NEGATIVE (distinct reasons):
        // (a) garbage bytes — not a decodable certificate.
        assert_eq!(
            verify_certificate(&[0xde, 0xad, 0xbe, 0xef], self_id, constants::IC_ROOT_KEY, now_ms),
            Err(CertRejectReason::CborDecodeFailed),
            "garbage bytes -> CborDecodeFailed"
        );

        // (b) tampered signature — structure still decodes, BLS fails.
        let mut tampered = CERT.to_vec();
        let mid = tampered.len() / 2;
        tampered[mid] ^= 0xFF;
        assert_eq!(
            verify_certificate(&tampered, self_id, constants::IC_ROOT_KEY, now_ms),
            Err(CertRejectReason::SignatureInvalid),
            "flipped byte -> SignatureInvalid"
        );

        // (c) wrong canister id — not in the delegated subnet's ranges.
        let other = Principal::from_slice(&[0u8; 10]);
        assert_eq!(
            verify_certificate(CERT, other, constants::IC_ROOT_KEY, now_ms),
            Err(CertRejectReason::CanisterNotInRange),
            "wrong canister -> CanisterNotInRange"
        );

        // (d) stale — `now` beyond the max offset from the certificate time.
        assert_eq!(
            verify_certificate(CERT, self_id, constants::IC_ROOT_KEY, now_ms + 10 * 60 * 1000),
            Err(CertRejectReason::Stale),
            "expired freshness -> Stale"
        );

        // Independent range-auth taxonomy must stay distinct (Stef B1 / D2).
        assert_ne!(
            CertRejectReason::CanisterNotInRange.as_str(),
            CertRejectReason::CanisterRangesMissing.as_str()
        );
        assert_ne!(
            CertRejectReason::CanisterNotInRange.as_str(),
            CertRejectReason::CanisterRangesMalformed.as_str()
        );
        assert_eq!(
            map_range_auth_error(crate::model::cvdr_canister_ranges::RangeAuthError::NotInRange),
            CertRejectReason::CanisterNotInRange
        );
        assert_eq!(
            map_range_auth_error(crate::model::cvdr_canister_ranges::RangeAuthError::RangesMissing),
            CertRejectReason::CanisterRangesMissing
        );
        assert_eq!(
            map_range_auth_error(crate::model::cvdr_canister_ranges::RangeAuthError::Malformed),
            CertRejectReason::CanisterRangesMalformed
        );
    }

    /// Positive store-gate: genuine mainnet `read_state` certificate for
    /// `/canister/<OpenChat LUI>/module_hash` verifies against the NNS root and
    /// yields a non-empty module hash + cert time.
    #[test]
    fn verify_index_module_hash_accepts_real_mainnet_module_hash_cert() {
        use ic_cbor::CertificateToCbor;
        use ic_certification::{Certificate, LookupResult};

        const CERT: &[u8] = include_bytes!("testdata/mainnet_module_hash_certificate.bin");
        let self_id = Principal::from_text("nq4qv-wqaaa-aaaaf-bhdgq-cai").unwrap();
        let cert = Certificate::from_cbor(CERT).unwrap();
        let time_ns = match cert.tree.lookup_path([b"time".as_ref()]) {
            LookupResult::Found(t) => leb128_u64(t),
            _ => panic!("no /time in mainnet module_hash fixture"),
        };
        let now_ms = time_ns / 1_000_000;

        let v = verify_index_module_hash_evidence(CERT, self_id, constants::IC_ROOT_KEY, now_ms, None)
            .expect("mainnet module_hash certificate must verify");
        assert_eq!(v.cert_time_ns, time_ns);
        assert_eq!(v.module_hash.len(), 32, "WASM module hash is 32 bytes");
        assert_ne!(v.module_hash, vec![0u8; 32]);

        // Ordering gate (R-2): a certificate older than the uninstall → reject.
        assert_eq!(
            verify_index_module_hash_evidence(CERT, self_id, constants::IC_ROOT_KEY, now_ms, Some(time_ns.saturating_add(1)),),
            Err(IndexEvidenceRejectReason::CertTimeBeforeUninstall)
        );
        // Certificate time equal to the uninstall time is accepted.
        assert!(verify_index_module_hash_evidence(CERT, self_id, constants::IC_ROOT_KEY, now_ms, Some(time_ns),).is_ok());

        // Wrong canister → not in delegated range (or path miss under that id).
        let other = Principal::from_slice(&[0u8; 10]);
        assert!(matches!(
            verify_index_module_hash_evidence(CERT, other, constants::IC_ROOT_KEY, now_ms, None),
            Err(IndexEvidenceRejectReason::Certificate(_)) | Err(IndexEvidenceRejectReason::ModuleHashMissing)
        ));
    }

    /// HARD RULE (spec §6): the store-gate must REJECT (never Store) a fully verified certificate
    /// whose witness does not bind OUR receipt. The A1 witness is over the A1 canister's own 2-leaf
    /// tree (key "cvdr"), so it carries no `["receipts", receipt_id]` leaf.
    #[test]
    fn store_gate_rejects_cert_not_binding_our_receipt() {
        use ic_cbor::CertificateToCbor;
        use ic_certification::{Certificate, LookupResult};

        const CERT: &[u8] = include_bytes!("testdata/A1_certificate.bin");
        const WITNESS: &[u8] = include_bytes!("testdata/A1_witness.cbor");
        let self_id = Principal::from_text("iplx4-aiaaa-aaaap-quuuq-cai").unwrap();
        let cert = Certificate::from_cbor(CERT).unwrap();
        let time_ns = match cert.tree.lookup_path([b"time".as_ref()]) {
            LookupResult::Found(t) => leb128_u64(t),
            _ => panic!(),
        };
        let now_ms = time_ns / 1_000_000;

        let receipt_id = receipt_id_for(&record_id_for(p(1).into()), 1, &[9u8; 32]);
        let receipt_hash = [0x11u8; 32];
        // The A1 witness is over the A1 canister's own "cvdr" tree, so it carries no
        // `["receipts", receipt_id]` leaf: distinct reason `WitnessLeafNeReceiptHash` (not a
        // blanket failure — the D2 honest-taxonomy fix). (The witness DOES reconstruct the A1 root
        // == the cert's certified_data, so it passes the root check and fails at the leaf.)
        match verify_finalization_package(
            CERT,
            WITNESS,
            &receipt_id,
            &receipt_hash,
            self_id,
            constants::IC_ROOT_KEY,
            now_ms,
            0,
        ) {
            FinalizeVerdict::Reject(FinalizeRejectReason::WitnessLeafNeReceiptHash) => {}
            FinalizeVerdict::Reject(r) => panic!("expected witness_leaf_ne_receipt_hash, got Reject({})", r.as_str()),
            FinalizeVerdict::InWindow { .. } | FinalizeVerdict::Late { .. } => {
                panic!("store-gate must NOT store a cert that doesn't bind our receipt")
            }
        }
    }

    /// Store-gate reject MATRIX (D2 honest taxonomy): each failure class the A1 fixture can reach
    /// maps to its OWN [`FinalizeRejectReason`], not a blanket failure. The window-tier reasons
    /// (`CertTimeBeforeReceiptCommitted`) and the `InWindow`/`Late` split need a real certificate
    /// over OUR receipts-tree root (a real signature), so they are exercised at the PocketIC /
    /// backstop-integration level; here we pin the cert-stage and witness-stage reasons.
    #[test]
    fn store_gate_reject_reasons_are_distinct() {
        use ic_cbor::CertificateToCbor;
        use ic_certification::{Certificate, LookupResult};

        const CERT: &[u8] = include_bytes!("testdata/A1_certificate.bin");
        const WITNESS: &[u8] = include_bytes!("testdata/A1_witness.cbor");
        // A valid HashTree CBOR whose digest is NOT the A1 root (the ic-certification docs example).
        const OTHER_TREE_CBOR: &str = "8301830183024161830183018302417882034568656c6c6f810083024179820345776f726c6483024162820344676f6f648301830241638100830241648203476d6f726e696e67";
        let self_id = Principal::from_text("iplx4-aiaaa-aaaap-quuuq-cai").unwrap();
        let cert = Certificate::from_cbor(CERT).unwrap();
        let time_ns = match cert.tree.lookup_path([b"time".as_ref()]) {
            LookupResult::Found(t) => leb128_u64(t),
            _ => panic!(),
        };
        let now_ms = time_ns / 1_000_000;
        let rid = receipt_id_for(&record_id_for(p(1).into()), 1, &[9u8; 32]);
        let rh = [0x11u8; 32];

        // Extract the reject reason or fail loudly (no store verdict is acceptable here).
        let reason = |cert: &[u8], witness: &[u8], self_id: Principal, now: TimestampMillis| -> FinalizeRejectReason {
            match verify_finalization_package(cert, witness, &rid, &rh, self_id, constants::IC_ROOT_KEY, now, 0) {
                FinalizeVerdict::Reject(r) => r,
                FinalizeVerdict::InWindow { .. } | FinalizeVerdict::Late { .. } => panic!("expected a reject verdict"),
            }
        };

        // cert-stage reasons (wrapped through FinalizeRejectReason::Certificate)
        assert_eq!(
            reason(&[0xde, 0xad], WITNESS, self_id, now_ms),
            FinalizeRejectReason::Certificate(CertRejectReason::CborDecodeFailed)
        );
        let mut tampered = CERT.to_vec();
        let mid = tampered.len() / 2;
        tampered[mid] ^= 0xFF;
        assert_eq!(
            reason(&tampered, WITNESS, self_id, now_ms),
            FinalizeRejectReason::Certificate(CertRejectReason::SignatureInvalid)
        );
        assert_eq!(
            reason(CERT, WITNESS, Principal::from_slice(&[0u8; 10]), now_ms),
            FinalizeRejectReason::Certificate(CertRejectReason::CanisterNotInRange)
        );
        assert_eq!(
            reason(CERT, WITNESS, self_id, now_ms + 10 * 60 * 1000),
            FinalizeRejectReason::Certificate(CertRejectReason::Stale)
        );

        // witness-stage reasons (cert verifies; the witness fails to bind our receipt)
        assert_eq!(
            reason(CERT, &[0x00, 0x01, 0x02], self_id, now_ms),
            FinalizeRejectReason::WitnessDecodeFailed
        );
        let other_witness = hex::decode(OTHER_TREE_CBOR).unwrap();
        assert_eq!(
            reason(CERT, &other_witness, self_id, now_ms),
            FinalizeRejectReason::WitnessRootNeCertifiedData
        );
        // A1 witness reconstructs the A1 root but has no ["receipts", rid] leaf -> leaf mismatch.
        assert_eq!(
            reason(CERT, WITNESS, self_id, now_ms),
            FinalizeRejectReason::WitnessLeafNeReceiptHash
        );
    }

    /// spec §6 privacy: capture scrubs the salt + raw cleanup-target list from the retained draft,
    /// keeping the bookkeeping fields (stage, attempts, timestamps, ids).
    #[test]
    fn scrub_sensitive_clears_salt_and_targets_keeps_bookkeeping() {
        let mut draft = CvdrDraft {
            user_id: p(1).into(),
            user_canister_id: p(1),
            index_canister_id: p(2),
            record_salt: Some([0x5Au8; 32]),
            record_id: [1u8; 32],
            deletion_seq: 7,
            nonce: [2u8; 32],
            receipt_id: [3u8; 32],
            module_hash_pre: vec![9],
            h_user_pre: [4u8; 32],
            salt: [0xAB; 32],
            canisters_to_notify: vec![p(3), p(4)],
            uninstall_completed_at: 111,
            receipt_committed_at: 222,
            finalize_attempt: 5,
            finalize_last_attempt_at: 333,
            created_at: 100,
            attempt: 2,
            stage: DraftStage::CertificateCaptured,
        };
        draft.scrub_sensitive();
        // sensitive fields gone
        assert_eq!(draft.salt, [0u8; 32], "salt scrubbed");
        assert_eq!(
            draft.record_salt, None,
            "record_salt scrubbed (R-1: never retained server-side)"
        );
        assert!(!draft.is_legacy_pre_v2(), "a scrubbed TERMINAL draft is not a legacy blocker");
        assert!(draft.canisters_to_notify.is_empty(), "raw target list scrubbed");
        // bookkeeping kept
        assert_eq!(draft.uninstall_completed_at, 111);
        assert_eq!(draft.receipt_committed_at, 222);
        assert_eq!(draft.finalize_attempt, 5);
        assert_eq!(draft.finalize_last_attempt_at, 333);
        assert_eq!(draft.stage, DraftStage::CertificateCaptured);
        assert_eq!(draft.receipt_id, [3u8; 32]);
    }

    #[test]
    fn reveal_wire_json_is_sorted_hex_and_schema_tagged() {
        let salt = [0xABu8; 32];
        let a = p(3);
        let b = p(1);
        let record_salt = [0xC3u8; 32];
        let json = reveal_wire_canonical_json(&salt, &record_salt, &[a, b]);
        let v: serde_json::Value = serde_json::from_slice(&json).unwrap();
        assert_eq!(v["schema"], REVEAL_SCHEMA_ID);
        assert_eq!(v["version"], 2, "RevealWire v2 carries record_salt (R-1)");
        assert_eq!(v["encoding"], REVEAL_ENCODING);
        assert_eq!(v["salt"], hex::encode(salt));
        assert_eq!(v["record_salt"], hex::encode(record_salt));
        // canonical field order is part of the wire contract
        let text = String::from_utf8(json.clone()).unwrap();
        let pos = |k: &str| text.find(&format!("\"{k}\":")).unwrap_or_else(|| panic!("missing {k}"));
        let order = ["schema", "version", "encoding", "salt", "record_salt", "targets"];
        assert!(
            order.windows(2).all(|w| pos(w[0]) < pos(w[1])),
            "RevealWire field order: {text}"
        );
        let targets = v["targets"].as_array().unwrap();
        assert_eq!(targets.len(), 2);
        // sorted by raw principal bytes: p(1) before p(3)
        assert_eq!(targets[0], b.to_text());
        assert_eq!(targets[1], a.to_text());
        assert_eq!(
            targets_commitment(&salt, &[a, b]),
            targets_commitment(&salt, &[b, a]),
            "reveal sort must match commitment sort"
        );
    }

    /// R-1 linkage: the RevealWire `record_salt` + the user's own UserId recompute the public
    /// `record_id`, and with `deletion_seq` + `nonce` from the body, the `receipt_id`.
    #[test]
    fn reveal_record_salt_recomputes_record_id_and_receipt_id() {
        let uid: UserId = p(1).into();
        let record_salt = [0xC3u8; 32];
        let record_id = record_id_v2(&record_salt, uid);
        let nonce = [2u8; 32];
        let draft = CvdrDraft {
            user_id: uid,
            user_canister_id: p(1),
            index_canister_id: p(2),
            record_salt: Some(record_salt),
            record_id,
            deletion_seq: 4,
            nonce,
            receipt_id: receipt_id_for(&record_id, 4, &nonce),
            module_hash_pre: vec![9],
            h_user_pre: h_user_pre(p(1), &[9]),
            salt: [0xAB; 32],
            canisters_to_notify: vec![p(3)],
            uninstall_completed_at: 111,
            receipt_committed_at: 222,
            finalize_attempt: 0,
            finalize_last_attempt_at: 0,
            created_at: 1,
            attempt: 0,
            stage: DraftStage::AwaitingCertificate,
        };
        let reveal: serde_json::Value = serde_json::from_slice(&draft.reveal_wire_json()).unwrap();
        let revealed: [u8; 32] = hex::decode(reveal["record_salt"].as_str().unwrap())
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(record_id_v2(&revealed, uid), draft.record_id);
        // the public body never contains the salt
        let body = draft.receipt_body();
        assert!(
            !body.windows(32).any(|w| w == record_salt),
            "record_salt must not appear in the receipt body"
        );
        assert!(
            body.windows(32).any(|w| w == draft.record_id),
            "record_id is displayed in the body"
        );
    }

    /// Stored-struct upgrade (Candid, stable `StableBTreeMap`): a draft written by the pre-V2 wasm
    /// (with `executor_module_hash`/`h_index`/`commitment`, without `record_salt`) still DECODES —
    /// so the refusal below is an explicit decision, never a decode trap — and is flagged legacy
    /// unless terminal.
    #[test]
    fn pre_v2_stored_draft_decodes_and_is_flagged_legacy() {
        #[derive(CandidType, Serialize)]
        struct PreV2Draft {
            user_id: UserId,
            user_canister_id: CanisterId,
            index_canister_id: CanisterId,
            record_id: Hash,
            deletion_seq: u64,
            nonce: [u8; 32],
            receipt_id: Hash,
            module_hash_pre: Vec<u8>,
            executor_module_hash: Vec<u8>,
            h_user_pre: Hash,
            h_index: Hash,
            commitment: Hash,
            salt: [u8; 32],
            canisters_to_notify: Vec<CanisterId>,
            uninstall_completed_at: u64,
            receipt_committed_at: u64,
            finalize_attempt: u32,
            finalize_last_attempt_at: u64,
            created_at: TimestampMillis,
            attempt: u32,
            stage: DraftStage,
        }
        let old = |stage| PreV2Draft {
            user_id: p(1).into(),
            user_canister_id: p(1),
            index_canister_id: p(2),
            record_id: [1u8; 32],
            deletion_seq: 7,
            nonce: [2u8; 32],
            receipt_id: [3u8; 32],
            module_hash_pre: vec![9],
            executor_module_hash: vec![8; 32],
            h_user_pre: [4u8; 32],
            h_index: [5u8; 32],
            commitment: [6u8; 32],
            salt: [0xAB; 32],
            canisters_to_notify: vec![p(3)],
            uninstall_completed_at: 111,
            receipt_committed_at: 222,
            finalize_attempt: 1,
            finalize_last_attempt_at: 333,
            created_at: 100,
            attempt: 2,
            stage,
        };
        for (stage, legacy) in [
            (DraftStage::Prepared, true),
            (DraftStage::Captured, true),
            (DraftStage::Uninstalled, true),
            (DraftStage::AwaitingCertificate, true),
            (DraftStage::FailedStuck, true),
            (DraftStage::CertificateCaptured, false),
            (DraftStage::LateFinalized, false),
        ] {
            let bytes = candid::encode_one(old(stage)).unwrap();
            let draft = CvdrDraft::from_bytes(Cow::Owned(bytes));
            assert_eq!(draft.record_salt, None);
            assert_eq!((draft.deletion_seq, draft.receipt_id, draft.stage), (7, [3u8; 32], stage));
            assert_eq!(draft.is_legacy_pre_v2(), legacy, "{stage:?}");
        }
        // a V2 draft round-trips with its salt and is never legacy
        let mut store = CvdrStore::default();
        let v2 = CvdrDraft::from_bytes(Cow::Owned(candid::encode_one(old(DraftStage::Prepared)).unwrap()));
        let v2 = CvdrDraft {
            record_salt: Some([0xC3; 32]),
            ..v2
        };
        store.upsert_draft(v2);
        let back = store.get_draft(&p(1)).unwrap();
        assert_eq!(back.record_salt, Some([0xC3; 32]));
        assert!(!back.is_legacy_pre_v2());
        assert_eq!(store.legacy_pre_v2_draft_count(), 0);
    }

    fn v2_draft(stage: DraftStage, record_salt: Option<[u8; 32]>) -> CvdrDraft {
        CvdrDraft {
            user_id: p(1).into(),
            user_canister_id: p(1),
            index_canister_id: p(2),
            record_salt,
            record_id: [1u8; 32],
            deletion_seq: 1,
            nonce: [2u8; 32],
            receipt_id: [0xA7u8; 32],
            module_hash_pre: vec![],
            h_user_pre: [0u8; 32],
            salt: [0xCDu8; 32],
            canisters_to_notify: vec![p(5)],
            uninstall_completed_at: 0,
            receipt_committed_at: 0,
            finalize_attempt: 0,
            finalize_last_attempt_at: 0,
            created_at: 1,
            attempt: 0,
            stage,
        }
    }

    const NON_TERMINAL: [DraftStage; 5] = [
        DraftStage::Prepared,
        DraftStage::Captured,
        DraftStage::Uninstalled,
        DraftStage::AwaitingCertificate,
        DraftStage::FailedStuck,
    ];

    /// R-1 invariant: a NON-TERMINAL V2 draft never has `record_salt == None`. Every non-terminal
    /// stage persists and reloads with its salt; the salt survives stage transitions; only
    /// `scrub_sensitive` (terminal capture) clears it; and the single write path refuses to persist
    /// a non-terminal draft without one.
    #[test]
    fn non_terminal_v2_draft_always_carries_record_salt() {
        let mut store = CvdrStore::default();
        let salt = Some([0xC3u8; 32]);
        let mut draft = v2_draft(DraftStage::Prepared, salt);
        for stage in NON_TERMINAL {
            draft.stage = stage;
            store.upsert_draft(draft.clone());
            let back = store.get_draft(&p(1)).unwrap();
            assert_eq!(back.record_salt, salt, "{stage:?} keeps the prepare-time record_salt");
            assert!(!back.is_terminal() && !back.is_legacy_pre_v2());
        }
        assert_eq!(store.legacy_pre_v2_draft_count(), 0);
        // terminal capture is the only place the salt goes away
        draft.stage = DraftStage::CertificateCaptured;
        draft.scrub_sensitive();
        store.upsert_draft(draft);
        assert_eq!(store.get_draft(&p(1)).unwrap().record_salt, None);
        assert_eq!(store.legacy_pre_v2_draft_count(), 0);
    }

    #[test]
    fn write_path_refuses_a_non_terminal_draft_without_record_salt() {
        for stage in NON_TERMINAL {
            let refused = std::panic::catch_unwind(|| {
                let mut store = CvdrStore::default();
                store.upsert_draft(v2_draft(stage, None));
            });
            assert!(refused.is_err(), "{stage:?} without record_salt must not be persisted");
        }
    }

    /// The upgrade refusal names each blocker (prefix + stage) and the operator path.
    #[test]
    fn pre_v2_upgrade_refusal_lists_blockers_and_operator_path() {
        assert_eq!(pre_v2_upgrade_refusal(&[]), None);
        let blockers = vec![
            ("a7a7a7a7".to_string(), DraftStage::AwaitingCertificate),
            ("0badf00d".to_string(), DraftStage::FailedStuck),
        ];
        let msg = pre_v2_upgrade_refusal(&blockers).unwrap();
        for needle in [
            "upgrade refused: 2 in-flight",
            "pre-V2",
            "a7a7a7a7(AwaitingCertificate)",
            "0badf00d(FailedStuck)",
            "self-finalisation loop",
            "finalize_cvdr",
            "then upgrade again",
        ] {
            assert!(msg.contains(needle), "refusal must contain `{needle}`: {msg}");
        }
        let many: Vec<_> = (0..25).map(|i| (format!("{i:08x}"), DraftStage::Prepared)).collect();
        assert!(pre_v2_upgrade_refusal(&many).unwrap().contains("(+5 more)"));
    }

    /// R-6: `trust_root_key_id` comes from the configured root key only — mainnet for the IC NNS
    /// key, a self-labelled non-production id for anything else (never an empty/unknown id).
    #[test]
    fn trust_root_key_id_is_decided_by_configuration() {
        assert_eq!(trust_root_key_id_for(constants::IC_ROOT_KEY), "mainnet");
        assert_eq!(trust_root_key_id_for(&[1, 2, 3]), "non-production-test-root");
        assert_eq!(trust_root_key_id_for(&[]), "non-production-test-root");
        let mut almost = constants::IC_ROOT_KEY.to_vec();
        almost[10] ^= 1;
        assert_eq!(trust_root_key_id_for(&almost), "non-production-test-root");
    }

    #[test]
    fn purge_expired_prepared_scrubs_and_drops() {
        let mut store = CvdrStore::default();
        let mut draft = CvdrDraft {
            user_id: p(1).into(),
            user_canister_id: p(1),
            index_canister_id: p(2),
            record_salt: Some([0x5Au8; 32]),
            record_id: [1u8; 32],
            deletion_seq: 1,
            nonce: [2u8; 32],
            receipt_id: [9u8; 32],
            module_hash_pre: vec![],
            h_user_pre: [0u8; 32],
            salt: [0xCDu8; 32],
            canisters_to_notify: vec![p(5)],
            uninstall_completed_at: 0,
            receipt_committed_at: 0,
            finalize_attempt: 0,
            finalize_last_attempt_at: 0,
            created_at: 1,
            attempt: 0,
            stage: DraftStage::Prepared,
        };
        store.upsert_draft(draft.clone());
        assert_eq!(store.purge_expired_prepared(1 + PREPARED_DRAFT_TTL_MS), 0);
        assert!(store.get_draft(&p(1)).is_some());
        assert_eq!(store.purge_expired_prepared(2 + PREPARED_DRAFT_TTL_MS), 1);
        assert!(store.get_draft(&p(1)).is_none());
        assert!(store.find_any_draft_by_receipt_id(&draft.receipt_id).is_none());
        // fresh prepared not purged
        draft.created_at = 2 + PREPARED_DRAFT_TTL_MS;
        draft.user_canister_id = p(7);
        draft.receipt_id = [8u8; 32];
        store.upsert_draft(draft);
        assert_eq!(store.purge_expired_prepared(2 + PREPARED_DRAFT_TTL_MS), 0);
        assert!(store.get_draft(&p(7)).is_some());
    }
}
