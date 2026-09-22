//! INDEX code-identity evidence store (spec §14).
//!
//! Parallel to the frozen commitment package: insert-only, keyed by `receipt_id` for lookup.
//! Capture/verify-before-store and HTTP serving land in later M4/M5 slices.

use crate::memory::{
    Memory, get_cvdr_index_evidence_log_data_memory, get_cvdr_index_evidence_log_index_memory,
    get_cvdr_index_evidence_primary_memory,
};
use crate::model::cvdr::Hash;
use candid::CandidType;
use ic_stable_structures::storable::Bound;
use ic_stable_structures::{StableBTreeMap, StableLog, Storable};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// Complete portable subnet system-state `read_state` evidence for
/// `/canister/<local_user_index>/module_hash` (spec §14.2).
///
/// Stores the full certificate CBOR (including certified tree) — the V3A trust object — AND the
/// `index_module_hash` extracted from it at the store-gate (R-2): the archival code-identity value
/// displayed in the package and equality-checked by the verifier against the certificate. Never
/// reduce this to the extracted hash alone; never fill the hash from a deployer-supplied value.
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct IndexCodeIdentityEvidence {
    pub certificate_bytes: Vec<u8>,
    /// `Some` for every evidence written from step 3 on (the insert path refuses `None`). `None`
    /// only when decoding evidence stored by an earlier wasm, which kept the certificate alone
    /// (Candid rejects a missing non-optional field, so the field must be optional to decode).
    pub index_module_hash: Option<Vec<u8>>,
    /// R-6: the trust root the certificate was verified under, stamped from Index configuration at
    /// the store-gate (`"mainnet"` for the IC NNS key, a self-labelled non-production id otherwise).
    /// Never client-supplied, never inferred from the certificate. `None` only when decoding
    /// evidence stored by a pre-step-4 wasm.
    pub trust_root_key_id: Option<String>,
}

impl IndexCodeIdentityEvidence {
    pub fn new(certificate_bytes: Vec<u8>, index_module_hash: Vec<u8>, trust_root_key_id: String) -> Self {
        Self {
            certificate_bytes,
            index_module_hash: Some(index_module_hash),
            trust_root_key_id: Some(trust_root_key_id),
        }
    }
}

impl Storable for IndexCodeIdentityEvidence {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Owned(candid::encode_one(self).expect("IndexCodeIdentityEvidence encode"))
    }
    fn into_bytes(self) -> Vec<u8> {
        candid::encode_one(&self).expect("IndexCodeIdentityEvidence encode")
    }
    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        candid::decode_one(&bytes).expect("IndexCodeIdentityEvidence decode")
    }
    const BOUND: Bound = Bound::Unbounded;
}

#[derive(Debug, PartialEq, Eq)]
pub enum IndexEvidenceInsertError {
    AlreadyExists,
    LogFull,
    EmptyCertificate,
    /// The extracted `index_module_hash` is absent or empty (R-2: certificate AND hash are stored).
    ModuleHashMissing,
    /// `trust_root_key_id` absent or empty (R-6: stamped at the store-gate, always).
    TrustRootMissing,
    /// No receipt (post-uninstall draft or frozen package) exists for this `receipt_id`.
    ReceiptMissing,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ReceiptKey([u8; 32]);

impl Storable for ReceiptKey {
    fn to_bytes(&self) -> Cow<'_, [u8]> {
        Cow::Borrowed(&self.0)
    }
    fn into_bytes(self) -> Vec<u8> {
        self.0.to_vec()
    }
    fn from_bytes(bytes: Cow<'_, [u8]>) -> Self {
        let mut a = [0u8; 32];
        a.copy_from_slice(&bytes);
        ReceiptKey(a)
    }
    const BOUND: Bound = Bound::Bounded {
        max_size: 32,
        is_fixed_size: true,
    };
}

pub struct IndexCodeIdentityStore {
    log: StableLog<IndexCodeIdentityEvidence, Memory, Memory>,
    primary: StableBTreeMap<ReceiptKey, u64, Memory>,
}

impl IndexCodeIdentityStore {
    pub fn new() -> Self {
        Self {
            log: StableLog::init(
                get_cvdr_index_evidence_log_index_memory(),
                get_cvdr_index_evidence_log_data_memory(),
            ),
            primary: StableBTreeMap::init(get_cvdr_index_evidence_primary_memory()),
        }
    }

    pub fn insert(&mut self, receipt_id: Hash, evidence: IndexCodeIdentityEvidence) -> Result<(), IndexEvidenceInsertError> {
        if evidence.certificate_bytes.is_empty() {
            return Err(IndexEvidenceInsertError::EmptyCertificate);
        }
        if evidence.index_module_hash.as_ref().is_none_or(|h| h.is_empty()) {
            return Err(IndexEvidenceInsertError::ModuleHashMissing);
        }
        if evidence.trust_root_key_id.as_ref().is_none_or(|id| id.is_empty()) {
            return Err(IndexEvidenceInsertError::TrustRootMissing);
        }
        if self.primary.contains_key(&ReceiptKey(receipt_id)) {
            return Err(IndexEvidenceInsertError::AlreadyExists);
        }
        let offset = self.log.append(&evidence).map_err(|_| IndexEvidenceInsertError::LogFull)?;
        self.primary.insert(ReceiptKey(receipt_id), offset);
        Ok(())
    }

    pub fn get(&self, receipt_id: &Hash) -> Option<IndexCodeIdentityEvidence> {
        let offset = self.primary.get(&ReceiptKey(*receipt_id))?;
        self.log.get(offset)
    }

    pub fn contains(&self, receipt_id: &Hash) -> bool {
        self.primary.contains_key(&ReceiptKey(*receipt_id))
    }

    pub fn count(&self) -> u64 {
        self.primary.len()
    }
}

impl Default for IndexCodeIdentityStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_only_rejects_overwrite() {
        let mut store = IndexCodeIdentityStore::new();
        let receipt_id = [7u8; 32];
        let first = IndexCodeIdentityEvidence::new(vec![1, 2, 3], vec![0x1d; 32], "mainnet".to_string());
        assert_eq!(store.insert(receipt_id, first), Ok(()));
        assert_eq!(store.count(), 1);
        assert_eq!(store.get(&receipt_id).unwrap().certificate_bytes, vec![1, 2, 3]);

        let second = IndexCodeIdentityEvidence::new(vec![9, 9, 9], vec![0x1d; 32], "mainnet".to_string());
        assert_eq!(store.insert(receipt_id, second), Err(IndexEvidenceInsertError::AlreadyExists));
        assert_eq!(store.get(&receipt_id).unwrap().certificate_bytes, vec![1, 2, 3]);
        assert_eq!(store.count(), 1);
    }

    /// Stored-struct upgrade: evidence written before step 3 (certificate only) still decodes.
    #[test]
    fn pre_step3_stored_evidence_decodes_without_module_hash() {
        #[derive(CandidType, Serialize)]
        struct PreStep3Evidence {
            certificate_bytes: Vec<u8>,
        }
        let bytes = candid::encode_one(PreStep3Evidence {
            certificate_bytes: vec![1, 2, 3],
        })
        .unwrap();
        let decoded = IndexCodeIdentityEvidence::from_bytes(Cow::Owned(bytes));
        assert_eq!(decoded.certificate_bytes, vec![1, 2, 3]);
        assert_eq!(decoded.index_module_hash, None);
        assert_eq!(decoded.trust_root_key_id, None);
    }

    #[test]
    fn rejects_evidence_without_trust_root_id() {
        let mut store = IndexCodeIdentityStore::new();
        for id in [None, Some(String::new())] {
            let err = store.insert(
                [1u8; 32],
                IndexCodeIdentityEvidence {
                    certificate_bytes: vec![1],
                    index_module_hash: Some(vec![0x1d; 32]),
                    trust_root_key_id: id,
                },
            );
            assert_eq!(err, Err(IndexEvidenceInsertError::TrustRootMissing));
        }
        assert_eq!(store.count(), 0);
    }

    #[test]
    fn rejects_evidence_without_extracted_module_hash() {
        let mut store = IndexCodeIdentityStore::new();
        for hash in [None, Some(vec![])] {
            let err = store.insert(
                [1u8; 32],
                IndexCodeIdentityEvidence {
                    certificate_bytes: vec![1],
                    index_module_hash: hash,
                    trust_root_key_id: Some("mainnet".to_string()),
                },
            );
            assert_eq!(err, Err(IndexEvidenceInsertError::ModuleHashMissing));
        }
        assert_eq!(store.count(), 0);
    }

    #[test]
    fn rejects_empty_certificate_blob() {
        let mut store = IndexCodeIdentityStore::new();
        let err = store.insert(
            [1u8; 32],
            IndexCodeIdentityEvidence::new(vec![], vec![0x1d; 32], "mainnet".to_string()),
        );
        assert_eq!(err, Err(IndexEvidenceInsertError::EmptyCertificate));
        assert_eq!(store.count(), 0);
    }

    #[test]
    fn distinct_receipts_store_independently() {
        let mut store = IndexCodeIdentityStore::new();
        let a = [1u8; 32];
        let b = [2u8; 32];
        assert_eq!(
            store.insert(
                a,
                IndexCodeIdentityEvidence::new(vec![10], vec![0x1d; 32], "mainnet".to_string())
            ),
            Ok(())
        );
        assert_eq!(
            store.insert(
                b,
                IndexCodeIdentityEvidence::new(vec![20], vec![0x1d; 32], "mainnet".to_string())
            ),
            Ok(())
        );
        assert!(store.contains(&a));
        assert!(store.contains(&b));
        assert_eq!(store.get(&a).unwrap().certificate_bytes, vec![10]);
        assert_eq!(store.get(&b).unwrap().certificate_bytes, vec![20]);
        assert_eq!(store.count(), 2);
    }

    #[test]
    fn unknown_receipt_lookup_is_none() {
        let store = IndexCodeIdentityStore::new();
        assert!(!store.contains(&[0u8; 32]));
        assert_eq!(store.get(&[0u8; 32]), None);
        assert_eq!(store.count(), 0);
    }
}
