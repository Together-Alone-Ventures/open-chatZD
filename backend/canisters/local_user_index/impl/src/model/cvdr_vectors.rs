//! OpenChatZD suite-v5 test-vector corpus (plan Step 9; Brief B1 §2 step 6) and Antoine's
//! finaliser invariants (plan Step 8) as named tests.
//!
//! The corpus lives at `docs/test-vectors/openchatzd-v5/` and is **hash-gated**: every vector is
//! regenerated here from the normative formulas in [`super::cvdr`] and the wire encoders in
//! `local_user_index_canister::get_cvdr`, and must equal the committed file byte for byte
//! (`manifest.json` pins each file's SHA-256). Set `OPENCHATZD_WRITE_VECTORS=1` to (re)write them —
//! a diff is a formula or encoding change and must be ruled, never absorbed.
//!
//! Positive vectors: `receipt_id`, `record_id_v2`, `RECEIPT_BODY_V2` + leaf, canonical FrozenWire
//! / PortablePackageV3 / RevealWire v2 JSON. Negative vectors: a body/package that the verifier must
//! reject, each naming the check that rejects it (consumed by CVDR-Verify
//! `tests/openchatzd_v5_corpus.rs`).

#![cfg(test)]

use super::cvdr::{self, receipt_body_v2, receipt_id_for, receipt_leaf, record_id_v2, targets_commitment};
use candid::Principal;
use local_user_index_canister::get_cvdr::{
    FROZEN_SCHEMA_ID, FrozenWire, PortablePackageV3Wire, TRUST_ROOT_MAINNET, WIRE_ENCODING, WIRE_VERSION,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;

const CORPUS_DIR: &str = "../../../../docs/test-vectors/openchatzd-v5";

fn p(b: u8) -> Principal {
    Principal::from_slice(&[b; 10])
}

/// The fixed inputs every vector derives from (deterministic, documented in each file).
struct Inputs {
    record_salt: [u8; 32],
    user_id: Principal,
    index_id: Principal,
    deletion_seq: u64,
    nonce: [u8; 32],
    module_hash_pre: Vec<u8>,
    salt: [u8; 32],
    targets: Vec<Principal>,
    uninstall_ns: u64,
    committed_ns: u64,
}

fn inputs() -> Inputs {
    Inputs {
        record_salt: [0xC3; 32],
        user_id: p(2),
        index_id: p(3),
        deletion_seq: 5,
        nonce: [2u8; 32],
        module_hash_pre: vec![9u8; 32],
        salt: [4u8; 32],
        targets: vec![p(8), p(6)],
        uninstall_ns: 111,
        committed_ns: 222,
    }
}

struct Derived {
    record_id: [u8; 32],
    receipt_id: [u8; 32],
    h_user_pre: [u8; 32],
    targets_commitment: [u8; 32],
    body: Vec<u8>,
    leaf: [u8; 32],
}

fn derive(i: &Inputs) -> Derived {
    let record_id = record_id_v2(&i.record_salt, i.user_id.into());
    let receipt_id = receipt_id_for(&record_id, i.deletion_seq, &i.nonce);
    let h_user_pre = cvdr::h_user_pre(i.user_id, &i.module_hash_pre);
    let tc = targets_commitment(&i.salt, &i.targets);
    let body = receipt_body_v2(
        &receipt_id,
        &i.nonce,
        i.index_id,
        i.user_id,
        &record_id,
        i.deletion_seq,
        &h_user_pre,
        i.uninstall_ns,
        i.committed_ns,
        i.targets.len() as u32,
        &tc,
    );
    let leaf = receipt_leaf(&body);
    Derived {
        record_id,
        receipt_id,
        h_user_pre,
        targets_commitment: tc,
        body,
        leaf,
    }
}

fn hx(b: &[u8]) -> String {
    hex::encode(b)
}

/// The RETIRED `record_id` derivation (`OPENCHATZD_RECORD_ID_USER_V1` ‖ UserId), computed here only
/// to pin that the live derivation never equals it; the canister no longer carries it.
fn retired_record_id_v1(user_id: Principal) -> [u8; 32] {
    sha256::sha256(&[b"OPENCHATZD_RECORD_ID_USER_V1".as_slice(), user_id.as_slice()].concat())
}

/// Every vector file, in corpus order: (file name, JSON document).
fn vectors() -> Vec<(&'static str, Value)> {
    let i = inputs();
    let d = derive(&i);
    let mut sorted = i.targets.clone();
    sorted.sort_by(|a, b| a.as_slice().cmp(b.as_slice()));
    let frozen = FrozenWire {
        schema: FROZEN_SCHEMA_ID.to_string(),
        version: WIRE_VERSION,
        encoding: WIRE_ENCODING.to_string(),
        receipt_body: d.body.clone(),
        receipt_hash: d.leaf,
        tree_root: [0x7Au8; 32],
        witness_bytes: vec![0xAA, 0xBB],
        certificate_bytes: vec![0xCC, 0xDD],
        certificate_time: 333,
    };
    let frozen_bytes = frozen.to_canonical_json();
    let v3 = PortablePackageV3Wire::new(
        frozen_bytes.clone(),
        vec![0xEE, 0xFF],
        vec![0x1Du8; 32],
        TRUST_ROOT_MAINNET.to_string(),
    );
    let reveal = cvdr::reveal_wire_canonical_json(&i.salt, &i.record_salt, &i.targets);
    let common = json!({
        "record_salt": hx(&i.record_salt),
        "user_id_principal": i.user_id.to_text(),
        "index_canister_id": i.index_id.to_text(),
        "deletion_seq": i.deletion_seq,
        "nonce": hx(&i.nonce),
        "module_hash_pre": hx(&i.module_hash_pre),
        "targets_salt": hx(&i.salt),
        "targets": i.targets.iter().map(|t| t.to_text()).collect::<Vec<_>>(),
        "uninstall_completed_at_ns": i.uninstall_ns,
        "receipt_committed_at_ns": i.committed_ns,
    });

    // ---- a V1-shaped (historical) body from the same fields, for the negative vectors
    let mut v1_body = d.body.clone();
    v1_body[..26].copy_from_slice(b"OPENCHATZD_RECEIPT_BODY_V1");
    let hu_end = 26 + 64 + 22 + 32 + 8 + 32;
    for _ in 0..64 {
        v1_body.insert(hu_end, 0x55);
    }
    let mut v2_tag_v1_layout = v1_body.clone();
    v2_tag_v1_layout[..26].copy_from_slice(b"OPENCHATZD_RECEIPT_BODY_V2");
    let mut altered_rid = d.body.clone();
    altered_rid[26] ^= 0xFF;
    let mut altered_nonce = d.body.clone();
    altered_nonce[58] ^= 0xFF;
    let mut altered_field = d.body.clone();
    altered_field[26 + 64 + 22 + 32 + 8] ^= 0xFF; // h_user_pre
    let identifying = {
        // the retired UserId-only derivation in a V2 body: receipt_id recomputes over it, so only a
        // RevealWire v2 linkage (record_salt) exposes it
        let bad_record = retired_record_id_v1(i.user_id);
        let rid = receipt_id_for(&bad_record, i.deletion_seq, &i.nonce);
        receipt_body_v2(
            &rid,
            &i.nonce,
            i.index_id,
            i.user_id,
            &bad_record,
            i.deletion_seq,
            &d.h_user_pre,
            i.uninstall_ns,
            i.committed_ns,
            i.targets.len() as u32,
            &d.targets_commitment,
        )
    };

    vec![
        (
            "pv5-001-receipt_id.json",
            json!({
                "vector": "pv5-001", "kind": "positive", "formula": "receipt_id = SHA256(\"OPENCHATZD_CVDR_RECEIPT_V1\" ‖ record_id ‖ deletion_seq(u64 BE) ‖ nonce)",
                "inputs": {"record_id": hx(&d.record_id), "deletion_seq": i.deletion_seq, "nonce": hx(&i.nonce)},
                "expected": {"receipt_id": hx(&d.receipt_id)},
            }),
        ),
        (
            "pv5-002-record_id_v2.json",
            json!({
                "vector": "pv5-002", "kind": "positive", "formula": "record_id = SHA256(\"OPENCHATZD_RECORD_ID_USER_V2\" ‖ record_salt(32) ‖ canonical UserId principal bytes)  (R-1; non-identifying)",
                "inputs": {"record_salt": hx(&i.record_salt), "user_id_principal": i.user_id.to_text(), "user_id_principal_bytes": hx(i.user_id.as_slice())},
                "expected": {"record_id": hx(&d.record_id)},
                "must_not_equal": {"retired_v1_derivation": hx(&retired_record_id_v1(i.user_id))},
            }),
        ),
        (
            "pv5-003-receipt_body_v2.json",
            json!({
                "vector": "pv5-003", "kind": "positive",
                "layout": "\"OPENCHATZD_RECEIPT_BODY_V2\"(26) ‖ receipt_id(32) ‖ nonce(32) ‖ len‖index ‖ len‖user ‖ record_id(32) ‖ deletion_seq(u64 BE) ‖ h_user_pre(32) ‖ uninstall_completed_at(u64 BE ns) ‖ receipt_committed_at(u64 BE ns) ‖ targets_count(u32 BE) ‖ targets_commitment(32); leaf = SHA256(\"OPENCHATZD_RECEIPT_LEAF_V1\" ‖ body)",
                "inputs": common,
                "derived": {"record_id": hx(&d.record_id), "receipt_id": hx(&d.receipt_id), "h_user_pre": hx(&d.h_user_pre), "targets_commitment": hx(&d.targets_commitment)},
                "expected": {"receipt_body": hx(&d.body), "receipt_body_len": d.body.len(), "leaf": hx(&d.leaf)},
            }),
        ),
        (
            "pv5-004-canonical_package.json",
            json!({
                "vector": "pv5-004", "kind": "positive",
                "note": "canonical JSON = serde declaration order, no whitespace, lowercase hex; PortablePackageV3 nests the exact FrozenWire bytes (Gate B). tree_root/witness/certificate here are placeholders — encoding only, not a verifiable package.",
                "frozen_fields": {"tree_root": hx(&frozen.tree_root), "witness_bytes": hx(&frozen.witness_bytes), "certificate_bytes": hx(&frozen.certificate_bytes), "certificate_time": frozen.certificate_time},
                "evidence_fields": {"certificate_bytes": "eeff", "index_module_hash": hx(&[0x1Du8; 32]), "trust_root_key_id": TRUST_ROOT_MAINNET},
                "expected": {
                    "frozen_wire_canonical_json": String::from_utf8(frozen_bytes.clone()).unwrap(),
                    "frozen_wire_sha256": hx(&sha256::sha256(&frozen_bytes)),
                    "portable_package_v3_canonical_json": String::from_utf8(v3.to_canonical_json()).unwrap(),
                    "portable_package_v3_sha256": hx(&sha256::sha256(&v3.to_canonical_json())),
                    "reveal_wire_v2_canonical_json": String::from_utf8(reveal.clone()).unwrap(),
                },
            }),
        ),
        (
            "nv5-001-old_tag_and_prefix_dispatch.json",
            json!({
                "vector": "nv5-001", "kind": "negative", "rejected_by": "V1 body: tag dispatch + parse / PortablePackage version",
                "cases": [
                    {"name": "V1 tag with V1 layout inside a V3 package", "receipt_body": hx(&v1_body), "package_version": 3, "expect": "v1:version-tag-mismatch"},
                    {"name": "V2 tag over the V1 layout (64 trailing bytes)", "receipt_body": hx(&v2_tag_v1_layout), "package_version": 3, "expect": "v1:body-malformed"},
                    {"name": "future tag", "receipt_body": hx(&{ let mut b = d.body.clone(); b[..26].copy_from_slice(b"OPENCHATZD_RECEIPT_BODY_V3"); b }), "package_version": 3, "expect": "v1:body-malformed"},
                    {"name": "prefix-match package version 30", "receipt_body": hx(&d.body), "package_version": 30, "expect": "malformed"},
                    {"name": "prefix-match schema", "receipt_body": hx(&d.body), "package_version": 3, "schema": "openchatzd.cvdr.portable_package.v3", "expect": "malformed"},
                ],
            }),
        ),
        (
            "nv5-002-altered_receipt_id.json",
            json!({
                "vector": "nv5-002", "kind": "negative", "rejected_by": "V1 receipt_id recompute",
                "cases": [
                    {"name": "receipt_id byte flipped", "receipt_body": hx(&altered_rid), "expect": "v1:receipt-id"},
                    {"name": "nonce byte flipped", "receipt_body": hx(&altered_nonce), "expect": "v1:receipt-id"},
                ],
            }),
        ),
        (
            "nv5-003-identifying_record_id.json",
            json!({
                "vector": "nv5-003", "kind": "negative", "rejected_by": "V1 reveal: record_id_v2",
                "note": "a V2 body whose record_id is the retired UserId-only derivation: receipt_id still recomputes, so only the RevealWire v2 linkage check exposes it (the canister never emits it: record_id_v2 is the only live derivation)",
                "receipt_body": hx(&identifying),
                "reveal_wire_v2": String::from_utf8(reveal.clone()).unwrap(),
                "expect": "V1 reveal: record_id_v2 FAIL",
            }),
        ),
        (
            "nv5-004-module_hash_in_preimage_or_mismatched_evidence.json",
            json!({
                "vector": "nv5-004", "kind": "negative", "rejected_by": "V1 package version ↔ body tag / V3A",
                "cases": [
                    {"name": "deployer h_index + commitment inside the body (V1 layout) under a V3 package", "receipt_body": hx(&v1_body), "package_version": 3, "expect": "v1:version-tag-mismatch"},
                    {"name": "displayed index_module_hash != certified", "index_module_hash": "00".repeat(32), "expect": "INDEX_HASH_MISMATCH"},
                ],
            }),
        ),
        (
            "nv5-005-altered_body_field.json",
            json!({
                "vector": "nv5-005", "kind": "negative", "rejected_by": "V1 leaf == receipt_hash",
                "cases": [{"name": "h_user_pre byte flipped (receipt_id unaffected)", "receipt_body": hx(&altered_field), "expect": "V1 leaf == receipt_hash FAIL"}],
            }),
        ),
    ]
}

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(CORPUS_DIR)
}

fn manifest(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let map: BTreeMap<&str, String> = files.iter().map(|(n, b)| (*n, hx(&sha256::sha256(b)))).collect();
    let mut out = serde_json::to_vec_pretty(&json!({
        "corpus": "openchatzd-v5",
        "generated_by": "local_user_index_canister_impl::model::cvdr_vectors (hash-gated; OPENCHATZD_WRITE_VECTORS=1 to rewrite)",
        "files": map,
    }))
    .unwrap();
    out.push(b'\n');
    out
}

/// Hash gate: the committed corpus equals the regeneration from the live formulas, byte for byte.
#[test]
fn corpus_matches_committed_files_byte_for_byte() {
    let files: Vec<(&str, Vec<u8>)> = vectors()
        .into_iter()
        .map(|(n, v)| {
            let mut b = serde_json::to_vec_pretty(&v).unwrap();
            b.push(b'\n');
            (n, b)
        })
        .collect();
    let dir = corpus_dir();
    if std::env::var("OPENCHATZD_WRITE_VECTORS").is_ok() {
        std::fs::create_dir_all(&dir).unwrap();
        for (n, b) in &files {
            std::fs::write(dir.join(n), b).unwrap();
        }
        std::fs::write(dir.join("manifest.json"), manifest(&files)).unwrap();
    }
    let committed_manifest = std::fs::read(dir.join("manifest.json")).expect("corpus manifest committed");
    assert_eq!(
        committed_manifest,
        manifest(&files),
        "manifest.json drifted from the live formulas — rule it, do not absorb it"
    );
    for (n, b) in &files {
        let committed = std::fs::read(dir.join(n)).unwrap_or_else(|e| panic!("{n}: {e}"));
        assert_eq!(&committed, b, "{n} drifted from the live formulas");
    }
}

/// Cross-repo mirror guard: CVDR-Verify consumes a byte-identical mirror of this corpus
/// (`tests/fixtures/v5-openchatzd/corpus/`, driven through its CLI by `tests/openchatzd_v5_corpus.rs`).
/// Same resolution / skip semantics as the attestation label drift guard: absent sibling => skip;
/// present sibling without the mirror, or a differing manifest => fail loudly, naming the CI pin and
/// the sibling path (`sibling_precondition`).
#[test]
fn corpus_mirror_in_cvdr_verify_is_byte_identical() {
    use super::cvdr_index_attestation::{required_cvdr_verify_pin, resolve_cvdr_verify_root, sibling_precondition};
    let Some(root) = resolve_cvdr_verify_root() else {
        eprintln!(
            "CVDR-Verify sibling absent: corpus mirror guard skipped (CI pin {})",
            required_cvdr_verify_pin()
        );
        return;
    };
    let mirror = root.join("mktd02/mktd02-verify/tests/fixtures/v5-openchatzd/corpus");
    assert!(
        mirror.exists(),
        "CVDR-Verify present but the openchatzd-v5 corpus mirror is missing: {}. {}",
        mirror.display(),
        sibling_precondition(&root)
    );
    let dir = corpus_dir();
    for name in std::iter::once("manifest.json".to_string()).chain(vectors().into_iter().map(|(n, _)| n.to_string())) {
        let ours = std::fs::read(dir.join(&name)).unwrap();
        let theirs = std::fs::read(mirror.join(&name))
            .unwrap_or_else(|e| panic!("mirror missing {name}: {e}. {}", sibling_precondition(&root)));
        assert!(
            ours == theirs,
            "CVDR-Verify corpus mirror drifted: {name}. {}",
            sibling_precondition(&root)
        );
    }
}

/// Positive vectors are self-consistent under the formulas (independent of file I/O).
#[test]
fn positive_vectors_are_consistent() {
    let i = inputs();
    let d = derive(&i);
    assert_eq!(d.body.len(), 236);
    assert!(d.body.starts_with(b"OPENCHATZD_RECEIPT_BODY_V2"));
    assert_ne!(
        d.record_id,
        retired_record_id_v1(i.user_id),
        "record_id_v2 never equals the retired derivation"
    );
    assert!(
        !d.body.windows(32).any(|w| w == i.record_salt),
        "record_salt never in the body"
    );
}

// ===========================================================================================
// Antoine's finaliser architecture — regression invariants (plan Step 8), each a named test.
// The source-scan tests below (ownership, writer enumeration, gate-before-insert order, named-test
// presence) are REGRESSION TRIPWIRES, not behavioural proofs (ruling (b), Step 9): they fail loudly
// on a rename, move or added writer; the behaviour itself is proven by the units they point at.
// ===========================================================================================

fn lui_src(rel: &str) -> String {
    std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src").join(rel)).unwrap()
}

fn repo(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../..").join(rel)
}

/// Step 8 #4: CVDR-on-Index ownership remains in local_user_index; #5: the user canister is the
/// passive teardown target (no certified data, no receipt code of its own).
#[test]
fn invariant_cvdr_on_index_ownership_and_passive_user_target() {
    let user_src = repo("backend/canisters/user/impl/src");
    let mut hits = Vec::new();
    for entry in walkdir(&user_src) {
        let s = std::fs::read_to_string(&entry).unwrap();
        if s.contains("certified_data_set") || s.contains("mod cvdr") || s.contains("ReceiptTree") {
            hits.push(entry);
        }
    }
    assert!(hits.is_empty(), "user canister must stay the passive target: {hits:?}");
    assert!(lui_src("model/cvdr.rs").contains("pub struct ReceiptTree"));
}

/// Step 5 (plan §7) + Step 8 #9: exactly one certified_data meaning — every `certified_data_set`
/// writer in the Index publishes the ReceiptTree root, and only `delete_users.rs` writes it
/// (publish + post-upgrade rebuild).
#[test]
fn invariant_single_certified_data_meaning() {
    let mut writers = Vec::new();
    let needle = ["certified_data", "_set("].concat(); // not a literal, so this scanner does not match itself
    for entry in walkdir(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")) {
        let s = std::fs::read_to_string(&entry).unwrap();
        for line in s.lines() {
            let t = line.trim();
            if t.contains(&needle) && !t.starts_with("//") {
                writers.push((entry.file_name().unwrap().to_string_lossy().to_string(), t.to_string()));
            }
        }
    }
    assert_eq!(writers.len(), 2, "exactly two certified_data_set writers: {writers:?}");
    for (file, line) in &writers {
        assert_eq!(file, "delete_users.rs", "{line}");
        assert!(
            line.contains("cvdr_receipt_tree.root()"),
            "writer must publish the ReceiptTree root: {line}"
        );
    }
}

/// Plan §11 certified-data test: the genesis (empty) ReceiptTree root is deterministic and pinned.
#[test]
fn invariant_genesis_receipt_tree_root_is_deterministic() {
    let a = cvdr::ReceiptTree::default().root();
    let b = cvdr::ReceiptTree::default().root();
    assert_eq!(a, b);
    // Empty RbTree root = SHA256("ic-hashtree-empty") (IC interface spec); the certified root is
    // labeled_hash("receipts", that).
    let empty_root = sha256::sha256(b"\x11ic-hashtree-empty");
    assert_eq!(a, ic_certification::labeled_hash(b"receipts", &empty_root));
    assert_eq!(hx(&a), "36661ea7ac683d8bfae6f52860032a55137facbc1bb09b8509d46bd985af202d");
}

/// Step 8 #7 + #8: self-finalisation is primary, `finalize_cvdr` is the backstop, and BOTH run the
/// shared store-gate (`verify_finalization_package`) before `insert_frozen_package`.
#[test]
fn invariant_self_finalisation_primary_backstop_and_store_gate_order() {
    for rel in ["jobs/self_finalize_cvdr.rs", "updates/finalize_cvdr.rs"] {
        let s = lui_src(rel);
        let gate = s
            .find("cvdr::verify_finalization_package(")
            .unwrap_or_else(|| panic!("{rel}: store-gate missing"));
        let insert = s
            .find(".insert_frozen_package(")
            .unwrap_or_else(|| panic!("{rel}: insert missing"));
        assert!(gate < insert, "{rel}: the store-gate must precede the frozen insert");
    }
    assert!(
        lui_src("updates/finalize_cvdr.rs").contains("permissionless")
            || lui_src("updates/finalize_cvdr.rs").contains("backstop")
    );
}

/// Step 8 #6 (as amended by R-2/C2): captured Index evidence is historical and never overwritten
/// by a later read — the evidence store is insert-only and the gate refuses a second capture.
#[test]
fn invariant_index_evidence_is_never_overwritten() {
    assert_eq!(
        cvdr::evidence_store_gate(1, 1, 500, 100, true),
        Err(cvdr::EvidenceGateReject::AlreadyStored)
    );
    let s = lui_src("model/cvdr_index_evidence.rs");
    assert!(s.contains("IndexEvidenceInsertError::AlreadyExists"), "insert-only store");
}

/// Step 8 #1/#2/#3: dual-layout canister_ranges parsing, certificate-pair delay and
/// finalisation-age residual remain implemented — pinned by the presence of their named tests.
#[test]
fn invariant_antoine_named_tests_present() {
    let ranges = lui_src("model/cvdr_canister_ranges.rs");
    for name in [
        "fn legacy_layout_authorizes_in_range",
        "fn sharded_layout_authorizes_in_range",
        "fn sharded_layout_rejects_malformed_cbor_leaf",
        "fn rejects_when_ranges_signed_under_wrong_subnet",
    ] {
        assert!(ranges.contains(name), "dual-layout canister_ranges test missing: {name}");
    }
    let att = lui_src("model/cvdr_index_attestation.rs");
    for label in ["TIMING_DELAY_EXCEEDED", "TIMING_OUTSIDE_COMPLETION_WINDOW"] {
        assert!(
            att.contains(label),
            "certificate-pair delay / finalisation-age label missing: {label}"
        );
    }
}

/// Step 8 #9 + R-3 option (i): no MKTd03, MKTd02 or zombie-core dependency anywhere in the lockfile.
#[test]
fn invariant_no_ceremonial_dependency() {
    let lock = std::fs::read_to_string(repo("Cargo.lock")).unwrap();
    for name in ["name = \"mktd03\"", "name = \"mktd02\"", "name = \"zombie-core\""] {
        assert!(!lock.contains(name), "{name} must not be a dependency");
    }
}

fn walkdir(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(walkdir(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out
}
