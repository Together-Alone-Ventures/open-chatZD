# OpenChatZD CVDR Build Spec — v1 (ADOPTED baseline)

**Status:** governing build spec for the CVDR finalization rework. Derived from Master
Implementation Plan v4 (ADOPTED) + Finalization Baseline v1. On conflict, Master Plan v4 wins.
**Base:** fork commit `8c8667ac1`. Everything not listed here stays as committed.
**Authoritative once committed into this repo** (invariants-in-source rule).

**M1 close-out (2026-07-31 / 2026-08-01, branch `antek`):** Gate 4 rulings from the Completion
Handover Pack v2.0 are folded below (§5 timing, §12 INDEX attestation, §13 scheduler). §14
**PortablePackageV2** placement is **approved with edits** (Stef countersign 2026-08-01). M4 may
proceed subject to §14–§19 as written here.

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

**Verifier timing rules (G4-R3 — two distinct thresholds; IC certified times only):**

1. **Commitment ordering (hard fail):** commitment certificate `/time` must not predate
   `receipt_committed_at`.
2. **Attestation delay (1 hour):** INDEX Module Hash certificate `/time` minus commitment
   certificate `/time` must be ≥ 0. If the delta is ≤ 1 hour → routine. If the delta is >
   1 hour but still within the completion window → `DELAY_EXCEEDED` / late downgrade
   (never a silent pass). Frontend wall clocks are never used.
3. **Completion expiry (24 hours):** all required finalisation evidence (commitment cert +
   INDEX Module Hash cert) must be captured within 24 hours of `receipt_committed_at`.
   Beyond 24 hours the package is expired and not finalisable under that commitment;
   public HTTP/Candid still reports Pending until a valid late/backstop path stores a
   package, and serving never emits verifier verdicts.

Commitment-certificate window for store-gate (self-finalise / backstop):
`certificate_time ≥ receipt_committed_at` AND
`certificate_time − receipt_committed_at ≤ 24h`. Tiers remain verifier-derived:
`VerifiedFinal` (in routine windows) / `LateFinalized` (valid but late — **never silently
promoted**) / failed-stuck (no cert). The index never claims these tiers on public surfaces.

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

## 8. Non-negotiable pre-build rule (G) — **SUPERSEDED** (historical)

> **Normative today:** §8b + `jobs/delete_users.rs`. OpenChat-native cleanup runs in the
> delete job after uninstall + receipt publish, **independently of certificate capture**.
> `finalize_cvdr` / self-finalization are CVDR-only. Do **not** re-gate user deletion on
> certificate capture.

The text below is retained as historical evidence of the pre-split design at `8c8667ac1`
and must not be followed by implementors:

D Check 1b + CC grounding (from source at `8c8667ac1`): **all** OpenChat-native cleanup
(global/local user-mapping removal + membership-removal notifications) lived in
`complete_deletion` (in `jobs/delete_users.rs`), which was invoked **only** from
`finalize_cvdr` after certificate capture; the DeleteUser job itself did no cleanup
(its `ProcessOutcome::AwaitingFinalizer => {}` arm). That old design violated the adopted
baseline and was split per §8b.

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

1. **HTTP (canonical for users and frontend V1):** `GET /cvdr/<receipt_id>` on the raw domain
   (`<index_canister_id>.raw.icp0.io`). Path form is canonical. The query form
   (`/cvdr?receipt_id=...`) is NOT served; any doc using it is corrected in the
   docs pass.
2. **Candid (canonical for canister-to-canister, test, and
   byte-equality surfaces):** `get_cvdr(receipt_id)` query on
   `local_user_index`. The dormant `CvdrReceipt` API shape is replaced: the
   response payload IS the FrozenWire package (P0 interface lock — the public
   API serves FrozenWire verbatim, never a re-projection).

`receipt_id` is a bearer capability: 32 bytes, hex-encoded in the URL
(64 lowercase hex chars). There is no lookup by `record_id`, user, or
principal on any public surface. Support-path lookup remains gated and
out of scope for this contract.

No new frontend candid wiring in V1: the de-registered user path
must not depend on authenticated frontend or candid access.

### 11.2 Response states (both surfaces)

| State | Condition (from durable state) | HTTP | Candid variant | Body |
|---|---|---|---|---|
| **Available** (commitment-only) | Frozen package stored; INDEX code-identity evidence **not** stored | `200` | `Available(FrozenWire)` | Stored FrozenWire, byte-for-byte (11.3 Gate A). Commitment verifiable; INDEX result is `INDEX_ATTESTATION_UNAVAILABLE` (§14.5) |
| **Available** (`PortablePackageV2`) | Frozen package stored **and** INDEX evidence stored | `200` | `Available(PortablePackageV2)` | Canonical `PortablePackageV2` bytes (11.3 Gate B / §14). Nested `frozen` == Gate A FrozenWire bytes |
| **Pending** | Draft exists for this `receipt_id`; no frozen package yet | `202` | `Pending(PendingInfo)` | Minimal JSON: `{"schema":"openchatzd.cvdr.status","version":1,"status":"pending","retry_after_secs":N}` |
| **Unknown** | No draft and no package for this `receipt_id` (or malformed id) | `404` (`400` if malformed) | `NotFound` | Minimal JSON error body; constant-shape, no detail |
| **Scrubbed-unavailable** | Applies to sensitive reveal material only, never to the frozen package — see 11.4 | `404` on any route that would expose reveal data | n/a (no candid surface serves reveal data) | Same constant-shape 404 as Unknown |

**Erratum (Stef 2026-08-01):** Available is **not** blocked on INDEX evidence. FrozenWire-only
Available retains the commitment path. INDEX subnet-attested results require the additional
evidence and are served as `PortablePackageV2` when that evidence is stored. Public surfaces
still never emit verifier V3 verdicts.

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
  `receipt_id` is 256-bit unguessable and issued only by
  `prepare_account_deletion`; only the legitimate holder (or a link
  thief, 11.6) can observe the distinction.
  Carve-out: the /cvdr_live route is a finalization-only surface.
  For a finalizable committed draft, it may serve the draft receipt
  body, including targets_count and targets_commitment, to the bearer
  so the IC certificate can be captured. It never serves salt or raw
  target data. The 'Pending leaks nothing' rule applies to the
  /cvdr/<receipt_id> pending response body, not to /cvdr_live.
  Prepared-but-not-deleted drafts are not finalizable and are not
  served by /cvdr_live.
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

Two gates; both must pass for their respective Available shapes.

**Gate A — FrozenWire (commitment package):**

```
SHA-256(stored FrozenWire, canonical serialization)
  == SHA-256(HTTP 200 FrozenWire body)
  == SHA-256(candid Available(FrozenWire) payload re-serialized)
```

**Gate B — PortablePackageV2 (when INDEX evidence is stored):**

```
SHA-256(stored PortablePackageV2, canonical V2 encoding)
  == SHA-256(HTTP 200 PortablePackageV2 body)
  == SHA-256(candid Available(PortablePackageV2) payload re-serialized)
```

Additionally, nested `frozen` bytes inside PortablePackageV2 **must equal** the Gate A
FrozenWire bytes for the same `receipt_id` (exact nested bytes — never re-encode).

Packages are never re-derived from draft/source state. A fresh read-time certificate is an
OPTIONAL extra delivered outside the package bytes, never merged into them. Gate A remains
the Definition of Done for the commitment delivery leg; Gate B is the Definition of Done for
INDEX evidence delivery (M4/M5).

### 11.4 Scrub semantics vs serving

Scrub-at-capture removes reveal material (salt + target list) from the
draft. It never touches the frozen package. Therefore:

- Available responses are unaffected by scrub — the frozen package
  (including `targets_count` and `targets_commitment`) is served
  forever (subject to storage policy, out of scope here).
- No post-deletion read surface serves reveal material. The reveal
  package is handed in the deletion flow, strictly before the
  irreversible deletion step (hard sequencing invariant, G-ruled):
  `prepare_account_deletion` returns `{ receipt_id, RevealWire }` and
  creates the draft, and the UX must require the user to save or
  acknowledge the reveal package before the irreversible delete step is
  enabled. It is not recoverable after deletion commits and
  scrub-at-capture runs.
- The e2e scrub test asserts: post-capture, no live route (HTTP or
  candid) returns salt or raw target data for the deleted user —
  distinct from and compatible with Pending/Available semantics above.

Prepared-draft lifecycle (G-ruled):

- Prepared-but-not-deleted drafts are TTL-purged; purge scrubs the
  draft material and thereafter the receipt_id serves Unknown.
- Re-issue of the reveal package is permitted before deletion
  commits. Once deletion commits, the reveal is once-only: no
  regeneration, no second handover.
- Prepared-but-not-deleted drafts are not finalizable and are not
  served by /cvdr_live.

### 11.5 Removal of `cvdr_data_certificate`

The obsolete external-finalizer query `cvdr_data_certificate` is
REMOVED (G-ruled): deleted from the `api` and `impl` query modules
(both mod.rs registrations) and the integration-test client. This
canister has no can.did and no .did.ts; no candid regeneration
applies. No route, method, or type of that
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

## 12. Code identity — INDEX Module Hash (G4-R1)

**Ruling:** attestation target is **INDEX only**. The earlier BOTH design (pre-uninstall
target-user `read_state` certificate) and associated mechanism memo are **superseded** and must
not be implemented. Reintroducing BOTH or any target-user certificate is a STOP item requiring a
new explicit G ruling.

| Role | Rule |
|---|---|
| Code-identity target | `local_user_index` only |
| Required evidence | Complete portable subnet **system-state** `read_state` evidence for path `/canister/<local_user_index>/module_hash` (see §14) |
| Comparison target | Extracted certified Module Hash is compared offline to captured `h_index` material already bound in `RECEIPT_BODY_V1` (via draft-captured `executor_module_hash`). The certified value **must never replace** captured `h_index`. |
| Not archival | `canister_status.module_hash` alone is **not** the archival certificate |
| Target user canister | Passive: uninstalled; `h_user_pre` remains an **index-recorded** management-canister observation and must be RTS-qualified as such — no archived target certificate |
| RECEIPT_BODY_V1 | **Unchanged** — no value from the INDEX Module Hash certificate enters the receipt-body preimage |

### Supported claim (OpenChatZD-scoped — do not overclaim)

**Do not** state that the certificate by itself “proves which INDEX code ran when the receipt was
sealed.”

**Supported claim:** OpenChatZD can carry subnet-certified evidence of the Module Hash installed
on the INDEX canister at the INDEX certificate time and compare it with the `h_index` captured in
the receipt. Because OpenChatZD currently has **no demonstrated upgrade-continuity interlock** on
`local_user_index`, matching endpoint hashes do **not** prove uninterrupted execution by that
module throughout the sealing window.

This claim must appear (same wording) in: this build spec, the OpenChatZD RTS, CVDR-Verify
OpenChatZD output/docs, and the shared claims register as an **OpenChatZD-scoped** entry naming
the missing upgrade interlock as the reason. Do **not** weaken the canonical MKTd02/Leaf claim
(whose continuity control has been demonstrated). Do **not** assign either answer to MKTd03 until
its Tree analogue has been tested.

Capture ordering (with §5): INDEX certificate time must not predate the commitment certificate
time; apply the 1h delay and 24h completion rules. Timing is an orthogonal axis to the V3
code-identity outcomes (§17).

## 13. Deletion-job scheduler (G4-R2)

After each deletion-job attempt:

- **Success** (`AwaitingCertificate`): if the queue still has work, schedule **one successor
  immediately** (zero delay). Do not use the 30-second interval on the success path.
- **Retry/failure**: re-queue the user; apply **30 seconds** backoff only when that same user
  is next at the front of the queue. If another user is waiting ahead, drive them immediately.
- One-successor scheduling only — no recursive queue drain and no busy loop.
- Regression: K successful queued deletions must not take approximately `(K−1) × 30s`.

## 14. PortablePackageV2 — INDEX code-identity evidence (**APPROVED with edits — Stef 2026-08-01**)

**Constraint:** preserve existing `FrozenCvdrPackage` / FrozenWire **six-field** bytes and
byte-equality gates (§4, §11.3). Do not mutate `RECEIPT_BODY_V1`. INDEX code-identity evidence
remains **outside** the frozen commitment evidence.

### 14.1 Outer package (normative)

```text
name    = PortablePackageV2
schema  = "openchatzd.cvdr.portable_package"
version = 2

PortablePackageV2 {
  frozen:                          exact canonical FrozenWire bytes (§4 / §11.3)
  index_code_identity_evidence:    complete portable read_state evidence (§14.2)
}
```

- The nested `frozen` value **must** be the exact canonical FrozenWire serialization already
  specified. Never a decoded and independently re-encoded approximation.
- Pin one canonical V2 encoding. **Stored bytes, HTTP Available bytes, and Candid-returned
  package bytes must agree** (byte-equality across all three).
- Keying durable storage by `receipt_id` is acceptable for lookup. `receipt_id` is **not** the
  cryptographic binding: the verifier must independently recompute and validate the receipt ID
  from FrozenWire (and perform the checks in §14.4).

### 14.2 Parallel insert-only store (INDEX evidence)

Stable storage of the frozen commitment package remains exactly §4 (`FrozenCvdrPackage` fields
1–6). No seventh field inside that struct.

New parallel durable evidence (new MemoryId(s), **insert-only**, keyed by `receipt_id` for
lookup):

- Store **everything** an offline verifier needs to validate the system-state response for the
  exact path `/canister/<local_user_index>/module_hash`, including the certificate and the
  certified tree/witness material for that path.
- **Do not** store only an extracted Module Hash and certificate time.
- Certificate `/time` may be derived by the verifier from the stored certificate; it is not a
  substitute for the full evidence blob.
- **MemoryIds (pinned beside `memory.rs`):** 12 = evidence StableLog index, 13 = evidence
  StableLog data, 14 = primary `receipt_id → log offset`. Insert-only; first valid wins.

### 14.3 Store and serving invariants

1. **Store only fully verified evidence** (same hard security rule family as §6: BLS → NNS
   delegation → canister-range coverage → exact path → value proof). A response failing
   verification is discarded and retried — never stored.
2. **First valid evidence wins** — no overwrite, no certificate substitution.
3. **Serving (aligned with §11.2):**
   - Frozen stored, INDEX evidence absent → Available **FrozenWire** (commitment-only).
   - Frozen stored, INDEX evidence present → Available **`PortablePackageV2`**.
   - No frozen yet → Pending.
4. Public surfaces never emit verifier V3 verdicts; offline CVDR-Verify remains the authority.

### 14.4 Verifier obligations (offline)

Independently of storage keying, CVDR-Verify (OpenChatZD path) must:

1. Recompute and validate the receipt ID from FrozenWire.
2. Verify the BLS certificate, NNS delegation, and canister-range coverage.
3. Verify the exact path `/canister/<local_user_index>/module_hash`.
4. Extract the certified Module Hash.
5. Compare it with the immutable `h_index` captured in the receipt (never replace `h_index`).
6. Apply the approved timing rules (§5 / §17) as an orthogonal qualifier.
7. Emit the V3 code-identity outcomes (§17). Mismatch is **not** absence and must not be
   reported as deployer-declared identity.

FrozenWire commitment validity remains **independent** of INDEX code-identity outcomes.

### 14.5 FrozenWire-only / legacy behaviour

A FrozenWire receipt (or a PortablePackageV2 whose INDEX evidence is missing) can retain its
commitment verdict but **cannot** receive the new subnet-attested INDEX result without the
additional evidence. Verifier output must make that absence explicit
(`INDEX_ATTESTATION_UNAVAILABLE`), not imply an INDEX match.

### 14.6 Migration inventory (FrozenWire packages)

Confirm whether any **mainnet OpenChatZD** FrozenWire packages already exist before Available
v2 cutover. This is migration inventory only and does **not** reopen the frozen-format ruling.

**Inventory answer (Antoine, reconfirmed 2026-08-03):** **No.** There are no known mainnet
OpenChatZD FrozenWire packages under this fork’s CVDR delivery leg. OpenChatZD CVDR Available /
INDEX evidence has not shipped to mainnet on this product path. Upstream OpenChat mainnet is out
of scope. Compatibility population only; this does not reopen the frozen-schema ruling.

### 14.7 Leaf / OpenChatZD package-shape divergence (governed)

OpenChatZD uses a **versioned outer package** (`PortablePackageV2`), while canonical Leaf places
module-hash evidence differently. This is a deliberate governed divergence requiring permanent
support for: two verifier input shapes; two conformance-corpus paths; and configuration-specific
documentation. Do not silently unify shapes.

### 14.8 INDEX evidence capture workflow (normative outline for M4)

Commitment finalization (§6 / §7) and INDEX evidence capture are **separate** stores and may
complete in either order after receipt commit, subject to timing rules (§5). INDEX capture:

1. **Trigger:** after `receipt_committed_at` (timer / sweep), once a commitment certificate is
   available or in parallel with commitment finalization — implementor choice, but INDEX
   certificate `/time` must not predate commitment certificate `/time` when both exist.
2. **Outcall:** obtain a complete subnet system-state `read_state` response for
   `/canister/<local_user_index>/module_hash` (not `canister_status.module_hash` alone).
3. **Verify-before-store:** full BLS → NNS delegation → canister-range → exact path → value
   proof (§14.3.1). Failures are discarded and retried — never stored.
4. **Insert-only** under `receipt_id` lookup key; first valid wins.
5. **Serving effect:** transitions Available body from FrozenWire to `PortablePackageV2` for
   that `receipt_id` (§11.2 / §14.3).
6. Timing qualifiers (§17.2) are applied by the offline verifier; on-chain store must not invent
   verdict strings.

**Status:** APPROVED with edits (Stef). Implement INDEX evidence capture, parallel insert-only
storage, `PortablePackageV2` serving, and CVDR-Verify wiring under §14–§19.

## 15. HTTP Pending status (G4-R4)

Prefer HTTP `202` for Pending (§11.2). Before treating gateway behaviour as blocking,
probe raw-domain end-to-end deliverability of `202`. Only if `202` cannot be preserved
end-to-end may a countersigned erratum adopt `200` + the same machine-distinguishable
pending body (no package fields). Until that erratum exists, implementors target `202`.

## 16. Terminology and identity notes (G4-R7 / G4-R8)

- Wording: “versioned fixed-width tag-concatenation” (not CBOR) for `RECEIPT_BODY_V1`.
- Memory slots remain as §4 (ids 8–11 for frozen packages); INDEX code-identity evidence uses
  **new** MemoryIds — document beside `memory.rs` at implementation.
- `record_id_for(user_id)` remains caller-independent; direct and mediated callers must produce
  identical bytes (A4 closed — preserve and regression-test).
- Public surfaces must not claim `VerifiedFinal` / `LateFinalized` / V3 INDEX outcomes; those are
  verifier results.

## 17. V3 code-identity outcomes and timing axis

Under the current renumbering these are **V3** outcomes; **V3-A is retired**.

### 17.1 Code-identity outcomes (evidence validity + hash relationship)

| Outcome | Meaning |
|---|---|
| `INDEX_HASH_MATCH_AT_CERT_TIME` | Valid INDEX evidence and certified value equals captured `h_index` |
| `INDEX_HASH_MISMATCH` | Valid INDEX evidence **positively contradicts** captured `h_index` |
| `INDEX_ATTESTATION_UNAVAILABLE` | Required evidence was not obtained |
| `INDEX_ATTESTATION_INVALID` | Certificate, delegation, path, canister range, or value proof is invalid |

FrozenWire commitment validity remains independent of these outcomes. The certified value must
never replace captured `h_index`. Mismatch is not absence and must not be reported as
deployer-declared identity.

### 17.2 Timing qualifier (orthogonal)

The four outcomes above describe evidence validity and hash relationship. Each result also
carries the applicable timing qualifier:

- **routine** — within the one-hour attestation-delay threshold (§5);
- **`DELAY_EXCEEDED`** — valid evidence after one hour but within the 24-hour completion window;
- **outside / late-path** — under existing expiry / late-finalization rules beyond the permitted
  completion window.

A late matching certificate is therefore **not** silently reported as an unqualified match.

## 18. Upgrade continuity interlock — not assumed

Current `local_user_index` source does **not** block upgrades while INDEX evidence is pending; it
deliberately persists and resumes in-flight drafts across upgrades. Agreement between captured
and later certified hashes also cannot prove continuity because an A → B → A upgrade sequence can
produce matching endpoints.

A continuity interlock is **neither part of this approval nor presumed future work**. On a shared
canister following upstream OpenChat release cadence, such an interlock may prove operationally
unacceptable.

Should one ever be proposed, it requires a **separate architecture ruling** covering:

- enforcement from before `h_index` capture through evidence capture;
- the actual controller / deployment upgrade path;
- durability across restart;
- multiple pending receipts and liveness consequences; and
- adversarial upgrade-window testing.

Until such a mechanism is approved and demonstrated, OpenChatZD remains limited to endpoint
match/mismatch plus timing — **not** proof of uninterrupted code execution.

## 19. Propagation checklist (claims / RTS / verifier)

When implementing M4+, keep these in sync with §12 / §14 / §17 / §18:

1. OpenChatZD RTS — OpenChatZD-scoped residual-trust entry (missing upgrade interlock).
2. Shared claims register — OpenChatZD-scoped entry only; do not weaken MKTd02/Leaf.
3. CVDR-Verify OpenChatZD output strings and documentation — V3 outcomes + timing qualifiers;
   correct claim wording; support both Leaf and `PortablePackageV2` input shapes (§14.7).
4. This build spec remains authoritative for OpenChatZD wire/store rules.
5. Delivery contract §11.2 / §11.3 / Candid API — Available dual-shape (FrozenWire +
   `PortablePackageV2`) and Gate A/B byte-equality tests must land with M4/M5.
