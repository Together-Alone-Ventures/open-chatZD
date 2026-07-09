# OpenChatZD CVDR Build Spec — v1 (ADOPTED baseline)

**Status:** governing build spec for the CVDR finalization rework. Derived from Master
Implementation Plan v4 (ADOPTED) + Finalization Baseline v1. On conflict, Master Plan v4 wins.
**Base:** fork commit `8c8667ac1`. Everything not listed here stays as committed.
**Authoritative once committed into this repo** (invariants-in-source rule).

---

## 1. What changes (delta summary)

| # | Committed today | Build to |
|---|---|---|
| 1 | Single `certified_data` commitment slot + global single-flight guard | Certified **receipt tree**; guard removed |
| 2 | External finalizer calls `finalize_cvdr` (primary) | **Self-finalization** (timer + non-replicated outcall); `finalize_cvdr` demoted to permissionless backstop |
| 3 | Final CVDR = receipt + cert in StableBTreeMap | **Frozen package** (6 fields, §4) in StableLog + BTreeMap indexes |
| 4 | No temporal rule | **Four timestamp fields + window rule** (§5) |
| 5 | `complete_deletion` gated on finalizer callback | **CVDR-only finalization** — see non-negotiable rule (§8) |

## 2. Receipt tree (D1 — G-approved, FROZEN)

- Tree: `RbTree` / ic-cdk certified-map style (as used in spike A1).
- **Tree path (frozen):** `["receipts", receipt_id]` — byte-string labels exactly as written.
- **Leaf value (frozen):** `SHA256(RECEIPT_LEAF_TAG ‖ canonical_receipt_body)` where
  `RECEIPT_LEAF_TAG = b"OPENCHATZD_RECEIPT_LEAF_V1"`.
- **Witness:** IC `HashTree` CBOR encoding, **stored byte-for-byte** in the frozen package.
  CVDR-Verify decodes it as an IC HashTree and recomputes the certified root. Do not
  re-encode, normalize, or regenerate the witness after capture.
- **Receipt body (RECEIPT_BODY_V1 — FROZEN; Ruling A).** Supersedes the erroneous
  "canonical CBOR" wording — the committed code has NO CBOR encoder (its idiom is
  tag-concatenation SHA-256 + candid Storable; Master Plan erratum queued). Definition:
  a fixed-width tag-concatenation extending the committed idiom:

  ```
  receipt_body = RECEIPT_BODY_TAG          (26 bytes = b"OPENCHATZD_RECEIPT_BODY_V1")
               ‖ receipt_id             (32)
               ‖ nonce                  (32)  # receipt_id derivation material, exactly as committed.
                                              # receipt_id = SHA256(RECEIPT_ID_TAG ‖ record_id ‖
                                              #   deletion_seq(u64 BE) ‖ nonce); record_id + deletion_seq
                                              #   already appear below, so `nonce` is the sole missing input.
               ‖ index_canister_id      (len(u8) ‖ raw principal bytes — G len-prefix ruling: two
                                              #   ADJACENT variable-length principals are not self-
                                              #   delimiting, so len-prefix both to close the second-
                                              #   preimage surface on the identity fields.)
               ‖ user_canister_id       (len(u8) ‖ raw principal bytes — same; unifies with
                                              #   TARGETS_COMMITMENT_V1's len(u8) ‖ principal_bytes.)
               ‖ record_id              (32)
               ‖ deletion_seq           (8,  u64 BE)
               ‖ h_user_pre             (32)
               ‖ h_index                (32)
               ‖ commitment             (32 — committed OPENCHATZD_CVDR_V1 formula, reused verbatim)
               ‖ uninstall_completed_at (8,  u64 BE, ns)
               ‖ receipt_committed_at   (8,  u64 BE, ns)
               ‖ targets_count          (4,  u32 BE)
               ‖ targets_commitment     (32 — TARGETS_COMMITMENT_V1, defined below)
  RECEIPT_BODY_TAG = b"OPENCHATZD_RECEIPT_BODY_V1"
  leaf = SHA256(RECEIPT_LEAF_TAG ‖ receipt_body)   // unchanged from §2 above
  # Byte widths PINNED by CC (spec §2 G width/endian rule): every field above is fixed-width
  # except the two principals, which are `len(u8) ‖ raw bytes` (u8 length prefix; IC principals
  # <=29 bytes). G adjacency ruling: the committed single-principal-between-fixed-fields convention
  # does NOT cover two ADJACENT principals — the len(u8)‖bytes fallback governs, unifying with
  # TARGETS_COMMITMENT_V1. Total = 26 + 32 + 32 + (1+|index|) + (1+|user|) + 32 + 8 + 32 + 32 +
  # 32 + 8 + 8 + 4 + 32.
  ```
  **Width/endian rules (G):** all integers fixed-width big-endian; principals use the
  committed serialization convention where one exists (the commitment preimage), else
  `len(u8) ‖ raw bytes` (u8 suffices for IC principals). CC pins the exact final byte
  layout — including the receipt_id derivation material — in this file from the committed
  formulas before freezing, and documents each field's width beside the struct.
  **G principle (frame ruling):** the certified body must bind every field needed to
  make, verify, and interpret the deletion claim — identity (both canister ids,
  receipt_id + its derivation), timing, provenance, interpretation — but NOT the
  certificate/witness/root, which are the certification package OVER the body.

  **TARGETS_COMMITMENT_V1 (FROZEN — Ruling; implements the adopted user-held
  reveal-package decision):**
  ```
  targets              = point-in-time snapshot of cleanup-target Principals
                         (canisters_to_notify), captured pre-uninstall
  ordering             = sort ascending by raw principal bytes (deterministic recompute)
  serialized_targets   = concat over sorted targets of: len(u8) ‖ principal_bytes
  salt                 = 32 bytes from management-canister raw_rand, fetched fresh
                         per deletion during pre-commitments (await allowed there),
                         stored in the durable draft; NEVER reused across deletions
  targets_commitment   = SHA256(b"OPENCHATZD_TARGETS_COMMITMENT_V1" ‖ salt ‖ serialized_targets)
  targets_count        = u32 = number of targets (0 is valid; commitment still computed)
  ```
  The draft (CvdrDraft) therefore gains `salt` + the raw target list alongside §5's
  timestamps — one CvdrDraft change, no churn. The reveal package handed to the user
  (delivery leg, later slice) = { version, salt, sorted target list }; recomputation by
  the user or any third party they show it to must reproduce `targets_commitment`
  exactly. Server retains nothing beyond the draft's lifetime per the adopted privacy
  default. **G-countersigned.**

  Rationale: reuses every committed derivation verbatim (no schema invention), and puts
  the load-bearing fields INSIDE the hash-bound content — G's bounded-time rule requires
  `receipt_committed_at` to be certificate-bound, else the window check is spoofable.
  `delete_requested_at` stays OUT of the body (context only, never load-bearing);
  `certificate_time` obviously arrives later. **G-countersigned (with the field-set amendment above applied).** CVDR-Verify must recompute this exact
  layout; wording everywhere becomes "versioned fixed-width tag-concatenation", not CBOR.
  New stable-memory structures take the next free MemoryIds per §4.

## 3. Atomicity rule (HARD)

Tree mutation + `certified_data_set(root)` + `receipt_committed_at` recording happen in
**one message** — never across an `await`. (Actor-model sequencing makes this race-free;
no CAS, no lock.) Any refactor that introduces an await between read-root and set-root is
a spec violation.

## 4. Frozen package (HARD — exact fields, order, and types FROZEN; immutable once stored)

```rust
pub struct FrozenCvdrPackage {
    pub receipt_body: Vec<u8>,       // RECEIPT_BODY_V1 fixed-width concatenation (see §2)
    pub receipt_hash: [u8; 32],      // SHA256(RECEIPT_LEAF_TAG ‖ receipt_body) — the tree leaf
    pub tree_root: [u8; 32],         // certified receipt-tree root at capture
    pub witness_bytes: Vec<u8>,      // IC HashTree CBOR, verbatim from capture
    pub certificate_bytes: Vec<u8>,  // IC certificate CBOR, verbatim
    pub certificate_time: u64,       // nanoseconds, from certificate /time
}
```
- Field names, order, and types are frozen as written (this order is the serialization
  order). Storable encoding: reuse the committed Storable idiom in `model/cvdr.rs`
  (`candid::encode_one`/`decode_one`, `Bound::Unbounded`) — do not introduce a new
  serialization convention.
- **Memory slots (FROZEN — extends the existing 0–7 map):**
  `8` = package StableLog index memory; `9` = package StableLog data memory;
  `10` = `receipt_id → log_offset` StableBTreeMap (primary);
  `11` = `record_id + deletion_seq → receipt_id` StableBTreeMap (secondary, access-gated).
  Document beside the existing map in `memory.rs`. (Spreadsheet update queued separately.)
- **Upgrade rule:** the certified receipt tree is heap-resident; in `post_upgrade`,
  rebuild it from the receipt store and re-assert `certified_data_set(root)` (certified
  data must be re-set after upgrade). Frozen packages themselves live in stable memory
  and are untouched by upgrades.
- Insert-only. No update/delete path. Serving returns the stored package **verbatim**;
  a fresh read-time certificate may be offered only as an extra.
- Headroom: ~4–8 KB/package; ample v1 headroom; archive canister not required for launch.

## 5. Timestamps (HARD — IC consensus time only, never frontend time)

| Field | When recorded | Load-bearing? |
|---|---|---|
| `delete_requested_at` | optional, frontend-flow context | no — context only |
| `uninstall_completed_at` | on `uninstall_code` success | yes |
| `receipt_committed_at` | same message as tree insert + `certified_data_set` | **window anchor** |
| `certificate_time` | from captured certificate `/time` | yes |

**Verifier window rule:** `certificate_time ≥ receipt_committed_at` AND
`certificate_time − receipt_committed_at ≤ allowed_finalization_window` (24 h provisional,
aligned to retry cap). Tiers: `VerifiedFinal` (in window) / `LateFinalized` (valid, late —
**never silently promoted**) / `Failed-Stuck` (no cert after cap).

## 6. Self-finalization loop (per Finalization Baseline v1 / A1 evidence)

- Trigger: canister timer ~3 s after `receipt_committed_at`; plus a periodic sweep timer
  that re-drives any `AwaitingCertificate` records after upgrade.
- Outcall: management-canister `http_request` with `is_replicated: Some(false)` via a
  **narrow local Candid shim** (own mirror arg type — pinned ic-cdk 0.18.7 resolves
  `ic-management-canister-types 0.3.3`, which lacks the field; do NOT bump the workspace CDK).
- Target: own **raw-domain** query route `GET /cvdr_live/<receipt_id>` returning
  `{receipt_body, witness, data_certificate()}` in the **payload** (no gateway asset-cert reliance;
  route is query `http_request`, never `http_request_update`). Named `/cvdr_live` to stay distinct
  from the frozen-package delivery-leg serving route `/cvdr` (delivery leg, later slice).
- `max_response_bytes`: 16 KB initial; one retry at 64 KB on size/parse miss.
- Backoff: 5 s → 15 s → 45 s → 2 m → cap; max 24 h; then `Failed-Stuck` + metric.
- On response: **full on-chain verification BEFORE store** — reuse the committed
  `finalize_cvdr` verification path verbatim (BLS signature → NNS delegation, delegation
  range covers self → witness reconstructs → certificate `certified_data` == witness root
  == our certified root → witness leaf == receipt_hash → canister id == self → window/tier
  rules). Store package → `CertificateCaptured` ONLY on pass. A response failing
  verification is **discarded and retried — never stored** (an untrusted single node must
  not be able to poison the first-wins slot with a forged-signature package; gap identified
  by OpenChat review). **HARD SECURITY RULE — G-countersigned; not optional hardening.**
- **Claim discipline (unchanged):** offline CVDR-Verify remains the external authority;
  the index never claims `VerifiedFinal` — that is a verifier result.
- Idempotency: first **verified** in-window package wins; later results are no-ops.

## 7. Backstop `finalize_cvdr` (permissionless, idempotent — HARD acceptance rules)

Retain the committed full-BLS verification path here. Accept a submission ONLY if ALL hold:
1. receipt exists in a pending/finalizable state;
2. submitted package matches the stored receipt hash / root;
3. certificate verifies against the NNS root (full BLS + delegation);
4. witness proves `receipt_id → receipt_hash`;
5. certificate time satisfies the same window/tier rules;
6. first valid package wins;
7. no overwrite of an existing finalized package;
8. a late package can only produce `LateFinalized`, never `VerifiedFinal`.

## 8. Non-negotiable pre-build rule (G) — RESOLVED: GATING FOUND, split is ACTIVE work

D Check 1b + CC grounding (from source at `8c8667ac1`): **all** OpenChat-native cleanup
(global/local user-mapping removal + membership-removal notifications) lives in
`complete_deletion` (in `jobs/delete_users.rs`), which is invoked **only** from
`finalize_cvdr` after certificate capture; the DeleteUser job itself does no cleanup
(its `ProcessOutcome::AwaitingFinalizer => {}` arm). Cite symbols, not line numbers —
earlier line refs proved unreliable. This is the old design's correctness hold and
violates the adopted baseline.

**CC must split it:** move the `complete_deletion` cleanup into the delete-job path so it
executes after `uninstall_code` + post-commitments and **independently of certificate
capture** — never from the finalization path. `finalize_cvdr` (backstop) and the
self-finalization continuation become CVDR-only. **Rename** `ProcessOutcome::AwaitingFinalizer`
(there is no finalizer) to align with `DraftStage::AwaitingCertificate`; update the
module-contract doc comments to the new baseline. Exact ordering relative to tree insert
is CC's choice within the rule; the receipt's claim stays "targets captured; notification
attempted or queued" (never "confirmed erased").

(History note: an earlier D check claimed the symbol `complete_deletion` was absent at
this commit — retracted; it was run without access to the actual tree. Verdicts must
state which tree/commit was read.)

## 8b. Slice-1 interim state (Ruling B — scope of "existing tests pass")

Slice 1 necessarily breaks the committed finalize-gated flow (`certified_data` becomes
the tree root, so `finalize_cvdr`'s `certified_data == commitment` binding cannot hold).
Ruling: in Slice 1, `finalize_cvdr` becomes an **inert, compile-safe stub** returning an
explicit error ("pending Slice 2/3 rework") — do NOT half-adapt it to the tree (its full
rework, with the §7 acceptance rules, is Slice 3; the primary store path is Slice 2's
self-finalization). "Existing tests pass" = crate unit/derivation tests (the committed
hash chain carries forward unchanged). The finalize-gated integration tests
(`cvdr_tests.rs`, `delete_user_tests.rs`) are **updated in Slice 1** to the new baseline:
deletion (uninstall + cleanup + notifications) completes with certificate capture entirely
absent — which is precisely the DoD's cert-absent test. This interim state is
dev-branch-only, documented in-code, and never shipped.

## 9. Verifier reject list (CVDR-Verify — MUST reject if any)

1. receipt hash ≠ witness leaf;
2. witness root ≠ bundled `tree_root`;
3. certificate `certified_data` ≠ witness root;
4. canister id / delegation range does not cover `local_user_index`;
5. certificate time outside the allowed window (→ downgrade to `LateFinalized` if
   otherwise valid; reject only on rule 1–4 failures).

## 10. Guardrails (unchanged, restated)

- No `dfx deploy` from CC (gate files). No commit/push/tag without Stef's explicit word;
  explicit-path `git add` only. Move the three stray reports out of ICP-Delete-Leaf before
  any git work there.
- Receipts are never deleted, mutated, or removed from the tree — including in ops
  remediation (ladder: retries → operator-fetched cert via backstop → stays
  `AwaitingCertificate` + escalation).
- CC does not certify its own work; CD verifies from source with file:line evidence.
