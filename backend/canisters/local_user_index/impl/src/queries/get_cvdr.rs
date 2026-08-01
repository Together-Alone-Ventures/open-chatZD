use crate::model::cvdr::CvdrStore;
use crate::{RuntimeState, read_state};
use ic_cdk::query;
use local_user_index_canister::get_cvdr::{
    AvailablePackage, FrozenWire, PendingInfo, PortablePackageV2Wire, Response::*, *,
};

/// Public fetch by the unguessable bearer `receipt_id` (spec §11.1/§11.2).
///
/// Serves FACTS, NOT VERDICTS: no `VerifiedFinal` / `LateFinalized` / V3 INDEX outcomes.
/// Dual Available: FrozenWire (commitment-only) or PortablePackageV2 (when INDEX evidence stored).
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
        if let Some(evidence) = cvdr.get_index_evidence(receipt_id) {
            let frozen_bytes = frozen_wire.to_canonical_json();
            Available(AvailablePackage::PortablePackageV2(PortablePackageV2Wire::new(
                frozen_bytes,
                evidence.certificate_bytes,
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
    use crate::model::cvdr::{FrozenCvdrPackage, record_id_for, receipt_id_for};
    use crate::model::cvdr_index_evidence::IndexCodeIdentityEvidence;
    use candid::Principal;

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
        let record_id = record_id_for(Principal::from_slice(&[1]).into());
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
    fn frozen_plus_index_evidence_serves_portable_v2_with_gate_a_nested() {
        let mut cvdr = CvdrStore::default();
        let record_id = record_id_for(Principal::from_slice(&[2]).into());
        let receipt_id = receipt_id_for(&record_id, 2, &[9u8; 32]);
        assert!(cvdr.insert_frozen_package(receipt_id, record_id, 2, sample_pkg()).is_ok());
        assert!(cvdr
            .insert_index_evidence(
                receipt_id,
                IndexCodeIdentityEvidence {
                    certificate_bytes: vec![0xde, 0xad],
                }
            )
            .is_ok());

        match get_cvdr_from_store(&receipt_id, &cvdr) {
            Response::Available(AvailablePackage::PortablePackageV2(v2)) => {
                let gate_a = FrozenWire::from(&sample_pkg()).to_canonical_json();
                assert_eq!(v2.frozen, gate_a, "Gate B nested frozen must equal Gate A bytes");
                assert_eq!(v2.index_code_identity_evidence.certificate_bytes, vec![0xde, 0xad]);
                let http_body = AvailablePackage::PortablePackageV2(v2.clone()).to_canonical_json();
                assert_eq!(http_body, v2.to_canonical_json());
            }
            other => panic!("expected PortablePackageV2, got {other:?}"),
        }
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
        let record_id = record_id_for(Principal::from_slice(&[3]).into());
        let receipt_id = receipt_id_for(&record_id, 3, &[7u8; 32]);
        assert!(cvdr.insert_frozen_package(receipt_id, record_id, 3, sample_pkg()).is_ok());

        let mut draft = CvdrDraft {
            user_id: Principal::from_slice(&[3]).into(),
            user_canister_id: Principal::from_slice(&[3]),
            index_canister_id: Principal::from_slice(&[4]),
            record_id,
            deletion_seq: 3,
            nonce: [7u8; 32],
            receipt_id,
            module_hash_pre: vec![1],
            executor_module_hash: vec![2],
            h_user_pre: [4u8; 32],
            h_index: [5u8; 32],
            commitment: [6u8; 32],
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
        let record_id = record_id_for(Principal::from_slice(&[9]).into());
        let receipt_id = receipt_id_for(&record_id, 9, &[1u8; 32]);
        let draft = CvdrDraft {
            user_id: Principal::from_slice(&[9]).into(),
            user_canister_id: Principal::from_slice(&[9]),
            index_canister_id: Principal::from_slice(&[4]),
            record_id,
            deletion_seq: 9,
            nonce: [1u8; 32],
            receipt_id,
            module_hash_pre: vec![],
            executor_module_hash: vec![],
            h_user_pre: [0u8; 32],
            h_index: [0u8; 32],
            commitment: [0u8; 32],
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
        assert!(matches!(
            get_cvdr_from_store(&receipt_id, &cvdr),
            Response::Pending(_)
        ));
        assert!(cvdr.find_draft_by_receipt_id(&receipt_id).is_none());
    }
}
