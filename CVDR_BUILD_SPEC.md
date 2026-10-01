# OpenChatZD CVDR Build Spec — suite v5 (v2; ADOPTED via Brief B1)

**Status:** governing build spec for the OpenChatZD suite-v5 retrofit ("Zombie Delete",
rulings R-1…R-6 of `OpenChatZD_v5_Brief_B1`, adopted 2026-09-21; C2 amendment 2026-09-22).
**Supersedes** `CVDR_BUILD_SPEC_V1.md` (historical; kept for the M1–M4 record). Where this document
is silent, V1 §2–§11 and §13–§16 stand unchanged (receipt tree, atomicity, frozen package, timestamps,
self-finalization, backstop, delivery contract, scheduler, terminology).
**Base:** `origin/antek` @ `c744de1`; branch `v5-retrofit`; baseline and design record in
`docs/dev/v5/BASELINE_c744de1.md` (§5 interlock, §6 V3 wire, §7 verifier, §8 corpus/invariants).
**Authoritative once committed into this repo** (invariants-in-source rule). Release: **v0.8.0**
(untagged — tags are minted once, at the final release commit; see `RELEASES.md`).

---

## 1. What changes (delta from V1 / the M4 build)

| # | V1 / M4 state | suite v5 (this spec) | Ruling |
|---|---|---|---|
| 1 | `record_id = SHA256(tag ‖ UserId)` (identifying) | `record_id_v2 = SHA256("OPENCHATZD_RECORD_ID_USER_V2" ‖ record_salt(32) ‖ UserId bytes)`; `record_salt` fresh per deletion, delivered only in RevealWire v2 | R-1 |
| 2 | `h_index` / `commitment` in the receipt preimage (`RECEIPT_BODY_V1`) | `RECEIPT_BODY_V2 = V1 − {h_index, commitment}`; the Index module hash lives only in subnet-attested evidence | R-4 |
| 3 | Deployer-supplied `executor_module_hash` as evidence | Evidence = the authenticated `/canister/<LUI>/module_hash` certificate + extracted hash, captured after uninstall under the upgrade interlock; deployer value only an expectation (`expected_index_module_hash: Option`) | R-2 |
| 4 | Local MKTd02/MKTd03 ceremonial dependency | Removed (option (i)); `export_pending` map + MemoryId 4 retained for old records; MemoryIds 100–107 reserved, never reused | R-3 |
| 5 | Receipt identity/ordering | `receipt_id = SHA256("OPENCHATZD_CVDR_RECEIPT_V1" ‖ record_id ‖ deletion_seq(u64 BE) ‖ nonce)`; verifier recomputes from displayed fields | R-5 |
| 6 | `PortablePackageV2` | `PortablePackageV3` (+ `trust_root_key_id` selector); V2 never emitted, decoded by the verifier as historical only | R-6 |
| 7 | 24 h "give-up" on wall clock | Certified-time store-gate (epoch, window, absence) — wall clock only schedules | C2 |

## 2. Cryptographic formulas (FROZEN; pinned by `docs/test-vectors/openchatzd-v5/`)

```
record_id        = SHA256("OPENCHATZD_RECORD_ID_USER_V2" ‖ record_salt(32) ‖ user_principal_bytes)
h_user_pre       = SHA256("OPENCHATZD_CVDR_H_USER_V1"   ‖ user_canister_principal ‖ module_hash_pre)
receipt_id       = SHA256("OPENCHATZD_CVDR_RECEIPT_V1"  ‖ record_id ‖ deletion_seq(u64 BE) ‖ nonce(32))
targets_commit.  = SHA256("OPENCHATZD_TARGETS_COMMITMENT_V1" ‖ salt(32) ‖ concat(len(u8)‖principal over sorted targets))
RECEIPT_BODY_V2  = "OPENCHATZD_RECEIPT_BODY_V2" ‖ receipt_id(32) ‖ nonce(32) ‖ len‖index_canister_id ‖ len‖user_canister_id
                   ‖ record_id(32) ‖ deletion_seq(u64 BE) ‖ h_user_pre(32) ‖ uninstall_completed_at(u64 BE ns)
                   ‖ receipt_committed_at(u64 BE ns) ‖ targets_count(u32 BE) ‖ targets_commitment(32)
leaf             = SHA256("OPENCHATZD_RECEIPT_LEAF_V1" ‖ RECEIPT_BODY_V2)          (== FrozenCvdrPackage.receipt_hash)
tree path        = ["receipts", receipt_id]; certified root = labeled_hash("receipts", RbTree root)
genesis root     = 36661ea7ac683d8bfae6f52860032a55137facbc1bb09b8509d46bd985af202d
```

Retired, never reused: `OPENCHATZD_RECORD_ID_USER_V1`, `RECEIPT_BODY_V1` as a live tag. The
corpus is generated from the live formulas and hash-gated
(`local_user_index_canister_impl::model::cvdr_vectors`); a diff is a formula change and must be
ruled, never absorbed.

## 3. Privacy: `record_salt` and RevealWire v2

- `record_salt` (32 bytes, `raw_rand`) is drawn at `prepare_account_deletion`, stored in the
  durable draft, **scrubbed with the salt** once the receipt is committed, and never enters any
  receipt, log, metric or public surface. The receipt's `record_id` is therefore non-identifying:
  recomputable only by a holder of `record_salt`.
- RevealWire v2 (`openchatzd.cvdr.reveal_package`, `version: 2`, canonical JSON
  `schema, version, encoding, salt, record_salt, targets[sorted]`) is delivered in-session at
  deletion. A v1 reveal cannot link a V2 receipt; the verifier fails closed on the mismatch.
- Pre-V2 drafts (no `record_salt`) are refused at upgrade (`pre_v2_upgrade_refusal`): they must
  finalise or be purged under the old wasm. Zero such drafts existed on mainnet at c744de1.

## 4. Index code identity — evidence, interlock, store-gate (R-2 + C2)

**Claim (ratified wording, `OCZD_SUPPORTED_CLAIM`, identical in every propagation target §10):**
OpenChatZD carries subnet-attested installed module identity during the finalization/certification
window. The Index upgrade interlock bounds that window from uninstall to evidence capture: the same
Index code stays installed until the module-hash certificate is stored, so the certified module hash
is the code identity of the deleting Index. *Do not* say the certificate "proves which INDEX code
ran when the receipt was sealed". "Protected deletion→evidence interval" is the internal name only.

**Evidence.** `IndexCodeIdentityEvidence { certificate_bytes, index_module_hash, trust_root_key_id }`
— the complete `read_state` certificate for `/canister/<local_user_index>/module_hash`, the hash
extracted from it at the store-gate, and the trust-root id stamped from Index configuration
(`"mainnet"` iff the configured root equals the IC NNS key, else `"non-production-test-root"`).
Insert-only, first wins (MemoryIds 12/13/14). Never client-supplied, never inferred.

**Capture.** The self-capture sweep starts at `Uninstalled` (not at package storage) and issues a
non-replicated outcall to `read_state`; one certificate serves every draft it post-dates. Requests
expire after ~30 s if unanswered (IC canister-HTTP behaviour) and are simply re-issued.

**Store-gate (`cvdr::evidence_store_gate`, pure, wall clock is not an input).** Evidence is
admissible iff, checked *after* the async reply and *before* insert: (a) the code epoch captured
when the outcall was issued equals the current epoch (`cvdr_code_epoch_started_at_ns`, set at init
and every successful `post_upgrade`); (b) on the BLS-authenticated certificate `/time` `t`:
`uninstall_completed_at ≤ t ≤ uninstall_completed_at + 24 h`; (c) no evidence is stored for the
receipt. Any failure → discard, logged `cvdr_index_evidence_discarded{reason}`; never store.

> **Invariant (G, C2, verbatim):** V3A evidence is admissible only if the authenticated certificate
> /time is ≤ uninstall_completed_at + 24 h, the receipt's captured code epoch is still current, and
> no evidence has already been stored. Once the epoch changes, or the certificate time is outside
> that window, V3A is permanently unavailable.

**Upgrade interlock.** A draft blocks an upgrade iff `uninstall_completed_at > 0` ∧ stage ∉
{`Prepared`, `Captured`} ∧ no evidence ∧ `now − uninstall_completed_at ≤ 24 h` ∧
`uninstall_completed_at ≥ code epoch`. Checked twice with the same predicate, both before any
state change: first statement of `pre_upgrade` (authoritative for whatever wasm is incoming) and
in `post_upgrade::resume_in_flight_drafts` (covers upgrades from a pre-interlock wasm). A trap
fails `install_code`; the old wasm keeps serving. Refusal text: count, `prefix(Stage)` per blocker
(max 20), and the operator path (§9). Metric `cvdr_upgrade_blockers`.

**Permanently V3A-unavailable** = no qualifying certificate can now be accepted (epoch changed, or
every obtainable `/time` would exceed the bound). Never asserted merely because wall-clock 24 h
passed while a capture was in flight. Wall-clock 24 h releases only the block and the sweep.

## 5. Wire: FrozenWire (Gate A) and PortablePackageV3 (Gate B)

Unchanged delivery contract (V1 §11: `GET /cvdr/<receipt_id>` raw domain, `get_cvdr` Candid,
states Available / Pending `202` / Unknown `404`, bearer `receipt_id`, byte-equality gates), with
the Available shapes now:

```
FrozenWire   schema "openchatzd.cvdr.frozen_package" version 1 encoding "hex":
             receipt_body (RECEIPT_BODY_V2), receipt_hash, tree_root, witness_bytes, certificate_bytes, certificate_time
PortablePackageV3  schema "openchatzd.cvdr.portable_package" version 3 encoding "hex":
             schema, version, encoding, trust_root_key_id, frozen (hex of the exact FrozenWire bytes),
             index_code_identity_evidence { certificate_bytes, index_module_hash }
```

- Canonical JSON = serde declaration order, no whitespace, lowercase hex; stored bytes, HTTP bytes
  and the Candid re-serialisation agree byte for byte (Gate B test `available_v3_bytes_agree…`).
- `FrozenWire` (commitment-only, evidence absent) and `PortablePackageV3` are distinct Candid arms;
  a V3 package cannot omit the certificate, the extracted hash or the trust-root id. Evidence stored
  by a pre-step-4 wasm (no hash / no id) is served as `FrozenWire`, never projected into V3.
- `PortablePackageV2` is never emitted (zero mainnet packages); the verifier decodes it as historical.

## 6. Verifier contract (CVDR-Verify `openchatzd-v5`, `mktd02/mktd02-verify/`)

- **Dispatch (G rule 3):** exact body tag (`_V2` live, `_V1` historical) **and** package version
  (3 live, 2 historical): V3 ⇔ body V2, V2 ⇔ body V1, bare FrozenWire either; any other pairing
  or tag is malformed (`v1:version-tag-mismatch` / `v1:body-malformed`); never a prefix match.
- **Trust root = selector (G rule 1):** `trust_root_key_id` picks a verifier-configured root;
  `non-production-test-root` only under `--allow-fixture-root-key` with root material supplied out
  of band (`--fixture-root-key-hex`); unknown ids / conflicting `--trust-root-key-id` fail closed
  before anything is verified; a non-production root is announced in the verdict.
- **V3A — exactly three outcomes (G rule 2):** `V3A_PASS` (certificate authenticated under the
  selected root, `/time` ∈ [`uninstall_completed_at`, +24 h] — the same comparison as the Index
  store-gate — and displayed `index_module_hash` == certified), `V3A_PENDING_IN_PROTECTED_WINDOW`,
  `V3A_PERMANENTLY_UNAVAILABLE`. `INDEX_ATTESTATION_INVALID` / `INDEX_HASH_MISMATCH` are named
  failures. PENDING / PERMANENTLY_UNAVAILABLE are as-of-verification-time classifications: the
  report prints the evaluation time and its source (`--now-ns` | system clock) with the frozen
  statement.
- **Validity:** `PASS | INCOMPLETE | FAIL`, exit `0 | 4 | 1`. Timing axis (`ROUTINE`,
  `DELAY_EXCEEDED`, `PREDATES_COMMITMENT`, `OUTSIDE_COMPLETION_WINDOW`, `NOT_APPLICABLE`) and the
  finalization-window tier are reported, non-gating.
- **Evidence-binding rule (Stef, 2026-09-22):** Index module-hash evidence is bound by
  (`index_canister_id`, certified `/time` within the receipt's window), not by receipt identity.
- **Corpus:** `docs/test-vectors/openchatzd-v5/` mirrored byte-identically at
  `tests/fixtures/v5-openchatzd/corpus/` and driven through the CLI (`tests/openchatzd_v5_corpus.rs`);
  the LUI mirror guard fails when a sibling checkout's mirror drifts.
- **CI pin:** `.github/workflows/backend.yaml` checks out CVDR-Verify at the immutable
  `openchatzd-v5` SHA recorded there; the pin resolves only once the branch is pushed.

## 7. Storage census (MemoryIds, FROZEN)

| Id | Use |
|---|---|
| 0 | upgrades | 3 | stable memory map | 4 | `export_pending` (retained, banked) |
| 5, 6 | retired legacy CVDR store / index (reserved) |
| 7 | durable deletion draft (`CvdrDraft`, keyed by user canister; carries `record_salt` until scrubbed) |
| 8–11 | frozen-package StableLog index/data, primary `receipt_id → offset`, secondary `(record_id, deletion_seq) → receipt_id` |
| 12–14 | Index evidence StableLog index/data, primary `receipt_id → offset` (insert-only) |
| 100–107 | R-3 ceremonial slots: unreclaimed, never reused, documented |

## 8. Reproducible build (canonical recipe)

The **all-canister Docker recipe** is canonical: a published module hash names
`scripts/docker-build-all-wasms.sh` at the release commit (`GIT_COMMIT_ID` is embedded, so the hash
is a function of the committed tree *and* its commit id). No BuildKit secret or token (review F,
2026-10-01): every git source in `Cargo.lock` is public since R-3 removed the private MKTd02 /
zombie-core crates. Commits up to v0.8.0 (`8118d26a`) carry a Dockerfile that hard-fails without the
`gh_token` secret — run their own recipe with any GitHub read token; the `c744de1` baseline still
locks the private crates and needs a token that can read them. The
Step 10 same-window dual build compares all 23 wasms per file; the single-canister recipe
(`canister_name=<x>`) is a determinism check, not a publishable hash. Toolchain: rust 1.95.0,
ic-wasm 0.9.11, `linux/amd64`, `--locked`, path remaps in `generate-wasm.sh`.

## 9. Operator notes

- **Before upgrading `local_user_index`:** check `/metrics` — `cvdr_upgrade_blockers == 0`. A
  refused install (blockers > 0, or pre-V2 drafts) leaves the old wasm serving; the sweep normally
  captures evidence within seconds to minutes, else the block lapses at wall-clock 24 h. Each
  refused `install_code` consumes the IC install allowance and the next attempt is rate-limited for
  several minutes — do not retry in a tight loop.
- `cvdr_index_evidence_count`, `cvdr_index_evidence_discarded{reason}` (epoch changed / cert after
  window / cert before uninstall / already stored) and the `expected_index_module_hash` mismatch
  warning are the health signals; a mismatch is an ops warning, never evidence.
- Canister-HTTP outcalls expire after ~30 s unanswered; a PocketIC test that jumps the clock must
  answer or drain pending outcalls, or the canister never finishes stopping for an upgrade.
- `cargo check` of the LUI cdylib on rustc 1.95.0 hits a compiler ICE in dead-code lint emission
  (pre-existing at c744de1; `cargo build` / `clippy --tests` are unaffected; `RUSTFLAGS=-Adead_code`
  works around it).

## 10. Propagation checklist (`OCZD_SUPPORTED_CLAIM` — same wording everywhere)

1. `backend/canisters/local_user_index/impl/src/model/cvdr_index_attestation.rs` (`OCZD_SUPPORTED_CLAIM`, source of truth).
2. CVDR-Verify `mktd02/mktd02-verify/src/openchatzd/index_attestation.rs` (bidirectional label drift guard).
3. This build spec §4.
4. `OpenChatZD_RTS_draft.md` — OpenChatZD-scoped residual-trust entry (interlock now demonstrated; residual = `h_user_pre` Index-recorded, pre-interlock receipts V3A-unavailable).
5. Shared claims register — OpenChatZD-scoped entry; do not weaken the MKTd02/Leaf claim; nothing assigned to MKTd03.
6. `RELEASES.md` v0.8.0 entry.
