//! INDEX Module Hash evidence capture (spec §14.8).
//!
//! After a frozen commitment package exists, periodically fetch a subnet system-state
//! `read_state` certificate for `/canister/<self>/module_hash`, verify it on-chain, then
//! insert-only store. Never compares to `h_index` (offline V3). Never stores on verify fail.

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
const GIVE_UP_MS: u64 = 24 * 60 * 60 * 1_000;
const NS_PER_MS: u64 = 1_000_000;
const INGRESS_EXPIRY_NS: u64 = 5 * 60 * 1_000_000_000;
const MAX_CONCURRENT_CAPTURES: usize = 4;

fn backoff_ms(attempt: u32) -> u64 {
    match attempt {
        0 => 3 * SECOND_IN_MS,
        1 => 5 * SECOND_IN_MS,
        2 => 15 * SECOND_IN_MS,
        3 => 45 * SECOND_IN_MS,
        _ => 120 * SECOND_IN_MS,
    }
}

/// Spec §5 completion window is anchored on `receipt_committed_at`; fall back to frozen
/// commitment certificate time when the draft is gone.
pub(crate) fn give_up_anchor_ns(receipt_committed_at_ns: Option<u64>, frozen_certificate_time_ns: u64) -> u64 {
    receipt_committed_at_ns.unwrap_or(frozen_certificate_time_ns)
}

pub(crate) fn start_if_required(state: &RuntimeState) -> bool {
    let pending = !state.data.cvdr.frozen_receipt_ids_missing_index_evidence().is_empty();
    if TIMER_ID.with(|t| t.get().is_none()) && pending {
        let timer_id = ic_cdk_timers::set_timer(Duration::from_millis(SWEEP_INTERVAL_MS), run_sweep);
        TIMER_ID.with(|t| t.set(Some(timer_id)));
        true
    } else {
        false
    }
}

fn run_sweep() {
    TIMER_ID.with(|t| t.set(None));
    let now_ns = ic_cdk::api::time();

    let due: Vec<(Hash, u64)> = mutate_state(|state| {
        let mut due = Vec::new();
        for receipt_id in state.data.cvdr.frozen_receipt_ids_missing_index_evidence() {
            let Some(pkg) = state.data.cvdr.get_frozen_package(&receipt_id) else {
                continue;
            };
            let receipt_committed_at = state.data.cvdr.receipt_committed_at_ns(&receipt_id);
            let anchor_ns = give_up_anchor_ns(receipt_committed_at, pkg.certificate_time);
            let age_from_anchor_ms = now_ns.saturating_sub(anchor_ns) / NS_PER_MS;
            if age_from_anchor_ms > GIVE_UP_MS {
                ATTEMPTS.with(|m| {
                    m.borrow_mut().remove(&receipt_id);
                });
                warn!(
                    event = "cvdr_index_evidence_give_up",
                    receipt_prefix = %cvdr::receipt_id_prefix(&receipt_id),
                    "INDEX evidence not captured within 24h of receipt_committed_at; leaving UNAVAILABLE"
                );
                continue;
            }
            let attempt_state = ATTEMPTS.with(|m| *m.borrow().get(&receipt_id).unwrap_or(&AttemptState::default()));
            let due_at_ns = if attempt_state.attempt == 0 {
                anchor_ns.saturating_add(backoff_ms(0) * NS_PER_MS)
            } else {
                attempt_state
                    .last_attempt_at_ns
                    .saturating_add(backoff_ms(attempt_state.attempt) * NS_PER_MS)
            };
            if now_ns >= due_at_ns {
                due.push((receipt_id, pkg.certificate_time));
            }
        }
        due
    });

    for (receipt_id, commitment_time) in due.into_iter().take(MAX_CONCURRENT_CAPTURES) {
        ATTEMPTS.with(|m| {
            let mut map = m.borrow_mut();
            let entry = map.entry(receipt_id).or_default();
            entry.attempt = entry.attempt.saturating_add(1);
            entry.last_attempt_at_ns = now_ns;
        });
        ic_cdk::futures::spawn(attempt_capture(receipt_id, commitment_time));
    }

    mutate_state(|state| {
        start_if_required(state);
    });
}

async fn attempt_capture(receipt_id: Hash, commitment_certificate_time_ns: u64) {
    let self_id = read_state(|state| state.env.canister_id());
    let certificate = match fetch_module_hash_certificate(self_id, MAX_RESPONSE_BYTES).await {
        Some(c) => Some(c),
        None => fetch_module_hash_certificate(self_id, RETRY_RESPONSE_BYTES).await,
    };
    let Some(certificate) = certificate else {
        trace!(receipt_prefix = %cvdr::receipt_id_prefix(&receipt_id), "INDEX read_state miss; will retry");
        return;
    };

    mutate_state(|state| {
        if state.data.cvdr.has_index_evidence(&receipt_id) {
            return;
        }
        let Some(frozen) = state.data.cvdr.get_frozen_package(&receipt_id) else {
            return;
        };
        let commitment_time = frozen.certificate_time.max(commitment_certificate_time_ns);
        let now = state.env.now();
        let ic_root_key = state.env.ic_root_key();

        match cvdr::verify_index_module_hash_evidence(&certificate, self_id, &ic_root_key, now, Some(commitment_time)) {
            Ok(_verified) => {
                let evidence = IndexCodeIdentityEvidence {
                    certificate_bytes: certificate,
                };
                match state.data.cvdr.insert_index_evidence(receipt_id, evidence) {
                    Ok(()) | Err(IndexEvidenceInsertError::AlreadyExists) => {
                        ATTEMPTS.with(|m| {
                            m.borrow_mut().remove(&receipt_id);
                        });
                        trace!(receipt_prefix = %cvdr::receipt_id_prefix(&receipt_id), "INDEX module hash evidence stored");
                    }
                    Err(IndexEvidenceInsertError::EmptyCertificate) => {
                        warn!(event = "cvdr_index_evidence_empty", receipt_prefix = %cvdr::receipt_id_prefix(&receipt_id));
                    }
                    Err(IndexEvidenceInsertError::FrozenPackageMissing) => {}
                    Err(IndexEvidenceInsertError::LogFull) => {
                        warn!(event = "cvdr_index_evidence_log_full", receipt_prefix = %cvdr::receipt_id_prefix(&receipt_id));
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
    });
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
    use crate::model::cvdr::{IndexEvidenceRejectReason, check_index_cert_not_before_commitment};

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
    fn give_up_prefers_receipt_committed_at() {
        assert_eq!(give_up_anchor_ns(Some(100), 999), 100);
        assert_eq!(give_up_anchor_ns(None, 999), 999);
    }

    #[test]
    fn cert_time_before_commitment_is_rejected() {
        assert_eq!(
            check_index_cert_not_before_commitment(10, 20),
            Err(IndexEvidenceRejectReason::CertTimeBeforeCommitment)
        );
        assert_eq!(check_index_cert_not_before_commitment(20, 20), Ok(()));
        assert_eq!(check_index_cert_not_before_commitment(21, 20), Ok(()));
    }

    #[test]
    fn concurrent_inserts_keep_first_evidence() {
        use crate::model::cvdr::{CvdrStore, FrozenCvdrPackage, receipt_id_for, record_id_for};
        use crate::model::cvdr_index_evidence::{IndexCodeIdentityEvidence, IndexEvidenceInsertError};
        use candid::Principal;

        let mut s = CvdrStore::default();
        let user = Principal::from_slice(&[9u8; 29]);
        let record_id = record_id_for(user.into());
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
                IndexCodeIdentityEvidence {
                    certificate_bytes: vec![0xaa],
                }
            ),
            Ok(())
        );
        assert_eq!(
            s.insert_index_evidence(
                receipt_id,
                IndexCodeIdentityEvidence {
                    certificate_bytes: vec![0xbb],
                }
            ),
            Err(IndexEvidenceInsertError::AlreadyExists)
        );
        assert_eq!(s.get_index_evidence(&receipt_id).unwrap().certificate_bytes, vec![0xaa]);
    }
}
