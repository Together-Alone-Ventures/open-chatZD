# OpenChatZD — Stef four-workstream completion evidence

**Date:** 2026-08-04  
**Disposition:** all four workstreams ACCEPT / implemented  
**Repos:** `open-chatZD` branch `antek` · `CVDR-Verify` sibling (from `v0.7.0`)

## Pinned commits

| Repo | Branch / note | SHA |
|------|---------------|-----|
| open-chatZD | `antek` post-review tip | `8df74b35dd89f49314ad562000401a0b7946bd3d` |
| open-chatZD | initial Stef four-workstream impl (B1/timing/A + RTS) | `f59bcadbf36a14c106b6d4ecb434614f6f256315` |
| open-chatZD | evidence package | tip of antek after this docs commit |
| CVDR-Verify | `openchatzd-portable-v2-bytes-receipt-id` tip | `27e620667a94cf739b745be27a902c4846b26b09` |
| CVDR-Verify | PortablePackageV2 bytes-only + receipt_id recompute | `c760a2e3d8b723312bb00a170d13d3064a3e251b` |

Parent pins: open-chatZD parent `dd65774c54f04cd7335cab95d43f289de60c79c9`; CVDR-Verify base tag `v0.7.0` = `e884ac43b905a5a8ce6e82c0f591b728174b5593`.

---

## Workstream matrix

| ID | Severity | Done |
|----|----------|------|
| **B1** | BLOCKER | Independent `authorize_canister_ranges` after `cert.verify`; empty/malformed ≠ vacuous pass; distinct `NotInRange` / `RangesMissing` / `Malformed` |
| **B timing** | MAJOR | INDEX give-up anchors only on `receipt_committed_at > 0`; sticky given-up set stops timer spam |
| **C** | BLOCKER | PortablePackageV2 `frozen` = exact bytes only; `receipt_id` recompute before witness |
| **D** | MAJOR | Timing ⊥ outcome (incl. PREDATES + HASH_MISMATCH); no live V3-A labels; cross-repo guard green |
| **A** | BLOCKER | Persist readback blocks reveal; delayed ≠ done; no logout on exhaustion; anonymous recovery gated on `deletionStarted`; RT-OCZD-5 |

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
# 54+ passed including receipt_id_mismatch_rejects_before_witness,
# predate_timing_keeps_mismatch_outcome, portable_v2_rejects_non_string_frozen
```

### Frontend shared

```
cd frontend/openchat-shared && npx vitest run src/domain/cvdr.spec.ts
# 15 passed
```

---

## Negative matrices

### Store-gate ranges (B1)

| Case | Result |
|------|--------|
| Out of range | `NotInRange` → `CanisterNotInRange` |
| Empty / absent layouts | `RangesMissing` → `CanisterRangesMissing` |
| Malformed CBOR / depth / nested | `Malformed` → `CanisterRangesMalformed` |

### PortablePackageV2 (C)

| Case | Result |
|------|--------|
| Object-form / non-string `frozen` | structural reject |
| `receipt_id` mismatch | Reject before witness/§9 |

---

## PocketIC / FailedStuck + `#[ignore]` inventory

Six ignored in `cvdr_tests.rs` (Slice 2/3 / superseded). Active FailedStuck path covered.

**Re-run 2026-08-04:** `./scripts/run-integration-tests.sh local 4 cvdr_`

```
test result: ok. 15 passed; 0 failed; 6 ignored; finished in 162.23s
```

---

## Dual-build hashes

**Re-run 2026-08-04** at `78b9091a43e844878c8f0ac69389c1947a4dbba6`:

```
build1=7a646d2e20ccacc63e6a962755ee8d34bc0693db6a05b860adaf237323d7b780
build2=7a646d2e20ccacc63e6a962755ee8d34bc0693db6a05b860adaf237323d7b780
PASS: byte-identical local_user_index.wasm.gz
```

Tip `8df74b35` changes canister sources — re-run M2 on tip if claiming identity for the final SHA.

---

## Cross-repo guard

Sibling CVDR-Verify at `27e620667a94cf739b745be27a902c4846b26b09`: labels match; no live V3-A tokens.

---

## Frontend recovery walkthrough (manual repro)

1. Persist failure blocks reveal/delete.  
2. Prepare-only remount → re-auth (not auto-poll).  
3. Post-delete close-tab → anonymous recovery when `deletionStarted`.  
4. Poll exhaustion → delayed, never done; RT-OCZD-5 for abandoned capability.

---

## Key paths

**open-chatZD:** `cvdr_canister_ranges.rs`, `cvdr.rs`, `self_capture_index_evidence.rs`, `cvdr.ts`, ConfirmDeleteAccount (desktop/mobile), `CvdrAnonymousRecovery.svelte`, RTS RT-OCZD-5  

**CVDR-Verify:** `openchatzd/{package,body,mod,fixtures,index_attestation}.rs`
