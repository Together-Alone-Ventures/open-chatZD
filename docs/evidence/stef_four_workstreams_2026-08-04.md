# OpenChatZD — Stef four-workstream completion evidence

**Date:** 2026-08-04 (initial) · **Delta refresh:** 2026-08-06  
**Disposition:** completion gate close, not closed — delta A/B/C + tag correction in flight  
**Repos:** `open-chatZD` branch `antek` · `CVDR-Verify` sibling pinned by **immutable commit SHA** (not release tag)

## Pinned commits

| Repo | Branch / note | SHA |
|------|---------------|-----|
| open-chatZD | `antek` A/B/C delta code (`fix(cvdr): set deletionStarted…`) | `10f51710d063623d3fa9107e4d5c5e2d43217397` |
| open-chatZD | `antek` HEAD (docs pins after A) | pull `origin/antek` |
| open-chatZD | post-review tip before delta | `8df74b35dd89f49314ad562000401a0b7946bd3d` |
| open-chatZD | initial Stef four-workstream impl (B1/timing/A + RTS) | `f59bcadbf36a14c106b6d4ecb434614f6f256315` |
| CVDR-Verify | `openchatzd-portable-v2-bytes-receipt-id` tip (B empty-range + C retain bytes) | `ac64c1b881b46cda8ef909c4a23ae008f863536d` |
| CVDR-Verify | Stef independent cross-repo guard pass | `27e620667a94cf739b745be27a902c4846b26b09` |
| CVDR-Verify | PortablePackageV2 bytes-only + receipt_id recompute | `c760a2e3d8b723312bb00a170d13d3064a3e251b` |

CI sibling checkout: `backend.yaml` → `ref: ac64c1b…` (immutable). Former annotated tag `v0.7.0` at `e884ac43…` is **deleted** — it predated the bytes-only fix and must not be reused until the final gated mint.

---

## 2026-08-06 delta (Stef feedback)

| ID | Change |
|----|--------|
| **A** | `deletionStarted` written with readback **before** `deleteCurrentUser` (desktop + mobile); rollback via `clearCvdrDeletionStarted` if delete fails. No post-delete write is load-bearing for recovery. |
| **B** | Verifier twin: `empty_resolved_sharded_set_is_authorization_failure` in `v2_certificate.rs`. |
| **C** | `PortablePackage.frozen_exact_bytes` retained after parse + `frozen_exact_sha256()` audit; identity tests. |
| **Tag/CI** | Delete premature `v0.7.0`; CI pins SHA above until real release tag. |

### Export-completeness pointer (producer obligation)

Nested / Available byte identity is covered on the OpenChatZD side by:

| Surface | Test / location |
|---------|-----------------|
| Gate A HTTP body == candid FrozenWire | `backend/integration_tests/src/cvdr_tests.rs` → `cvdr_gate_a_frozen_wire_http_matches_candid` |
| Gate B nested `frozen` == Gate A bytes (store) | `local_user_index/.../cvdr_index_evidence.rs` (nested raw Gate A) |
| Gate B candid/HTTP PortablePackageV2 nested == Gate A | `local_user_index/.../queries/get_cvdr.rs` unit (`assert_eq!(v2.frozen, gate_a)` + canonical JSON) |
| Spec equality chain | `CVDR_BUILD_SPEC_V1.md` §11 Gate A/B SHA equality |

Verifier side retains exact nested bytes post-parse (`frozen_exact_bytes`) so audit can compare against the served artefact without a second input file.

---

## Workstream matrix

| ID | Severity | Done |
|----|----------|------|
| **B1** | BLOCKER | Independent `authorize_canister_ranges` after `cert.verify`; empty/malformed ≠ vacuous pass; distinct `NotInRange` / `RangesMissing` / `Malformed` |
| **B timing** | MAJOR | INDEX give-up anchors only on `receipt_committed_at > 0`; sticky given-up set stops timer spam |
| **C** | BLOCKER | PortablePackageV2 `frozen` = exact bytes only; `receipt_id` recompute before witness; **+ retain `frozen_exact_bytes`** |
| **D** | MAJOR | Timing ⊥ outcome (incl. PREDATES + HASH_MISMATCH); no live V3-A labels; cross-repo guard green (Stef independent @ `27e620667`) |
| **A** | BLOCKER | Persist readback blocks reveal; delayed ≠ done; no logout on exhaustion; **`deletionStarted` before irreversible delete**; anonymous recovery gated on flag; RT-OCZD-5 |

---

## Unit / verifier tests (this machine)

### open-chatZD — `local_user_index_canister_impl`

```
cargo test -p local_user_index_canister_impl --lib -- cvdr_canister_ranges
# 12 passed

cargo test -p local_user_index_canister_impl --lib -- give_up_anchor index_given_up
# 2 passed

cargo test -p local_user_index_canister_impl --lib -- labels_match_sibling
# 1 passed
```

### CVDR-Verify — `mktd02-verify`

```
cd CVDR-Verify/mktd02/mktd02-verify && cargo test --locked openchatzd
# plus: empty_resolved_sharded_set_is_authorization_failure; portable_v2_nested_frozen_*
```

### Frontend shared

```
cd frontend/openchat-shared && npx vitest run src/domain/cvdr.spec.ts
# 16 passed
```

---

## Remaining for release words (not claimed closed here)

1. Re-run **M2 dual-build** at the post-A tip (prior `78b9091` build predates `8df74b3` + this delta).
2. Frontend walkthrough: **unauthenticated recovery through successful CVDR delivery** (not only post-delete close-tab).
3. Stef delta-only re-check on A/B/C + tag/CI pin.
4. Mint annotated `v0.7.0` **once** at the final gated CVDR-Verify commit (`tag == crate version`).

Parent pins: open-chatZD parent `dd65774c54f04cd7335cab95d43f289de60c79c9`; historical incomplete tag tip `e884ac43b905a5a8ce6e82c0f591b728174b5593` (superseded — do not pin CI there).
