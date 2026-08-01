//! INDEX code-identity evidence store (spec §14).
//!
//! Parallel to the frozen commitment package: insert-only, keyed by `receipt_id` for lookup.
//! Capture/verify-before-store and HTTP serving land in later M4/M5 slices.

use crate::memory::{
    Memory, get_cvdr_index_evidence_log_data_memory, get_cvdr_index_evidence_log_index_memory,
    get_cvdr_index_evidence_primary_memory,
};
use crate::model::cvdr::Hash;
use crate::model::cvdr_index_attestation::{PORTABLE_PACKAGE_SCHEMA, PORTABLE_PACKAGE_VERSION};
use candid::CandidType;
use ic_stable_structures::storable::Bound;
use ic_stable_structures::{StableBTreeMap, StableLog, Storable};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// Complete portable subnet system-state `read_state` evidence for
/// `/canister/<local_user_index>/module_hash` (spec §14.2).
///
/// Stores the full certificate CBOR (including certified tree). Do not reduce this to an
/// extracted Module Hash + timestamp alone.
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct IndexCodeIdentityEvidence {
    pub certificate_bytes: Vec<u8>,
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

/// Outer downloadable package when INDEX evidence is present (spec §14.1).
/// `frozen` must be the exact canonical FrozenWire bytes for the same receipt.
#[derive(CandidType, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PortablePackageV2 {
    pub schema: String,
    pub version: u32,
    pub frozen: Vec<u8>,
    pub index_code_identity_evidence: IndexCodeIdentityEvidence,
}

impl PortablePackageV2 {
    pub fn new(frozen: Vec<u8>, index_code_identity_evidence: IndexCodeIdentityEvidence) -> Self {
        Self {
            schema: PORTABLE_PACKAGE_SCHEMA.to_string(),
            version: PORTABLE_PACKAGE_VERSION,
            frozen,
            index_code_identity_evidence,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum IndexEvidenceInsertError {
    AlreadyExists,
    LogFull,
    EmptyCertificate,
    FrozenPackageMissing,
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

    pub fn insert(
        &mut self,
        receipt_id: Hash,
        evidence: IndexCodeIdentityEvidence,
    ) -> Result<(), IndexEvidenceInsertError> {
        if evidence.certificate_bytes.is_empty() {
            return Err(IndexEvidenceInsertError::EmptyCertificate);
        }
        if self.primary.contains_key(&ReceiptKey(receipt_id)) {
            return Err(IndexEvidenceInsertError::AlreadyExists);
        }
        let offset = self
            .log
            .append(&evidence)
            .map_err(|_| IndexEvidenceInsertError::LogFull)?;
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
        let first = IndexCodeIdentityEvidence {
            certificate_bytes: vec![1, 2, 3],
        };
        assert_eq!(store.insert(receipt_id, first), Ok(()));
        assert_eq!(store.count(), 1);
        assert_eq!(store.get(&receipt_id).unwrap().certificate_bytes, vec![1, 2, 3]);

        let second = IndexCodeIdentityEvidence {
            certificate_bytes: vec![9, 9, 9],
        };
        assert_eq!(
            store.insert(receipt_id, second),
            Err(IndexEvidenceInsertError::AlreadyExists)
        );
        assert_eq!(store.get(&receipt_id).unwrap().certificate_bytes, vec![1, 2, 3]);
        assert_eq!(store.count(), 1);
    }

    #[test]
    fn rejects_empty_certificate_blob() {
        let mut store = IndexCodeIdentityStore::new();
        let err = store.insert(
            [1u8; 32],
            IndexCodeIdentityEvidence {
                certificate_bytes: vec![],
            },
        );
        assert_eq!(err, Err(IndexEvidenceInsertError::EmptyCertificate));
        assert_eq!(store.count(), 0);
    }

    #[test]
    fn portable_package_v2_pins_schema_and_nests_frozen_bytes() {
        let frozen = vec![0xde, 0xad, 0xbe, 0xef];
        let evidence = IndexCodeIdentityEvidence {
            certificate_bytes: vec![0xca, 0xfe],
        };
        let pkg = PortablePackageV2::new(frozen.clone(), evidence.clone());
        assert_eq!(pkg.schema, PORTABLE_PACKAGE_SCHEMA);
        assert_eq!(pkg.version, PORTABLE_PACKAGE_VERSION);
        assert_eq!(pkg.frozen, frozen);
        assert_eq!(pkg.index_code_identity_evidence, evidence);
    }

    #[test]
    fn distinct_receipts_store_independently() {
        let mut store = IndexCodeIdentityStore::new();
        let a = [1u8; 32];
        let b = [2u8; 32];
        assert_eq!(
            store.insert(
                a,
                IndexCodeIdentityEvidence {
                    certificate_bytes: vec![10],
                }
            ),
            Ok(())
        );
        assert_eq!(
            store.insert(
                b,
                IndexCodeIdentityEvidence {
                    certificate_bytes: vec![20],
                }
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

    #[test]
    fn portable_package_v2_preserves_exact_frozen_byte_identity() {
        let frozen = (0u8..64).collect::<Vec<_>>();
        let pkg = PortablePackageV2::new(
            frozen.clone(),
            IndexCodeIdentityEvidence {
                certificate_bytes: vec![1],
            },
        );
        assert_eq!(pkg.frozen.as_slice(), frozen.as_slice());
        assert_ne!(
            pkg.frozen,
            candid::encode_one(&frozen).unwrap(),
            "nested frozen must stay raw Gate A bytes, not a re-encoded Vec"
        );
    }
}
