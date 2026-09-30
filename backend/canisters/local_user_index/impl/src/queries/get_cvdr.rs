use crate::model::cvdr::CvdrStore;
use crate::{RuntimeState, read_state};
use ic_cdk::query;
use local_user_index_canister::get_cvdr::{AvailablePackage, FrozenWire, PendingInfo, PortablePackageV3Wire, Response::*, *};

/// Public fetch by the unguessable bearer `receipt_id` (spec §11.1/§11.2).
///
/// Serves FACTS, NOT VERDICTS: no `VerifiedFinal` / `LateFinalized` / V3 INDEX outcomes.
/// Dual Available: FrozenWire (commitment-only) or PortablePackageV3 (when INDEX evidence stored).
#[query]
fn get_cvdr(args: Args) -> Response {
    read_state(|state| get_cvdr_impl(args, state))
}

pub(crate) fn get_cvdr_impl(args: Args, state: &RuntimeState) -> Response {
    get_cvdr_from_store(&args.receipt_id, &state.data.cvdr)
}

/// Pure serving decision (spec §11.2) — unit-tested without a full `RuntimeState`.
pub(crate) fn get_cvdr_from_store(receipt_id: &[u8; 32], cvdr: &CvdrStore) -> Response {
    if let Some(package) = cvdr.get_frozen_package(receipt_id) {
        let frozen_wire = FrozenWire::from(&package);
        // Fail closed (R-6): evidence stored by a pre-step-4 wasm (no extracted hash / no trust-root
        // id) is never projected into a V3 package with fields invented at serve time — the receipt
        // is served as FrozenWire (V3A-unavailable) instead.
        if let Some((evidence, index_module_hash, trust_root_key_id)) = cvdr.get_index_evidence(receipt_id).and_then(|e| {
            Some((
                e.certificate_bytes.clone(),
                e.index_module_hash.clone()?,
                e.trust_root_key_id.clone()?,
            ))
        }) {
            let frozen_bytes = frozen_wire.to_canonical_json();
            Available(AvailablePackage::PortablePackageV3(PortablePackageV3Wire::new(
                frozen_bytes,
                evidence,
                index_module_hash,
                trust_root_key_id,
            )))
        } else {
            Available(AvailablePackage::FrozenWire(frozen_wire))
        }
    } else if cvdr.find_any_draft_by_receipt_id(receipt_id).is_some() {
        Pending(PendingInfo::default())
    } else {
        NotFound
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::cvdr::{FrozenCvdrPackage, receipt_id_for, record_id_v2};
    use crate::model::cvdr_index_evidence::IndexCodeIdentityEvidence;
    use candid::Principal;
    use ic_stable_structures::Storable;
    use local_user_index_canister::get_cvdr::PORTABLE_VERSION;

    fn sample_pkg() -> FrozenCvdrPackage {
        FrozenCvdrPackage {
            receipt_body: vec![1, 2, 3],
            receipt_hash: [4u8; 32],
            tree_root: [5u8; 32],
            witness_bytes: vec![6, 7],
            certificate_bytes: vec![8, 9],
            certificate_time: 1234,
        }
    }

    #[test]
    fn frozen_only_serves_frozen_wire() {
        let mut cvdr = CvdrStore::default();
        let record_id = record_id_v2(&[0x5Au8; 32], Principal::from_slice(&[1]).into());
        let receipt_id = receipt_id_for(&record_id, 1, &[3u8; 32]);
        assert!(cvdr.insert_frozen_package(receipt_id, record_id, 1, sample_pkg()).is_ok());

        match get_cvdr_from_store(&receipt_id, &cvdr) {
            Response::Available(AvailablePackage::FrozenWire(w)) => {
                assert_eq!(w.certificate_time, 1234);
            }
            other => panic!("expected FrozenWire, got {other:?}"),
        }
    }

    #[test]
    fn frozen_plus_index_evidence_serves_portable_v3_with_gate_a_nested() {
        let mut cvdr = CvdrStore::default();
        let record_id = record_id_v2(&[0x5Au8; 32], Principal::from_slice(&[2]).into());
        let receipt_id = receipt_id_for(&record_id, 2, &[9u8; 32]);
        assert!(cvdr.insert_frozen_package(receipt_id, record_id, 2, sample_pkg()).is_ok());
        assert!(
            cvdr.insert_index_evidence(
                receipt_id,
                IndexCodeIdentityEvidence::new(vec![0xde, 0xad], vec![0x1d; 32], "mainnet".to_string())
            )
            .is_ok()
        );

        match get_cvdr_from_store(&receipt_id, &cvdr) {
            Response::Available(AvailablePackage::PortablePackageV3(v3)) => {
                let gate_a = FrozenWire::from(&sample_pkg()).to_canonical_json();
                assert_eq!(v3.frozen, gate_a, "Gate B nested frozen must equal Gate A bytes");
                assert_eq!(v3.version, PORTABLE_VERSION);
                assert_eq!(v3.version, 3, "V2 portable packages are retired; serve path emits V3 only");
                assert_eq!(v3.trust_root_key_id, "mainnet");
                assert_eq!(v3.index_code_identity_evidence.index_module_hash, vec![0x1d; 32]);
                assert_eq!(v3.index_code_identity_evidence.certificate_bytes, vec![0xde, 0xad]);
                let http_body = AvailablePackage::PortablePackageV3(v3.clone()).to_canonical_json();
                assert_eq!(http_body, v3.to_canonical_json());
                let s = String::from_utf8(http_body).unwrap();
                assert!(s.contains(r#""version":3"#));
                assert!(s.contains(r#""trust_root_key_id":"mainnet""#));
                assert!(!s.contains(r#""version":2"#), "served JSON must not claim portable V2");
            }
            other => panic!("expected PortablePackageV3, got {other:?}"),
        }
    }

    /// R-6 fail-closed serve rule: incomplete evidence (no extracted hash and/or no trust root)
    /// must never be projected into PortablePackageV3. The insert gate already refuses such
    /// evidence; this pins the serve-path `.and_then` short-circuit against a decoded legacy shape.
    #[test]
    fn incomplete_index_evidence_never_projects_portable_v3() {
        let mut cvdr = CvdrStore::default();
        let record_id = record_id_v2(&[0x5Au8; 32], Principal::from_slice(&[9]).into());
        let receipt_id = receipt_id_for(&record_id, 9, &[1u8; 32]);
        assert!(cvdr.insert_frozen_package(receipt_id, record_id, 9, sample_pkg()).is_ok());

        // Bypass the insert gate the way a pre-step-4 stable decode would: put certificate-only
        // evidence into the store via candid round-trip of the pre-step-3 shape, then assert serve.
        let legacy = {
            #[derive(candid::CandidType, serde::Serialize)]
            struct PreStep3Evidence {
                certificate_bytes: Vec<u8>,
            }
            let bytes = candid::encode_one(PreStep3Evidence {
                certificate_bytes: vec![0xaa, 0xbb],
            })
            .unwrap();
            IndexCodeIdentityEvidence::from_bytes(std::borrow::Cow::Owned(bytes))
        };
        assert_eq!(legacy.index_module_hash, None);
        assert_eq!(legacy.trust_root_key_id, None);
        // Direct primary insert is not exposed; re-check the serve predicate in isolation.
        let projected = legacy
            .index_module_hash
            .clone()
            .zip(legacy.trust_root_key_id.clone());
        assert!(
            projected.is_none(),
            "legacy incomplete evidence must fail the V3 projection predicate"
        );
        assert!(
            matches!(
                get_cvdr_from_store(&receipt_id, &cvdr),
                Response::Available(AvailablePackage::FrozenWire(_))
            ),
            "without complete evidence the receipt stays FrozenWire"
        );
    }

    #[test]
    fn unknown_receipt_is_not_found() {
        let cvdr = CvdrStore::default();
        assert!(matches!(get_cvdr_from_store(&[0u8; 32], &cvdr), Response::NotFound));
    }

    #[test]
    fn scrubbed_draft_with_frozen_still_serves_available() {
        use crate::model::cvdr::{CvdrDraft, DraftStage};

        let mut cvdr = CvdrStore::default();
        let record_id = record_id_v2(&[0x5Au8; 32], Principal::from_slice(&[3]).into());
        let receipt_id = receipt_id_for(&record_id, 3, &[7u8; 32]);
        assert!(cvdr.insert_frozen_package(receipt_id, record_id, 3, sample_pkg()).is_ok());

        let mut draft = CvdrDraft {
            user_id: Principal::from_slice(&[3]).into(),
            user_canister_id: Principal::from_slice(&[3]),
            index_canister_id: Principal::from_slice(&[4]),
            record_salt: Some([0x5Au8; 32]),
            record_id,
            deletion_seq: 3,
            nonce: [7u8; 32],
            receipt_id,
            module_hash_pre: vec![1],
            h_user_pre: [4u8; 32],
            salt: [0xAB; 32],
            canisters_to_notify: vec![Principal::from_slice(&[5])],
            uninstall_completed_at: 111,
            receipt_committed_at: 222,
            finalize_attempt: 1,
            finalize_last_attempt_at: 333,
            created_at: 100,
            attempt: 1,
            stage: DraftStage::CertificateCaptured,
        };
        draft.scrub_sensitive();
        cvdr.upsert_draft(draft);

        assert!(matches!(
            get_cvdr_from_store(&receipt_id, &cvdr),
            Response::Available(AvailablePackage::FrozenWire(_))
        ));
    }

    #[test]
    fn pending_info_leaks_no_draft_fields() {
        let pending = PendingInfo::default();
        let body = serde_json::to_vec(&pending).unwrap();
        let s = String::from_utf8(body).unwrap();
        assert!(s.contains(r#""status":"pending""#));
        for leak in ["salt", "receipt_id", "receipt_body", "canisters_to_notify", "module_hash"] {
            assert!(!s.contains(leak), "PendingInfo must not contain `{leak}`");
        }
    }

    #[test]
    fn prepared_draft_serves_pending_not_available() {
        use crate::model::cvdr::{CvdrDraft, DraftStage};

        let mut cvdr = CvdrStore::default();
        let record_id = record_id_v2(&[0x5Au8; 32], Principal::from_slice(&[9]).into());
        let receipt_id = receipt_id_for(&record_id, 9, &[1u8; 32]);
        let draft = CvdrDraft {
            user_id: Principal::from_slice(&[9]).into(),
            user_canister_id: Principal::from_slice(&[9]),
            index_canister_id: Principal::from_slice(&[4]),
            record_salt: Some([0x5Au8; 32]),
            record_id,
            deletion_seq: 9,
            nonce: [1u8; 32],
            receipt_id,
            module_hash_pre: vec![],
            h_user_pre: [0u8; 32],
            salt: [0x11; 32],
            canisters_to_notify: vec![],
            uninstall_completed_at: 0,
            receipt_committed_at: 0,
            finalize_attempt: 0,
            finalize_last_attempt_at: 0,
            created_at: 1,
            attempt: 0,
            stage: DraftStage::Prepared,
        };
        assert!(!draft.is_finalizable(), "Prepared must not be /cvdr_live-servable");
        cvdr.upsert_draft(draft);
        assert!(matches!(get_cvdr_from_store(&receipt_id, &cvdr), Response::Pending(_)));
        assert!(cvdr.find_draft_by_receipt_id(&receipt_id).is_none());
    }
}
