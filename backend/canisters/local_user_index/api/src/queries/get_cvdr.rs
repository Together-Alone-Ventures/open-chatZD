use candid::CandidType;
use serde::{Deserialize, Serialize};

/// Portable frozen-package schema id (CVDR-Verify `package.rs`, `FROZEN_SCHEMA_ID`).
pub const FROZEN_SCHEMA_ID: &str = "openchatzd.cvdr.frozen_package";
/// Portable package schema id (unchanged across versions; the `version` field dispatches).
pub const PORTABLE_SCHEMA_ID: &str = "openchatzd.cvdr.portable_package";
/// Status-body schema id (spec §11.2).
pub const STATUS_SCHEMA_ID: &str = "openchatzd.cvdr.status";
/// FrozenWire schema version.
pub const WIRE_VERSION: u64 = 1;
/// PortablePackageV3 schema version (Brief B1 R-6). Exact match on the wire; a verifier fails closed
/// on any other version. Version 2 (certificate-only evidence, no `trust_root_key_id`) is historical
/// and is never emitted.
pub const PORTABLE_VERSION: u32 = 3;
/// `trust_root_key_id` stamped when the Index's configured root key is the IC mainnet NNS key.
pub const TRUST_ROOT_MAINNET: &str = "mainnet";
/// `trust_root_key_id` stamped for any other configured root key (PocketIC, local replica): a
/// self-labelled NON-PRODUCTION id, so a package can never pass as mainnet-attested by omission.
pub const TRUST_ROOT_NON_PRODUCTION: &str = "non-production-test-root";
/// Byte encoding of the JSON transport. Only `hex` is emitted.
pub const WIRE_ENCODING: &str = "hex";
/// Client politeness hint (spec §11.2); not a promise.
pub const PENDING_RETRY_AFTER_SECS: u32 = 5;

#[derive(CandidType, Serialize, Deserialize, Debug)]
pub struct Args {
    /// The unguessable bearer capability (spec §11.1). There is no lookup by `record_id`,
    /// user, or principal on any public surface.
    pub receipt_id: [u8; 32],
}

/// Spec §11.2 states (malformed/`400` is HTTP-only — candid `Args.receipt_id` is `[u8; 32]`).
#[derive(CandidType, Deserialize, Debug)]
pub enum Response {
    Available(AvailablePackage),
    Pending(PendingInfo),
    NotFound,
}

/// Dual Available shapes (spec §11.2): FrozenWire commitment-only, or PortablePackageV3 when
/// INDEX evidence is stored. Nested enum keeps a single `Available` candid arm. The two are
/// structurally distinct: a package claiming index evidence (`PortablePackageV3`) cannot omit the
/// certificate, the extracted hash or the trust-root id, and a `FrozenWire` never carries them.
#[derive(CandidType, Deserialize, Debug, Clone)]
pub enum AvailablePackage {
    FrozenWire(FrozenWire),
    PortablePackageV3(PortablePackageV3Wire),
}

/// The portable frozen package (spec §4) as served on both surfaces — P0 interface lock:
/// public API serves FrozenWire verbatim, never a re-projection (§11.1).
///
/// Constructed in exactly ONE place (`impl From<&FrozenCvdrPackage>`) and serialized to JSON
/// in exactly ONE place ([`FrozenWire::to_canonical_json`]). Does NOT derive `Serialize`: a
/// derived JSON encoding would emit byte fields as arrays and silently become a second,
/// non-canonical serialization. `root_key_hex` is fixture-only and never emitted here.
#[derive(CandidType, Deserialize, Debug, Clone, PartialEq, Eq)]
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
    /// Canonical FrozenWire JSON bytes (spec §11.3 Gate A). HTTP `200` body and candid
    /// re-serialization for Gate A both go through this method.
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

/// INDEX evidence inside PortablePackageV3 (R-2 / R-6): the `read_state` certificate for
/// `/canister/<index>/module_hash` AND the module hash extracted from it at the store-gate.
#[derive(CandidType, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct IndexCodeIdentityEvidenceWire {
    pub certificate_bytes: Vec<u8>,
    pub index_module_hash: Vec<u8>,
}

/// PortablePackageV3 wire (R-6). `frozen` is the exact Gate A FrozenWire JSON bytes;
/// `trust_root_key_id` is stamped by the Index from its own configuration at the store-gate —
/// never client-supplied, never inferred from the certificate.
#[derive(CandidType, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct PortablePackageV3Wire {
    pub schema: String,
    pub version: u32,
    pub encoding: String,
    pub trust_root_key_id: String,
    pub frozen: Vec<u8>,
    pub index_code_identity_evidence: IndexCodeIdentityEvidenceWire,
}

#[derive(Serialize)]
struct CanonicalIndexEvidence {
    certificate_bytes: String,
    index_module_hash: String,
}

/// Canonical PortablePackageV3 JSON field order (part of the wire contract — the serializer
/// preserves declaration order): `schema, version, encoding, trust_root_key_id, frozen,
/// index_code_identity_evidence{certificate_bytes, index_module_hash}`. Byte fields lowercase hex.
#[derive(Serialize)]
struct CanonicalPortablePackageV3<'a> {
    schema: &'a str,
    version: u32,
    encoding: &'a str,
    trust_root_key_id: &'a str,
    /// Hex of the exact nested FrozenWire JSON bytes (Gate B / §14.1).
    frozen: String,
    index_code_identity_evidence: CanonicalIndexEvidence,
}

impl PortablePackageV3Wire {
    pub fn new(
        frozen_wire_json_bytes: Vec<u8>,
        certificate_bytes: Vec<u8>,
        index_module_hash: Vec<u8>,
        trust_root_key_id: String,
    ) -> Self {
        Self {
            schema: PORTABLE_SCHEMA_ID.to_string(),
            version: PORTABLE_VERSION,
            encoding: WIRE_ENCODING.to_string(),
            trust_root_key_id,
            frozen: frozen_wire_json_bytes,
            index_code_identity_evidence: IndexCodeIdentityEvidenceWire {
                certificate_bytes,
                index_module_hash,
            },
        }
    }

    /// Canonical PortablePackageV3 JSON bytes (spec §11.3 Gate B).
    pub fn to_canonical_json(&self) -> Vec<u8> {
        let canonical = CanonicalPortablePackageV3 {
            schema: &self.schema,
            version: self.version,
            encoding: &self.encoding,
            trust_root_key_id: &self.trust_root_key_id,
            frozen: hex::encode(&self.frozen),
            index_code_identity_evidence: CanonicalIndexEvidence {
                certificate_bytes: hex::encode(&self.index_code_identity_evidence.certificate_bytes),
                index_module_hash: hex::encode(&self.index_code_identity_evidence.index_module_hash),
            },
        };
        serde_json::to_vec(&canonical).expect("PortablePackageV3 canonical serialization")
    }
}

impl AvailablePackage {
    /// HTTP/candid Gate A/B body bytes for the Available payload.
    pub fn to_canonical_json(&self) -> Vec<u8> {
        match self {
            AvailablePackage::FrozenWire(w) => w.to_canonical_json(),
            AvailablePackage::PortablePackageV3(p) => p.to_canonical_json(),
        }
    }
}

/// Pending body (spec §11.2). Leaks nothing: status and retry hint only.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_frozen() -> FrozenWire {
        FrozenWire {
            schema: FROZEN_SCHEMA_ID.to_string(),
            version: WIRE_VERSION,
            encoding: WIRE_ENCODING.to_string(),
            receipt_body: vec![0xab, 0xcd],
            receipt_hash: [1u8; 32],
            tree_root: [2u8; 32],
            witness_bytes: vec![0x11],
            certificate_bytes: vec![0x22],
            certificate_time: 42,
        }
    }

    #[test]
    fn frozen_wire_canonical_json_is_deterministic_hex() {
        let a = sample_frozen().to_canonical_json();
        let b = sample_frozen().to_canonical_json();
        assert_eq!(a, b);
        let s = String::from_utf8(a).unwrap();
        assert!(s.contains(r#""schema":"openchatzd.cvdr.frozen_package""#));
        assert!(s.contains(r#""receipt_body":"abcd""#));
        assert!(s.contains(r#""certificate_time":42"#));
        assert!(!s.contains(' '));
    }

    #[test]
    fn portable_v3_nested_frozen_equals_gate_a_bytes_and_pins_field_order() {
        let frozen = sample_frozen();
        let gate_a = frozen.to_canonical_json();
        let v3 = PortablePackageV3Wire::new(
            gate_a.clone(),
            vec![0xde, 0xad],
            vec![0x1d; 32],
            TRUST_ROOT_MAINNET.to_string(),
        );
        assert_eq!(v3.frozen, gate_a);
        assert_eq!(v3.version, 3);
        let s = String::from_utf8(v3.to_canonical_json()).unwrap();
        assert!(s.contains(r#""schema":"openchatzd.cvdr.portable_package""#));
        assert!(s.contains(r#""version":3"#));
        assert!(s.contains(r#""trust_root_key_id":"mainnet""#));
        assert!(s.contains(&format!(r#""frozen":"{}""#, hex::encode(&gate_a))));
        assert!(s.contains(r#""certificate_bytes":"dead""#));
        assert!(s.contains(&format!(r#""index_module_hash":"{}""#, "1d".repeat(32))));
        assert!(!s.contains(' '));
        // canonical field order is part of the wire contract
        let pos = |k: &str| s.find(&format!("\"{k}\":")).unwrap_or_else(|| panic!("missing {k}"));
        let order = [
            "schema",
            "version",
            "encoding",
            "trust_root_key_id",
            "frozen",
            "index_code_identity_evidence",
            "certificate_bytes",
            "index_module_hash",
        ];
        assert!(
            order.windows(2).all(|w| pos(w[0]) < pos(w[1])),
            "PortablePackageV3 field order: {s}"
        );
    }
}
