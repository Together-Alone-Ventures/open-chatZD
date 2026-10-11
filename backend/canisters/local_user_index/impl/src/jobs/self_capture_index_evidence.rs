//! INDEX Module Hash evidence capture (R-2).
//!
//! From the moment a deletion's `uninstall_code` completes, periodically fetch a subnet
//! system-state `read_state` certificate for `/canister/<self>/module_hash`, verify it on-chain,
//! extract the module hash, and insert-only store certificate + hash for every receipt it
//! qualifies for. One fetched certificate serves all due receipts. Never stores on verify fail;
//! never stores a deployer-supplied value. Only receipts uninstalled under the CURRENTLY installed
//! code are served (`CvdrStore::evidence_capturable`) — the upgrade interlock keeps that code
//! installed until they are.
//!
//! C2 (G ruling): what is STORED is decided after the await by [`cvdr::evidence_store_gate`] on the
//! certificate's authenticated `/time` (`uninstall ≤ /time ≤ uninstall + 24 h`), the code epoch
//! captured when the outcall was issued, and absence of prior evidence — never on the wall clock.
//! A callback landing after wall-clock 24 h still stores a qualifying certificate.

use crate::model::cvdr::EvidencePending;
use crate::model::cvdr::{self, Hash};
use crate::model::cvdr_index_evidence::{IndexCodeIdentityEvidence, IndexEvidenceInsertError};
use crate::model::http_outcall::{self, HttpHeader};
use crate::{RuntimeState, mutate_state, read_state};
use candid::Principal;
use constants::SECOND_IN_MS;
use ic_cdk_timers::TimerId;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;
use tracing::{trace, warn};
use types::{CanisterId, Milliseconds};

thread_local! {
    static TIMER_ID: std::cell::Cell<Option<TimerId>> = const { std::cell::Cell::new(None) };
    static ATTEMPTS: RefCell<HashMap<Hash, AttemptState>> = RefCell::new(HashMap::new());
}

#[derive(Clone, Copy, Default)]
struct AttemptState {
    attempt: u32,
    last_attempt_at_ns: u64,
}

const SWEEP_INTERVAL_MS: Milliseconds = 3 * SECOND_IN_MS;
const MAX_RESPONSE_BYTES: u64 = 16 * 1024;
const RETRY_RESPONSE_BYTES: u64 = 64 * 1024;
const NS_PER_MS: u64 = 1_000_000;
const INGRESS_EXPIRY_NS: u64 = 5 * 60 * 1_000_000_000;

fn backoff_ms(attempt: u32) -> u64 {
    match attempt {
        0 => 3 * SECOND_IN_MS,
        1 => 5 * SECOND_IN_MS,
        2 => 15 * SECOND_IN_MS,
        3 => 45 * SECOND_IN_MS,
        _ => 120 * SECOND_IN_MS,
    }
}

fn now_ns(state: &RuntimeState) -> u64 {
    state.env.now().saturating_mul(NS_PER_MS)
}

/// Receipts the sweep may still serve: past uninstall, no evidence, window open (24 h from
/// `uninstall_completed_at` — after that they drop out and stay V3A-UNAVAILABLE), uninstalled under
/// the current code epoch.
fn capturable(state: &RuntimeState) -> Vec<EvidencePending> {
    state
        .data
        .cvdr
        .evidence_capturable(now_ns(state), state.data.cvdr_code_epoch_started_at_ns)
}

pub(crate) fn start_if_required(state: &RuntimeState) -> bool {
    if TIMER_ID.with(|t| t.get().is_none()) && !capturable(state).is_empty() {
        let timer_id = ic_cdk_timers::set_timer(Duration::from_millis(SWEEP_INTERVAL_MS), run_sweep);
        TIMER_ID.with(|t| t.set(Some(timer_id)));
        true
    } else {
        false
    }
}

/// Backoff gate, anchored at the uninstall for the first attempt.
fn is_due(pending: &EvidencePending, attempt_state: AttemptState, now_ns: u64) -> bool {
    let due_at_ns = if attempt_state.attempt == 0 {
        pending.uninstall_completed_at.saturating_add(backoff_ms(0) * NS_PER_MS)
    } else {
        attempt_state
            .last_attempt_at_ns
            .saturating_add(backoff_ms(attempt_state.attempt) * NS_PER_MS)
    };
    now_ns >= due_at_ns
}

fn run_sweep() {
    TIMER_ID.with(|t| t.set(None));
    let now_ns = ic_cdk::api::time();

    let due: Vec<EvidencePending> = read_state(|state| {
        let pending = capturable(state);
        // forget attempt state of receipts that left the work list (stored, or window lapsed)
        ATTEMPTS.with(|m| m.borrow_mut().retain(|id, _| pending.iter().any(|p| &p.receipt_id == id)));
        pending
            .into_iter()
            .filter(|p| {
                let attempt_state = ATTEMPTS.with(|m| m.borrow().get(&p.receipt_id).copied().unwrap_or_default());
                is_due(p, attempt_state, now_ns)
            })
            .collect()
    });

    if !due.is_empty() {
        ATTEMPTS.with(|m| {
            let mut map = m.borrow_mut();
            for p in &due {
                let entry = map.entry(p.receipt_id).or_default();
                entry.attempt = entry.attempt.saturating_add(1);
                entry.last_attempt_at_ns = now_ns;
            }
        });
        // The epoch under which this capture is issued travels with the task (C2 gate (a)).
        let captured_epoch_ns = read_state(|state| state.data.cvdr_code_epoch_started_at_ns);
        // ONE certificate serves every due receipt it qualifies for.
        ic_cdk::futures::spawn(attempt_capture(due, captured_epoch_ns));
    }

    mutate_state(|state| {
        start_if_required(state);
    });
}

async fn attempt_capture(due: Vec<EvidencePending>, captured_epoch_ns: u64) {
    let self_id = read_state(|state| state.env.canister_id());
    let certificate = match fetch_module_hash_certificate(self_id, MAX_RESPONSE_BYTES).await {
        Some(c) => Some(c),
        None => fetch_module_hash_certificate(self_id, RETRY_RESPONSE_BYTES).await,
    };
    let Some(certificate) = certificate else {
        trace!(receipts = due.len(), "INDEX read_state miss; will retry");
        return;
    };

    mutate_state(|state| {
        let now = state.env.now();
        let ic_root_key = state.env.ic_root_key();
        for pending in due {
            store_verified_evidence(state, &pending, &certificate, self_id, &ic_root_key, now, captured_epoch_ns);
        }
    });
}

#[allow(clippy::too_many_arguments)]
fn store_verified_evidence(
    state: &mut RuntimeState,
    pending: &EvidencePending,
    certificate: &[u8],
    self_id: CanisterId,
    ic_root_key: &[u8],
    now: types::TimestampMillis,
    captured_epoch_ns: u64,
) {
    let receipt_id = pending.receipt_id;
    // Authenticate first (BLS → NNS → range → exact /module_hash path → /time); the not-before is
    // re-checked by the gate below from the same authenticated time.
    match cvdr::verify_index_module_hash_evidence(certificate, self_id, ic_root_key, now, None) {
        Ok(verified) => {
            // C2 store-gate, post-await, before insert: epoch / certified window / absence.
            if let Err(reject) = cvdr::evidence_store_gate(
                captured_epoch_ns,
                state.data.cvdr_code_epoch_started_at_ns,
                verified.cert_time_ns,
                pending.uninstall_completed_at,
                state.data.cvdr.has_index_evidence(&receipt_id),
            ) {
                if reject != cvdr::EvidenceGateReject::AlreadyStored {
                    warn!(
                        event = "cvdr_index_evidence_discarded",
                        receipt_prefix = %cvdr::receipt_id_prefix(&receipt_id),
                        reason = reject.as_str(),
                        cert_time_ns = verified.cert_time_ns,
                        uninstall_completed_at_ns = pending.uninstall_completed_at,
                        "verified INDEX certificate does not qualify for this receipt; discarded, not stored"
                    );
                }
                return;
            }
            // Ops-integrity guard ONLY (R-2): the deployer's expectation is compared, logged and
            // counted — it is never stored as evidence and never decides what is stored.
            if let Some(expected) = state.data.expected_index_module_hash
                && verified.module_hash.as_slice() != expected.as_slice()
            {
                state.data.cvdr_index_module_hash_expectation_mismatches =
                    state.data.cvdr_index_module_hash_expectation_mismatches.saturating_add(1);
                warn!(
                    event = "cvdr_index_module_hash_expectation_mismatch",
                    receipt_prefix = %cvdr::receipt_id_prefix(&receipt_id),
                    "certified Index module hash differs from the deploy-supplied expectation"
                );
            }
            let evidence = IndexCodeIdentityEvidence::new(
                certificate.to_vec(),
                verified.module_hash,
                cvdr::trust_root_key_id_for(ic_root_key).to_string(),
            );
            match state.data.cvdr.insert_index_evidence(receipt_id, evidence) {
                Ok(()) | Err(IndexEvidenceInsertError::AlreadyExists) => {
                    trace!(receipt_prefix = %cvdr::receipt_id_prefix(&receipt_id), "INDEX module hash evidence stored");
                }
                Err(e) => {
                    warn!(event = "cvdr_index_evidence_not_stored", receipt_prefix = %cvdr::receipt_id_prefix(&receipt_id), error = ?e);
                }
            }
        }
        Err(reason) => {
            warn!(
                event = "cvdr_index_evidence_rejected",
                receipt_prefix = %cvdr::receipt_id_prefix(&receipt_id),
                reason = reason.as_str(),
                "INDEX evidence failed store-gate; discarded"
            );
        }
    }
}

async fn fetch_module_hash_certificate(self_id: CanisterId, max_response_bytes: u64) -> Option<Vec<u8>> {
    let url = format!("https://icp-api.io/api/v2/canister/{}/read_state", self_id);
    let now_ns = ic_cdk::api::time();
    let body = encode_anonymous_module_hash_read_state(self_id, now_ns.saturating_add(INGRESS_EXPIRY_NS))?;
    let headers = vec![HttpHeader {
        name: "Content-Type".to_string(),
        value: "application/cbor".to_string(),
    }];
    let result = http_outcall::non_replicated_post(url, headers, body, max_response_bytes)
        .await
        .ok()?;
    if !nat_is_200(&result.status) {
        return None;
    }
    parse_read_state_certificate(&result.body)
}

fn nat_is_200(status: &candid::Nat) -> bool {
    status == &candid::Nat::from(200u32)
}

#[derive(Serialize)]
#[serde(tag = "request_type", rename_all = "snake_case")]
enum EnvelopeContent {
    ReadState {
        ingress_expiry: u64,
        sender: Principal,
        paths: Vec<Vec<serde_bytes::ByteBuf>>,
    },
}

#[derive(Serialize)]
struct Envelope {
    content: EnvelopeContent,
}

#[derive(Deserialize)]
struct ReadStateResponse {
    #[serde(with = "serde_bytes")]
    certificate: Vec<u8>,
}

pub(crate) fn encode_anonymous_module_hash_read_state(canister_id: Principal, ingress_expiry: u64) -> Option<Vec<u8>> {
    let path_module = vec![
        serde_bytes::ByteBuf::from(b"canister".as_slice()),
        serde_bytes::ByteBuf::from(canister_id.as_slice()),
        serde_bytes::ByteBuf::from(b"module_hash".as_slice()),
    ];
    let path_time = vec![serde_bytes::ByteBuf::from(b"time".as_slice())];
    let envelope = Envelope {
        content: EnvelopeContent::ReadState {
            ingress_expiry,
            sender: Principal::anonymous(),
            paths: vec![path_module, path_time],
        },
    };
    let mut serializer = serde_cbor::Serializer::new(Vec::new());
    serializer.self_describe().ok()?;
    envelope.serialize(&mut serializer).ok()?;
    Some(serializer.into_inner())
}

fn parse_read_state_certificate(body: &[u8]) -> Option<Vec<u8>> {
    let parsed: ReadStateResponse = serde_cbor::from_slice(body).ok()?;
    if parsed.certificate.is_empty() { None } else { Some(parsed.certificate) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::cvdr::{IndexEvidenceRejectReason, check_index_cert_not_before_uninstall};

    #[test]
    fn anonymous_read_state_request_encodes_module_hash_path() {
        let id = Principal::from_text("aaaaa-aa").unwrap();
        let bytes = encode_anonymous_module_hash_read_state(id, 1_000_000).unwrap();
        assert!(bytes.len() > 16);
        assert_eq!(&bytes[..3], &[0xd9, 0xd9, 0xf7]);
        assert!(bytes.windows(11).any(|w| w == b"module_hash"));
        assert!(bytes.windows(4).any(|w| w == b"time"));
        assert!(bytes.windows(8).any(|w| w == b"canister"));
        assert!(bytes.windows(10).any(|w| w == b"read_state"));
    }

    #[test]
    fn parse_read_state_rejects_empty_certificate() {
        let body = serde_cbor::to_vec(&serde_cbor::Value::Map(
            [(
                serde_cbor::Value::Text("certificate".into()),
                serde_cbor::Value::Bytes(vec![]),
            )]
            .into_iter()
            .collect(),
        ))
        .unwrap();
        assert!(parse_read_state_certificate(&body).is_none());
    }

    #[test]
    fn parse_read_state_accepts_non_empty_certificate_blob() {
        let body = serde_cbor::to_vec(&serde_cbor::Value::Map(
            [(
                serde_cbor::Value::Text("certificate".into()),
                serde_cbor::Value::Bytes(vec![0xca, 0xfe]),
            )]
            .into_iter()
            .collect(),
        ))
        .unwrap();
        assert_eq!(parse_read_state_certificate(&body), Some(vec![0xca, 0xfe]));
    }

    #[test]
    fn cert_time_before_commitment_is_rejected() {
        assert_eq!(
            check_index_cert_not_before_uninstall(10, 20),
            Err(IndexEvidenceRejectReason::CertTimeBeforeUninstall)
        );
        assert_eq!(check_index_cert_not_before_uninstall(20, 20), Ok(()));
        assert_eq!(check_index_cert_not_before_uninstall(21, 20), Ok(()));
    }

    #[test]
    fn concurrent_inserts_keep_first_evidence() {
        use crate::model::cvdr::{CvdrStore, FrozenCvdrPackage, receipt_id_for, record_id_v2};
        use crate::model::cvdr_index_evidence::{IndexCodeIdentityEvidence, IndexEvidenceInsertError};
        use candid::Principal;

        let mut s = CvdrStore::default();
        let user = Principal::from_slice(&[9u8; 29]);
        let record_id = record_id_v2(&[0x5Au8; 32], user.into());
        let receipt_id = receipt_id_for(&record_id, 1, &[1u8; 32]);
        let pkg = FrozenCvdrPackage {
            receipt_body: vec![1],
            receipt_hash: [2u8; 32],
            tree_root: [3u8; 32],
            witness_bytes: vec![4],
            certificate_bytes: vec![5],
            certificate_time: 100,
        };
        assert_eq!(s.insert_frozen_package(receipt_id, record_id, 1, pkg), Ok(()));
        assert_eq!(
            s.insert_index_evidence(
                receipt_id,
                IndexCodeIdentityEvidence::new(vec![0xaa], vec![0x1d; 32], "mainnet".to_string())
            ),
            Ok(())
        );
        assert_eq!(
            s.insert_index_evidence(
                receipt_id,
                IndexCodeIdentityEvidence::new(vec![0xbb], vec![0x1d; 32], "mainnet".to_string())
            ),
            Err(IndexEvidenceInsertError::AlreadyExists)
        );
        assert_eq!(s.get_index_evidence(&receipt_id).unwrap().certificate_bytes, vec![0xaa]);
    }
}
