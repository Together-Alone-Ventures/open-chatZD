# OpenChatZD — Stef four-workstream completion evidence

**Date:** 2026-08-04  
**Disposition:** all four workstreams ACCEPT / implemented  
**Repos:** `open-chatZD` branch `antek` (this commit) · `CVDR-Verify` (sibling commit after `v0.7.0`)

## Pinned commits

| Repo | Branch / note | SHA |
|------|---------------|-----|
| open-chatZD | `antek` | _(fill after commit)_ |
| CVDR-Verify | branch from `v0.7.0` | _(fill after commit)_ |

Fill SHAs after `git rev-parse HEAD` on each repo.

---

## Workstream matrix

| ID | Severity | Done |
|----|----------|------|
| **B1** | BLOCKER | Independent `authorize_canister_ranges` in store-gate after `cert.verify`; empty/malformed → reject |
| **B timing** | MAJOR | INDEX give-up anchors only on `receipt_committed_at > 0`; no `certificate_time` fallback |
| **C** | BLOCKER | PortablePackageV2 `frozen` = exact bytes only; `receipt_id` recompute before witness |
| **D** | MAJOR | Timing ⊥ outcome (`predate_timing_keeps_match_outcome`); no live V3-A labels; cross-repo guard green |
| **A** | BLOCKER | Persist readback blocks reveal; delayed ≠ done; no logout on exhaustion; App anonymous recovery; RT-OCZD-5 |

---

## Unit / verifier tests (this machine)

### open-chatZD — `local_user_index_canister_impl`

```
cargo test -p local_user_index_canister_impl --lib -- cvdr_canister_ranges
# 10 passed (wrong-range, empty-range-set, malformed shard, both layouts, …)

cargo test -p local_user_index_canister_impl --lib -- give_up_anchor
# 1 passed — give_up_anchor_is_receipt_committed_at_only

cargo test -p local_user_index_canister_impl --lib -- labels_match_sibling
# 1 passed — labels_match_sibling_cvdr_verify_when_present (sibling CVDR-Verify present)
```

### CVDR-Verify — `mktd02-verify`

```
cd CVDR-Verify/mktd02/mktd02-verify && cargo test --locked
# 82 passed; 0 failed; 0 ignored
# Includes: portable_v2_rejects_object_form_frozen, receipt_id recompute path,
#   predate_timing_keeps_match_outcome, match_and_outside_window_coexist_not_unqualified
```

OpenChatZD-filtered subset previously: **51** `openchatzd::*` passed.

### Frontend shared

```
cd frontend/openchat-shared && npx vitest run src/domain/cvdr.spec.ts
# 9 passed
```

---

## Negative matrices

### Store-gate ranges (B1)

| Case | Result |
|------|--------|
| Legacy in-range | authorize OK |
| Sharded in-range | authorize OK |
| Out of range (legacy/sharded) | reject |
| Empty resolved sharded set | authorization failure (not vacuous pass) |
| Malformed CBOR leaf | reject |
| Later shard malformed | reject |
| Deeper descendant path | reject |
| Wrong subnet | reject |
| Absent from both layouts | reject |

### PortablePackageV2 (C)

| Case | Result |
|------|--------|
| Object-form `frozen` | structural reject |
| Non-exact `version` | reject |
| Null/empty INDEX evidence | malformed (not UNAVAILABLE) |
| Whitespace-equivalent JSON bytes | sha256 reject |
| Hex/base64 Gate A agree | pass |
| `receipt_id` mismatch vs recompute | Reject before witness |

---

## PocketIC / FailedStuck + `#[ignore]` inventory

**FailedStuck path (active, not ignored):**  
`cvdr_tests.rs` — give-up → `FailedStuck` → `/cvdr_live` backstop;  
`failed_stuck_survives_upgrade` (M7).

**Six `#[ignore]` in `cvdr_tests.rs` (legacy / Slice 2–3):**

1. `cvdr_store_fetch_round_trip` — superseded by gate-A HTTP/Candid match  
2. `absent_finalizer_withholds_completion_and_resumes` — Slice 2/3 inverted by §8  
3. `p2_export_path_is_banked_not_half_alive` — Slice 2/3 finalize gating  
4. `draft_survives_upgrade_then_finalizes` — Slice 2/3 finalize tail  
5. `offline_verifier_round_trip_from_bytes` — needs stored package  
6. `captured_executor_hash_survives_mid_flight_upgrade` — needs stored receipt  

(Additional banked ignores exist under `receipts_tests.rs` for P2 LUI→receipts export; not the six CVDR Slice inventory.)

PocketIC full suite not re-run in this evidence window; prior CD_REGATE baseline: `./scripts/run-integration-tests.sh local 4 cvdr_` → 15 passed / 6 ignored.

---

## Dual-build hashes

Script: `scripts/m2-repro-local-user-index.sh` (same-window Docker dual-build identity).  
Not re-executed in this completion window (long Docker). Re-run before network cut if M2 claim is re-asserted:

```
./scripts/m2-repro-local-user-index.sh
# compare dest/*/SHA256 for local_user_index.wasm.gz
```

---

## Cross-repo guard

With sibling `../CVDR-Verify` checked out (post-`v0.7.0` workstream C/D tip):

- `labels_match_sibling_cvdr_verify_when_present` — **PASS**  
- No live OpenChatZD outcome tokens `V3-A` / `V3A` / `V3_A_` (retirement comments + guard tests only; VAPID base64 substrings in build scripts are unrelated)

---

## Frontend recovery walkthrough (manual repro)

**Capability persist + block**

1. Open Delete account → Continue → Prepare.  
2. Simulate storage failure (e.g. DevTools → Application → Local Storage → Block, or quota).  
3. Expect: error `danger.cvdr.persistFailed`, **no** reveal step, **no** delete.

**Close-tab mid-pending**

1. Prepare → save RevealWire → ack → reauth → delete.  
2. While UI shows polling, close the tab (leave `oc_cvdr_receipt_pending` in localStorage).  
3. Reopen app logged out / anonymous.  
4. Expect: `CvdrAnonymousRecovery` polls `/cvdr/<receipt_id>`, downloads when Available, clears pending, shows done.  
5. If still pending after poll budget: UI shows **delayed** (not done); pending key retained.

**Exhaustion ≠ success**

1. Force poll failures (block network to raw host).  
2. After attempts: step `delayed`, **no** `finishDeleteAccountLogout` unless user chooses “Sign out and keep waiting” (explicit handoff; pending kept).

**Abandon residual (RT-OCZD-5)**

1. At reveal, copy warns: destroying saved capability is unrecoverable.  
2. RTS § RT-OCZD-5 states the same residual.

---

## Key paths touched

**open-chatZD**

- `backend/.../model/cvdr_canister_ranges.rs` (new)  
- `backend/.../model/cvdr.rs` (store-gate call)  
- `backend/.../jobs/self_capture_index_evidence.rs`  
- `frontend/openchat-client/src/openchat.ts` (`persist` boolean + `pollCvdrDelivery`)  
- `ConfirmDeleteAccount.svelte` desktop + mobile  
- `CvdrAnonymousRecovery.svelte` + both `App.svelte`  
- `OpenChatZD_RTS_draft.md` (RT-OCZD-5)

**CVDR-Verify**

- `mktd02-verify/src/openchatzd/{package,body,mod}.rs`
