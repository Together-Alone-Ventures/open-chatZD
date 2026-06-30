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
//! ## V1 — exact field set + hash order (PINNED; bump [`CVDR_ENCODER_VERSION`] + the
//!         tag for ANY change)
//! All hashes are domain-separated SHA-256 over a fixed, length-stable preimage:
//! - `record_id   = SHA-256(RECORD_ID_TAG || canonical UserId principal bytes)`
//!   — byte-identical to the user canister's `mktd::record_id_for`; never `caller()`.
//! - `h_user_pre  = SHA-256(H_USER_TAG  || user_canister_principal || module_hash_pre)`
//!   — TARGET provenance: WHICH canister was de-referenced and WHAT code it ran pre-uninstall.
//! - `h_index     = SHA-256(H_INDEX_TAG || index_canister_principal || executor_module_hash)`
//!   — EXECUTOR provenance: WHICH index performed the de-reference and WHAT code IT ran.
//!     `executor_module_hash` is the index's OWN deploy-supplied module hash, captured into the
//!     draft BEFORE uninstall (same capture point as `module_hash_pre`); a mid-flight index
//!     upgrade does not change it. The de-reference *event* (record_id, deletion_seq, target
//!     principal) is bound SEPARATELY and explicitly in the commitment — it is NOT this hash.
//! - `commitment  = SHA-256(COMMITMENT_TAG || CVDR_ENCODER_VERSION || record_id ||
//!                          deletion_seq(8,BE) || h_user_pre || h_index || user_canister_principal)`
//!   — the value published to `certified_data` and matched by the certificate. `h_index` (and
//!     thus the captured executor module hash) is bound into this hash chain.
//! - `receipt_id  = SHA-256(RECEIPT_ID_TAG || record_id || deletion_seq(8,BE) || nonce)`
//!   — the public fetch capability; the 32-byte `nonce` (LUI rng, captured once at draft
//!     creation and persisted) makes it unguessable from the public UserId/record_id.
//!
//! ## V2 — the IC `data_certificate()` bytes are embedded verbatim in [`ReleasedCvdr`]
//!         (`certificate`), so an external verifier (CVDR-Verify) can re-check the NNS
//!         signature over `certified_data == commitment` itself. The index only *matches*
//!         the certificate to the pending commitment at finalize time.
//!
//! ## V3 — the release-record reference a verifier uses to match the index de-reference:
//!         (`record_id`, `deletion_seq`, `h_index`, `h_user_pre`, `module_hash_pre` [target],
//!         `executor_module_hash` [index]). A verifier recomputes `h_index`/`h_user_pre` from the
//!         raw module hashes + principals and re-derives the commitment.

use crate::memory::{Memory, get_cvdr_draft_memory, get_cvdr_index_memory, get_cvdr_store_memory};
use candid::{CandidType, Principal};
use ic_stable_structures::storable::Bound;
use ic_stable_structures::{StableBTreeMap, Storable};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use types::{CanisterId, TimestampMillis, UserId};

/// PINNED domain tag for the index-side record_id (matches the user canister).
const RECORD_ID_TAG: &[u8] = b"OPENCHATZD_RECORD_ID_USER_V1";
/// PINNED domain tag for h_user_pre.
const H_USER_TAG: &[u8] = b"OPENCHATZD_CVDR_H_USER_V1";
/// PINNED domain tag for h_index.
const H_INDEX_TAG: &[u8] = b"OPENCHATZD_CVDR_H_INDEX_V1";
/// PINNED domain tag for the certified commitment preimage.
const COMMITMENT_TAG: &[u8] = b"OPENCHATZD_CVDR_COMMITMENT_V1";
/// PINNED domain tag for the unguessable receipt_id.
const RECEIPT_ID_TAG: &[u8] = b"OPENCHATZD_CVDR_RECEIPT_V1";

/// PINNED CVDR encoding version. Bump (with the tags) for any preimage change.
pub const CVDR_ENCODER_VERSION: &str = "OPENCHATZD_CVDR_V1";

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

/// `record_id = SHA-256(RECORD_ID_TAG || canonical UserId principal bytes)`. Index-side,
/// deterministic from the durable `UserId` — byte-identical to `mktd::record_id_for`.
pub fn record_id_for(user_id: UserId) -> Hash {
    let principal: Principal = user_id.into();
    tagged(RECORD_ID_TAG, &[principal.as_slice()])
}

pub fn h_user_pre(user_canister_id: CanisterId, module_hash_pre: &[u8]) -> Hash {
    tagged(H_USER_TAG, &[user_canister_id.as_slice(), module_hash_pre])
}

/// `h_index` (EXECUTOR provenance) = `SHA-256(H_INDEX_TAG || index_canister_principal ||
/// executor_module_hash)`. The executor counterpart of `h_user_pre` (target): witnesses WHICH
/// index de-referenced and WHAT code IT ran. `executor_module_hash` is the index's OWN
/// deploy-supplied module hash, captured into the draft before uninstall. The de-reference
/// EVENT (record_id, deletion_seq, target principal) is bound separately + explicitly in the
/// commitment — it is NOT folded into this executor-provenance hash.
pub fn h_index(index_canister_id: CanisterId, executor_module_hash: &[u8]) -> Hash {
    tagged(H_INDEX_TAG, &[index_canister_id.as_slice(), executor_module_hash])
}

pub fn commitment(record_id: &Hash, deletion_seq: u64, h_user_pre: &Hash, h_index: &Hash, user_canister_id: CanisterId) -> Hash {
    tagged(
        COMMITMENT_TAG,
        &[
            CVDR_ENCODER_VERSION.as_bytes(),
            record_id,
            &deletion_seq.to_be_bytes(),
            h_user_pre,
            h_index,
            user_canister_id.as_slice(),
        ],
    )
}

pub fn receipt_id_for(record_id: &Hash, deletion_seq: u64, nonce: &[u8; 32]) -> Hash {
    tagged(RECEIPT_ID_TAG, &[record_id, &deletion_seq.to_be_bytes(), nonce])
}

/// Max age of a submitted certificate: its BLS-signed snapshot time must be within this of
/// `now`. Defence-in-depth against ancient-certificate replay (the commitment binding below
/// already defeats cross-deletion replay, since a stale cert certifies a stale commitment).
const CERT_MAX_OFFSET_MS: u64 = 5 * 60 * 1_000;
const NANOS_PER_MILLI: u128 = 1_000_000;

/// SECURITY-CRITICAL. Fully verify the IC `data_certificate()` bytes and confirm they certify
/// this canister's `certified_data == commitment`. An unverified certificate must NOT finalize.
///
/// Two independent checks, both required:
///   1. BLS-to-NNS verification (`ic_certificate_verification::VerifyCertificate::verify`):
///      walks the subnet delegation chain, verifies the BLS signature against the NNS
///      `ic_root_key`, confirms `self_canister_id` is in the delegated subnet's ranges, and
///      checks the certificate time is within `CERT_MAX_OFFSET_MS` of `now`. An unsigned /
///      forged / wrong-subnet / stale certificate fails here.
///   2. Commitment binding: the certified leaf at `["canister", self, "certified_data"]` must
///      equal the pending `commitment`. A genuine cert over the wrong value fails here.
pub fn certificate_verifies_commitment(
    certificate: &[u8],
    self_canister_id: Principal,
    ic_root_key: &[u8],
    now: TimestampMillis,
    commitment: &Hash,
) -> bool {
    use ic_cbor::CertificateToCbor;
    use ic_certificate_verification::VerifyCertificate;
    use ic_certification::{Certificate, LookupResult};

    let Ok(cert) = Certificate::from_cbor(certificate) else {
        return false;
    };

    // 1. BLS signature -> subnet delegation -> NNS root key (+ certificate time / subnet range).
    let now_nanos = (now as u128).saturating_mul(NANOS_PER_MILLI);
    let max_offset_nanos = (CERT_MAX_OFFSET_MS as u128).saturating_mul(NANOS_PER_MILLI);
    if cert
        .verify(self_canister_id.as_slice(), ic_root_key, &now_nanos, &max_offset_nanos)
        .is_err()
    {
        return false;
    }

    // 2. The verified certificate must certify THIS canister's certified_data == commitment.
    let path: [&[u8]; 3] = [b"canister", self_canister_id.as_slice(), b"certified_data"];
    matches!(cert.tree.lookup_path(path), LookupResult::Found(data) if data == commitment.as_slice())
}

// ---------------------------------------------------------------------------
// Stage of an in-flight deletion draft (forward-only; never rolls back).
// ---------------------------------------------------------------------------

#[derive(CandidType, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DraftStage {
    /// Pre-uninstall: H_user_pre + targets + record_id captured, draft persisted.
    Captured,
    /// `uninstall_code` confirmed (no module). The commitment may now be published.
    Uninstalled,
    /// The commitment occupies the single certified-data slot; awaiting the external
    /// finalizer to return the IC `data_certificate()`.
    AwaitingCertificate,
}

/// In-flight durable deletion draft, keyed by `user_canister_id`. Survives a
/// local_user_index upgrade (stable-backed, `#[serde(skip)]`) so recovery is purely
/// forward (resume from the persisted stage) with no upgrade-trapping heap lock.
#[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
pub struct CvdrDraft {
    pub user_id: UserId,
    pub user_canister_id: CanisterId,
    /// This index's own principal (the executor) — bound into `h_index` and exposed in the
    /// receipt so an offline verifier can recompute `h_index` without the certificate.
    pub index_canister_id: CanisterId,
    pub record_id: Hash,
    pub deletion_seq: u64,
    /// 32 LUI-rng bytes; makes `receipt_id` unguessable. Captured once, never changed.
    pub nonce: [u8; 32],
    pub receipt_id: Hash,
    /// Pre-uninstall module hash of the TARGET user canister (the destroyed code); empty only
    /// if the canister was already module-less when captured.
    pub module_hash_pre: Vec<u8>,
    /// The EXECUTOR (this index) module hash, captured from durable deploy-supplied state at the
    /// same point as `module_hash_pre`. Authoritative for `h_index`; a mid-flight index upgrade
    /// does not change this captured value.
    pub executor_module_hash: Vec<u8>,
    pub h_user_pre: Hash,
    pub h_index: Hash,
    pub commitment: Hash,
    /// Group/community targets for `NotifyOfUserDeleted`, captured BEFORE uninstall so
    /// recovery never re-reads the destroyed canister.
    pub canisters_to_notify: Vec<CanisterId>,
    pub created_at: TimestampMillis,
    pub attempt: u32,
    pub stage: DraftStage,
}

/// The releasable CVDR, stored keyed by `receipt_id`. See V1/V2/V3 in the module docs.
#[derive(CandidType, Serialize, Deserialize, Clone, Debug)]
pub struct ReleasedCvdr {
    pub encoder_version: String,
    pub receipt_id: Hash,
    pub record_id: Hash,
    pub deletion_seq: u64,
    pub user_canister_id: CanisterId,
    /// This index's own principal (executor) — recomputes `h_index` together with `executor_module_hash`.
    pub index_canister_id: CanisterId,
    /// Raw TARGET (user canister) pre-uninstall module hash — recomputes `h_user_pre`.
    pub module_hash_pre: Vec<u8>,
    /// Raw EXECUTOR (this index) module hash, captured pre-uninstall — recomputes `h_index`.
    pub executor_module_hash: Vec<u8>,
    pub h_user_pre: Hash,
    pub h_index: Hash,
    pub commitment: Hash,
    /// V2: the IC `data_certificate()` bytes, embedded verbatim for external verification.
    pub certificate: Vec<u8>,
    pub created_at: TimestampMillis,
    pub finalized_at: TimestampMillis,
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
    const BOUND: Bound = Bound::Bounded { max_size: 32, is_fixed_size: true };
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
    const BOUND: Bound = Bound::Bounded { max_size: 40, is_fixed_size: true };
}

fn index_key(record_id: &Hash, deletion_seq: u64) -> Key40 {
    let mut k = [0u8; 40];
    k[..32].copy_from_slice(record_id);
    k[32..].copy_from_slice(&deletion_seq.to_be_bytes());
    Key40(k)
}

impl Storable for ReleasedCvdr {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("ReleasedCvdr encode"))
    }
    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(&self).expect("ReleasedCvdr encode")
    }
    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(&bytes).expect("ReleasedCvdr decode")
    }
    const BOUND: Bound = Bound::Unbounded;
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

/// Durable, upgrade-surviving v5 stores: the released-CVDR map (by receipt_id), its
/// secondary index ((record_id, deletion_seq) -> receipt_id), and the in-flight draft
/// map (by user_canister_id). All three are stable-backed and `#[serde(skip)]`, so a
/// local_user_index upgrade never loses an in-flight deletion or a released receipt.
#[derive(Serialize, Deserialize)]
pub struct CvdrStore {
    #[serde(skip, default = "init_store")]
    store: StableBTreeMap<Key32, ReleasedCvdr, Memory>,
    #[serde(skip, default = "init_index")]
    index: StableBTreeMap<Key40, Key32, Memory>,
    #[serde(skip, default = "init_drafts")]
    drafts: StableBTreeMap<Principal, CvdrDraft, Memory>,
}

impl Default for CvdrStore {
    fn default() -> Self {
        CvdrStore { store: init_store(), index: init_index(), drafts: init_drafts() }
    }
}

fn init_store() -> StableBTreeMap<Key32, ReleasedCvdr, Memory> {
    StableBTreeMap::init(get_cvdr_store_memory())
}
fn init_index() -> StableBTreeMap<Key40, Key32, Memory> {
    StableBTreeMap::init(get_cvdr_index_memory())
}
fn init_drafts() -> StableBTreeMap<Principal, CvdrDraft, Memory> {
    StableBTreeMap::init(get_cvdr_draft_memory())
}

impl CvdrStore {
    // ---- draft lifecycle (in-flight, keyed by user_canister_id) ----

    pub fn upsert_draft(&mut self, draft: CvdrDraft) {
        self.drafts.insert(draft.user_canister_id, draft);
    }

    pub fn get_draft(&self, user_canister_id: &CanisterId) -> Option<CvdrDraft> {
        self.drafts.get(user_canister_id)
    }

    pub fn remove_draft(&mut self, user_canister_id: &CanisterId) -> Option<CvdrDraft> {
        self.drafts.remove(user_canister_id)
    }

    /// The single receipt currently AwaitingCertificate (occupying the certified-data
    /// slot), if any. Used by the single-slot guard.
    pub fn awaiting_certificate_draft(&self) -> Option<CvdrDraft> {
        self.drafts.iter().map(|e| e.value()).find(|d| d.stage == DraftStage::AwaitingCertificate)
    }

    pub fn draft_count(&self) -> u64 {
        self.drafts.len()
    }

    /// Every in-flight draft. Used post-upgrade to resume deletions the volatile delete
    /// queue lost (the user was popped pre-upgrade) and to re-publish the pending
    /// certified commitment (IC certified_data is cleared by an upgrade).
    pub fn all_drafts(&self) -> Vec<CvdrDraft> {
        self.drafts.iter().map(|e| e.value()).collect()
    }

    // ---- released CVDR store (by receipt_id) + secondary index ----

    pub fn store_released(&mut self, cvdr: ReleasedCvdr) {
        let key = Key32(cvdr.receipt_id);
        self.index.insert(index_key(&cvdr.record_id, cvdr.deletion_seq), key.clone());
        self.store.insert(key, cvdr);
    }

    /// Public path: fetch a released CVDR by its unguessable `receipt_id`.
    pub fn get_by_receipt_id(&self, receipt_id: &Hash) -> Option<ReleasedCvdr> {
        self.store.get(&Key32(*receipt_id))
    }

    /// Gated path: resolve `(record_id, deletion_seq)` to a released CVDR. Restricted —
    /// `record_id` is derivable from the (public) UserId, so this must not be a public route.
    /// Not exposed in build-v1 (only the unguessable `receipt_id` path is public).
    #[allow(dead_code)]
    pub fn get_by_record(&self, record_id: &Hash, deletion_seq: u64) -> Option<ReleasedCvdr> {
        self.store.get(&self.index.get(&index_key(record_id, deletion_seq))?)
    }

    pub fn released_count(&self) -> u64 {
        self.store.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(b: u8) -> CanisterId {
        Principal::from_slice(&[b; 10])
    }

    #[test]
    fn record_id_matches_tagged_sha256() {
        let uid: UserId = p(7).into();
        let principal: Principal = uid.into();
        let mut pre = Vec::new();
        pre.extend_from_slice(RECORD_ID_TAG);
        pre.extend_from_slice(principal.as_slice());
        assert_eq!(record_id_for(uid), sha256::sha256(&pre));
    }

    #[test]
    fn receipt_id_unguessable_depends_on_nonce() {
        let rid = record_id_for(p(1).into());
        let a = receipt_id_for(&rid, 5, &[0u8; 32]);
        let b = receipt_id_for(&rid, 5, &[1u8; 32]);
        assert_ne!(a, b, "nonce must affect receipt_id");
    }

    #[test]
    fn commitment_field_order_is_load_bearing() {
        let rid = record_id_for(p(1).into());
        let hu = h_user_pre(p(2), &[9u8; 32]);
        let hi = h_index(p(3), &[7u8; 32]);
        let base = commitment(&rid, 3, &hu, &hi, p(2));
        // Swapping h_user_pre and h_index must change the commitment.
        assert_ne!(base, commitment(&rid, 3, &hi, &hu, p(2)));
        // Changing the sequence must change the commitment.
        assert_ne!(base, commitment(&rid, 4, &hu, &hi, p(2)));
    }

    #[test]
    fn h_index_binds_executor_module_hash() {
        // h_index is EXECUTOR provenance: it must change when the index's module hash changes,
        // and differ from h_user_pre (target) even for the same principal + module hash.
        let base = h_index(p(3), &[1u8; 32]);
        assert_ne!(base, h_index(p(3), &[2u8; 32]), "executor module hash must affect h_index");
        assert_ne!(base, h_index(p(4), &[1u8; 32]), "index principal must affect h_index");
        assert_ne!(base, h_user_pre(p(3), &[1u8; 32]), "executor and target hashes must not collide");
    }

    #[test]
    fn store_round_trip_by_receipt_and_record() {
        let mut s = CvdrStore::default();
        let rid = record_id_for(p(1).into());
        let receipt = receipt_id_for(&rid, 1, &[3u8; 32]);
        let cvdr = ReleasedCvdr {
            encoder_version: CVDR_ENCODER_VERSION.to_string(),
            receipt_id: receipt,
            record_id: rid,
            deletion_seq: 1,
            user_canister_id: p(2),
            index_canister_id: p(5),
            module_hash_pre: vec![1, 2, 3],
            executor_module_hash: vec![10, 11, 12],
            h_user_pre: [4u8; 32],
            h_index: [5u8; 32],
            commitment: [6u8; 32],
            certificate: vec![7, 8, 9],
            created_at: 100,
            finalized_at: 200,
        };
        s.store_released(cvdr.clone());
        assert_eq!(s.get_by_receipt_id(&receipt).unwrap().receipt_id, receipt);
        assert_eq!(s.get_by_record(&rid, 1).unwrap().receipt_id, receipt);
        assert!(s.get_by_receipt_id(&[0u8; 32]).is_none());
        assert_eq!(s.released_count(), 1);
    }
}
