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

## 8b. Slice-1→3 resolution (Ruling B — scope of "existing tests pass")

Slice 1 changed `certified_data` to the receipt-tree root, so the committed finalize-gated
flow (`finalize_cvdr`'s `certified_data == commitment` binding) no longer applies. That
rework is complete: the primary store path is the implemented self-finalization of §6, and
`finalize_cvdr` is the implemented permissionless backstop of §7.

Ruling B (scope of "existing tests pass"), as applied: the crate unit/derivation tests carry
the committed hash chain forward unchanged; and the former finalize-gated integration tests
(`cvdr_tests.rs`, `delete_user_tests.rs`) were rebased to the new baseline, where deletion
(uninstall + cleanup + notifications) completes with certificate capture entirely absent —
the DoD's cert-absent test.

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

## 11. CVDR Delivery Endpoint Contract (V1) — G-countersigned


### 11.1 Canonical surfaces

Two read surfaces, identical state semantics:

1. **HTTP (canonical for users):** `GET /cvdr/<receipt_id>` on the raw domain
   (`<index_canister_id>.raw.icp0.io`). Path form is canonical. The query form
   (`/cvdr?receipt_id=...`) is NOT served; any doc using it is corrected in the
   docs pass.
2. **Candid (canonical for frontend):** `get_cvdr(receipt_id)` query on
   `local_user_index`. The dormant `CvdrReceipt` API shape is replaced: the
   response payload IS the FrozenWire package (P0 interface lock — the public
   API serves FrozenWire verbatim, never a re-projection).

`receipt_id` is a bearer capability: 32 bytes, hex-encoded in the URL
(64 lowercase hex chars). There is no lookup by `record_id`, user, or
principal on any public surface. Support-path lookup remains gated and
out of scope for this contract.

### 11.2 Response states (both surfaces)

| State | Condition (from durable state) | HTTP | Candid variant | Body |
|---|---|---|---|---|
| **Available** | Frozen package stored for this `receipt_id` | `200` | `Available(FrozenWire)` | The stored FrozenWire package, byte-for-byte (11.3) |
| **Pending** | Draft exists for this `receipt_id`; no frozen package yet | `202` | `Pending(PendingInfo)` | Minimal JSON: `{"schema":"openchatzd.cvdr.status","version":1,"status":"pending","retry_after_secs":N}` |
| **Unknown** | No draft and no package for this `receipt_id` (or malformed id) | `404` (`400` if malformed) | `NotFound` | Minimal JSON error body; constant-shape, no detail |
| **Scrubbed-unavailable** | Applies to sensitive reveal material only, never to the frozen package — see 11.4 | `404` on any route that would expose reveal data | n/a (no candid surface serves reveal data) | Same constant-shape 404 as Unknown |

Notes:

- **Pending covers stuck.** A draft past the 24 h window that has not
  finalized still reports Pending (the permissionless backstop can still
  rescue it into LateFinalized). Age/monitoring of stuck drafts is a
  runbook concern (`AwaitingCertificate` monitoring), not a distinct
  public state. `retry_after_secs` is a client politeness hint
  (suggested: 5), not a promise.
- **Pending body leaks nothing.** It contains no draft contents, no
  timestamps derived from the draft, no target data — status and retry
  hint only. Distinguishing Pending from Unknown is safe because
  `receipt_id` is 256-bit unguessable and issued only in the deletion
  response; only the legitimate holder (or a link thief, 11.6) can
  observe the distinction.
- **Facts, not verdicts.** No surface reports VerifiedFinal /
  LateFinalized / any tier. Tiers are verifier-derived (CVDR-Verify)
  from `certificate_time` vs the window. The serving layer never
  classifies.
- **Pending is served as HTTP `202` (fixed).** CC probes IC HTTP
  gateway behaviour before implementation; only if `202` is proven
  undeliverable end-to-end is the fallback (`200` with the
  `"status":"pending"` body and NO FrozenWire fields) adopted — by
  spec erratum to this section, before build. In either case the
  invariant is machine-distinguishability of Pending from both
  Available and Unknown by body schema.

### 11.3 Byte-for-byte serving invariant (acceptance test)

The Available body is the stored frozen package canonically serialized
to the portable schema (FrozenWire, `package.rs`), served verbatim:

```
SHA-256(stored package, canonical FrozenWire serialization)
  == SHA-256(HTTP 200 response body bytes)
  == SHA-256(candid Available payload re-serialized)
```

stable across repeated fetches. The package is never re-derived
from draft/source state, re-projected into an old API shape, or
enriched inside the package bytes; canonical serialization of the
stored FrozenWire object is allowed and required. A fresh read-time certificate is
an OPTIONAL extra delivered outside the package bytes (e.g. a separate
header/field), never merged into them. This equality is the Definition
of Done gate for the delivery leg (per Delivery-Leg Preconditions v1).

### 11.4 Scrub semantics vs serving

Scrub-at-capture removes reveal material (salt + target list) from the
draft. It never touches the frozen package. Therefore:

- Available responses are unaffected by scrub — the frozen package
  (including `targets_count` and `targets_commitment`) is served
  forever (subject to storage policy, out of scope here).
- No post-deletion read surface serves reveal material. The reveal
  package is handed exactly once, in the deletion response, strictly
  BEFORE scrub (hard sequencing invariant, G-ruled). After scrub it is
  unrecoverable by design.
- The e2e scrub test asserts: post-capture, no live route (HTTP or
  candid) returns salt or raw target data for the deleted user —
  distinct from and compatible with Pending/Available semantics above.

### 11.5 Removal of `cvdr_data_certificate`

The obsolete external-finalizer query `cvdr_data_certificate` is
REMOVED (G-ruled): deleted from `api` and `impl` query modules, candid
regenerated, and `.did.ts` updated by hand (commit message must state
"manual edit — not regenerated"). No route, method, or type of that
name survives. Rationale: implementation is permanently
`NotAvailable`; the name describes a flow that no longer exists;
pending-state signalling belongs to the canonical route (11.2).

No `cvdr_status` endpoint is added in V1. If frontend/typebox work
demonstrates a concrete need for a status-only query, a `cvdr_status`
contract is drafted for G countersign before build (G's stated
fallback); the default path is that the canonical surfaces suffice.

### 11.6 Transport and logging rules (ops-binding)

- HTTPS only (raw domain is HTTPS; no plaintext alternative is
  documented or linked).
- The full `receipt_id` MUST NOT be written to logs, metrics, traces,
  or error messages on any server-side path. Where correlation is
  needed, log at most a truncated prefix (first 8 hex chars).
- Responses carry `Cache-Control: no-store` — bearer-URL content must
  not be cached by intermediaries.
- User-facing copy treats the link as a secret: "Treat this link like
  a password."

### 11.7 Test obligations bound to this contract

1. Byte-equality gate (11.3) — HTTP and candid.
2. Pending → Available transition observed across finalization on the
   same `receipt_id` (no 404 in the gap).
3. Unknown returns `404`, malformed id returns `400` — necessarily
   distinguishable by status code; both bodies are constant-shape and
   echo no detail (no id reflection, no reason strings).
4. E2e scrub assertion (11.4).
5. Reveal-package round-trip against CVDR-Verify `--reveal` mode.
6. Un-ignore candidates from CD's 11-test inventory mapped to the
   above (exact list proposed by C separately; the 5 banked P2
   `receipts_tests` stay banked).
