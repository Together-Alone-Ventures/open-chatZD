use candid::CandidType;
use serde::{Deserialize, Serialize};

/// Portable frozen-package schema id (CVDR-Verify `package.rs`, `FROZEN_SCHEMA_ID`).
pub const FROZEN_SCHEMA_ID: &str = "openchatzd.cvdr.frozen_package";
/// Status-body schema id (spec §11.2).
pub const STATUS_SCHEMA_ID: &str = "openchatzd.cvdr.status";
/// Wire schema version for both of the above.
pub const WIRE_VERSION: u64 = 1;
/// Byte encoding of the FrozenWire JSON transport. Only `hex` is emitted.
pub const WIRE_ENCODING: &str = "hex";
/// Client politeness hint (spec §11.2); not a promise.
pub const PENDING_RETRY_AFTER_SECS: u32 = 5;

#[derive(CandidType, Serialize, Deserialize, Debug)]
pub struct Args {
    /// The unguessable bearer capability (spec §11.1). There is no lookup by `record_id`,
    /// user, or principal on any public surface.
    pub receipt_id: [u8; 32],
}

/// The four §11.2 states, minus the malformed/`400` case: candid `Args.receipt_id` is
/// typed `[u8; 32]`, so a malformed id is a decode error rather than a variant. Only the
/// HTTP surface can distinguish malformed (`400`) from unknown (`404`).
#[derive(CandidType, Deserialize, Debug)]
pub enum Response {
    Available(FrozenWire),
    Pending(PendingInfo),
    NotFound,
}

/// The portable frozen package (spec §4) as served on both surfaces — the P0 interface lock:
/// the public API serves FrozenWire verbatim, never a re-projection (§11.1).
///
/// This type is constructed in exactly ONE place (`impl From<&FrozenCvdrPackage>` in the impl
/// crate's `model::cvdr`) and serialized to JSON in exactly ONE place
/// ([`FrozenWire::to_canonical_json`]). That is what makes §11.3's three-way byte equality a
/// property of the code rather than of the test. Deliberately does NOT derive `Serialize`: a
/// derived JSON encoding would emit the byte fields as arrays and silently become a second,
/// non-canonical serialization.
///
/// `root_key_hex` is a CVDR-Verify *fixture-only* field and is never emitted here.
#[derive(CandidType, Deserialize, Debug, Clone)]
pub struct FrozenWire {
    pub schema: String,
    pub version: u64,
    pub encoding: String,
    pub receipt_body: Vec<u8>,
    pub receipt_hash: [u8; 32],
    pub tree_root: [u8; 32],
    pub witness_bytes: Vec<u8>,
    pub certificate_bytes: Vec<u8>,
    pub certificate_time: u64,
}

/// Field order here IS the canonical serialization order (serde_json preserves struct
/// declaration order). Byte fields are lowercase hex; `certificate_time` is a JSON number.
#[derive(Serialize)]
struct CanonicalFrozenWire<'a> {
    schema: &'a str,
    version: u64,
    encoding: &'a str,
    receipt_body: String,
    receipt_hash: String,
    tree_root: String,
    witness_bytes: String,
    certificate_bytes: String,
    certificate_time: u64,
}

impl FrozenWire {
    /// The canonical FrozenWire JSON bytes (spec §11.3). The HTTP `200` body is exactly these
    /// bytes; the byte-equality gate re-serializes the candid `Available` payload through this
    /// same method. Deterministic: fixed field order, lowercase hex, no whitespace.
    pub fn to_canonical_json(&self) -> Vec<u8> {
        let canonical = CanonicalFrozenWire {
            schema: &self.schema,
            version: self.version,
            encoding: &self.encoding,
            receipt_body: hex::encode(&self.receipt_body),
            receipt_hash: hex::encode(self.receipt_hash),
            tree_root: hex::encode(self.tree_root),
            witness_bytes: hex::encode(&self.witness_bytes),
            certificate_bytes: hex::encode(&self.certificate_bytes),
            certificate_time: self.certificate_time,
        };
        serde_json::to_vec(&canonical).expect("FrozenWire canonical serialization")
    }
}

/// The Pending body (spec §11.2). Leaks nothing: no draft contents, no draft-derived
/// timestamps, no target data — status and retry hint only.
#[derive(CandidType, Serialize, Deserialize, Debug, Clone)]
pub struct PendingInfo {
    pub schema: String,
    pub version: u64,
    pub status: String,
    pub retry_after_secs: u32,
}

impl Default for PendingInfo {
    fn default() -> Self {
        Self {
            schema: STATUS_SCHEMA_ID.to_string(),
            version: WIRE_VERSION,
            status: "pending".to_string(),
            retry_after_secs: PENDING_RETRY_AFTER_SECS,
        }
    }
}
