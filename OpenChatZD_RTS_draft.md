# OpenChatZD — Residual Trust Statement (draft, suite v5)

**Status:** draft, updated for the suite-v5 retrofit (Brief B1 R-1…R-6 + C2; Step 10, 2026-09-22).
Earlier text (M1 claim propagation, Stef countersign 2026-08-01; G timing/S12 2026-08-04) is
superseded where it conflicts.
**Scope:** OpenChatZD only. Does not modify MKTd02/Leaf or MKTd03 claims; nothing here is assigned
to MKTd03.

## RT-OCZD-1 — INDEX code identity is subnet-attested within an interlocked window

**Claim (ratified wording, identical to `OCZD_SUPPORTED_CLAIM`):** OpenChatZD carries
subnet-attested installed module identity during the finalization/certification window. The Index
upgrade interlock bounds that window from uninstall to evidence capture: the same Index code stays
installed until the module-hash certificate is stored, so the certified module hash is the code
identity of the deleting Index.

**What makes it true:** the `/canister/<local_user_index>/module_hash` certificate is fetched after
the uninstall; an upgrade is refused (pre_upgrade and post_upgrade) while any receipt inside its
window lacks evidence; evidence is stored only if its certified `/time` lies in
[`uninstall_completed_at`, +24 h], the code epoch is unchanged and nothing is stored yet (C2
store-gate). The deployer-supplied module hash is no longer evidence and no longer in the receipt
preimage (R-2, R-4).

**Residual:** (a) `h_user_pre` (the user canister's pre-uninstall module hash) remains an
Index-recorded management-canister observation, integrity-bound in `RECEIPT_BODY_V2` but not
independently attested. (b) Receipts whose evidence could not be captured (epoch changed, or the
certified window lapsed) are **permanently V3A-unavailable**: reported as such — never as a pass,
never as a failure — and the commitment (V1/V2) still verifies. (c) Receipts finalised under a
pre-interlock wasm are V3A-unavailable unless evidence was captured before that wasm was replaced.

**Not claimed:** that a certificate by itself proves which INDEX code ran when the receipt was
sealed (the interlock, not the certificate, carries that), or anything about code before
`uninstall_completed_at`.

**Related:** `CVDR_BUILD_SPEC.md` §4, §6; `docs/dev/v5/BASELINE_c744de1.md` §5.

## RT-OCZD-2 — Commitment vs code identity are independent; three V3A outcomes only

FrozenWire commitment verification (V1 recompute + V2 certificate) is independent of the V3A
outcome. V3A is exactly one of `V3A_PASS`, `V3A_PENDING_IN_PROTECTED_WINDOW`,
`V3A_PERMANENTLY_UNAVAILABLE`; `INDEX_ATTESTATION_INVALID` and `INDEX_HASH_MISMATCH` are named
failures, never outcomes. Pending / permanently-unavailable are **as-of-verification-time**
classifications: the verifier prints the evaluation time and its source. Validity is
`PASS | INCOMPLETE | FAIL`; a V3A-unavailable receipt is INCOMPLETE, not FAIL. Mismatch is not
absence; an incomplete PortablePackageV3 is structurally malformed, not unavailable.

## RT-OCZD-3 — Package shape and trust root

OpenChatZD serves `PortablePackageV3` (outer package + nested exact FrozenWire +
`trust_root_key_id`). The trust-root id is a **selector** of a verifier-configured root, never
evidence: an unknown id fails closed, and a non-production root is honoured only under an explicit
verifier flag with root material supplied out of band, announced in the verdict. Canonical Leaf
places module-hash evidence differently; verifiers and corpora retain both shapes.
`PortablePackageV2` is historical (never emitted).

## RT-OCZD-4 — Certificate-pair delay and age residual (S12)

Attestation **delay** is `t(INDEX certificate) − t(commitment certificate)`; it bounds how far
apart the two certificates are, not how long a receipt sat pending. The gating **24-hour window**
is now anchored to `uninstall_completed_at` (hash-bound in `RECEIPT_BODY_V2`) and enforced on
certified time by the Index store-gate and the verifier alike; evidence outside it is never stored.
The timing axis stays non-gating and descriptive: because capture now starts at uninstall, an
INDEX certificate may legitimately **predate** the commitment certificate
(`PREDATES_COMMITMENT`), and `OUTSIDE_COMPLETION_WINDOW` (INDEX certificate more than 24 h after
`receipt_committed_at`) can only appear on evidence stored by a pre-C2 wasm.

**Not claimed:** that a small delay proves timely operator behaviour in wall-clock terms, or that
delay measures "pending age".

## RT-OCZD-5 — Receipt identity is non-identifying; the reveal package is the only link

`record_id = SHA256(tag ‖ record_salt ‖ UserId)` (R-1). Without `record_salt` — delivered only in
RevealWire v2 at deletion — no public field of a receipt links it to a user principal, and the
retired UserId-only derivation is never emitted. **Residual:** the user (or whoever holds the
reveal package) can prove the linkage; OpenChatZD cannot, and cannot re-mint a lost reveal package.

## RT-OCZD-6 — Abandoned retrieval capability is unrecoverable

After prepare, OpenChatZD persists a local retrieval capability (`receipt_id` + local-user-index
endpoint) so the CVDR can be polled and downloaded without a second authenticated action,
including anonymous restart recovery after logout.

**Limitation:** if the user destroys that capability — clearing site data / local storage, wiping
the device profile, or otherwise discarding the pending session key — OpenChatZD cannot re-mint or
look up the receipt for them. Deletion of the account remains irreversible; the receipt may still
exist in canister storage, but **client recovery from an abandoned capability is not offered**.

**Stated at risk:** UI copy at reveal acknowledges that abandoning the saved capability is
unrecoverable. Truthful-and-stated; not implied recoverable.
