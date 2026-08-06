# OpenChatZD — Residual Trust Statement (draft)

**Status:** draft for M1 claim propagation (Stef countersign 2026-08-01; G v0.7.0 timing/S12 2026-08-04).  
**Scope:** OpenChatZD only. Does not modify MKTd02/Leaf or MKTd03 claims.

## RT-OCZD-1 — INDEX Module Hash is endpoint evidence, not continuity

OpenChatZD can carry subnet-certified evidence of the Module Hash installed on the
`local_user_index` canister at the INDEX certificate time and compare it with the `h_index`
captured in the receipt.

**Limitation:** OpenChatZD currently has no demonstrated upgrade-continuity interlock on
`local_user_index`. Matching endpoint hashes do **not** prove uninterrupted execution by that
module throughout the sealing window. An A → B → A upgrade sequence can produce matching
endpoints.

**Not claimed:** that the certificate by itself proves which INDEX code ran when the receipt was
sealed.

**Related:** `CVDR_BUILD_SPEC_V1.md` §12, §14, §17, §18.

## RT-OCZD-2 — Commitment vs code-identity are independent

FrozenWire commitment verification remains independent of INDEX code-identity outcomes.
Absence of INDEX evidence on a FrozenWire-only package yields `INDEX_ATTESTATION_UNAVAILABLE`
(timing `NOT_APPLICABLE`) and must not be reported as a match. Mismatch is not absence.
Incomplete PortablePackageV2 is structurally malformed, not `UNAVAILABLE`.

## RT-OCZD-3 — Package-shape divergence from Leaf

OpenChatZD serves `PortablePackageV2` (outer package + nested exact FrozenWire). Canonical Leaf
places module-hash evidence differently. Verifiers and corpora must retain both shapes.

## RT-OCZD-4 — Certificate-pair delay and age residual (S12)

Attestation **delay** is `t(INDEX certificate) − t(commitment certificate)`. It bounds how far
apart the two certificates are — **not** how long a receipt sat pending before evidence capture.

OpenChatZD additionally anchors a **24-hour completion window** to `receipt_committed_at` for
evidence capture (store-gate / late path). That window is a residual trust boundary: evidence
captured outside it may still verify cryptographically with timing
`OUTSIDE_COMPLETION_WINDOW`, but it is outside the primary finalisation policy.

**Not claimed:** that a small delay proves timely operator behaviour in wall-clock terms, or
that delay measures “pending age” of the receipt.

## RT-OCZD-5 — Abandoned retrieval capability is unrecoverable

After prepare, OpenChatZD persists a local retrieval capability (`receipt_id` + local-user-index
endpoint) so the CVDR can be polled and downloaded without a second authenticated action,
including anonymous restart recovery after logout.

**Limitation:** If the user destroys that capability — clearing site data / local storage, wiping
the device profile, or otherwise discarding the pending session key — OpenChatZD cannot re-mint or
look up the receipt for them. Deletion of the account remains irreversible; the cryptographic
receipt may still exist on-chain/canister storage, but **client recovery from an abandoned
capability is not offered**.

**Stated at risk:** UI copy at reveal acknowledges that abandoning the saved capability is
unrecoverable. Truthful-and-stated; not implied recoverable.
