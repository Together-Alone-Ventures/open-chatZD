# CD Re-gate Request — OpenChatZD M4 → M8

**From:** Antoine (CC)  
**To:** CD (independent re-gate)  
**Date:** 2026-08-01  
**Branch:** `antek` (tip advances; see PR)  
**Base:** `master` @ `7fb5569bd`  
**CD interim reply (2026-08-03):** packaging approved; verifier pin gap is TAV-owned; CC follow-ups below.

> **Not a certification.** CC does not certify its own work (spec §10).  
> CD re-runs load-bearing commands from source and records PASS / FAIL / HOLD with file:line.

---

## Ask

Please independently re-gate milestones **M4, M5, M6, M7, and M8** on tip `c8da2da89`.

| M | What | Tip / range |
|---|------|-------------|
| M4 | INDEX capture + dual Available + Verify V3 | through `1dce89a6b` |
| M5 | Delivery matrix polish / scrub / §11.5–11.7 | `1dce89a6b` |
| M6 | Prepare/RevealWire + frontend wizard | `4590cc17f` |
| M7 | FailedStuck tree resume + stuck metrics | `c8da2da89` |
| M8 | Full verification matrix (Antoine ran; CD must re-run) | tip above |

Local evidence (CC workspace, not this git tree):  
`documents/openChat/OpenChatZD_CD_Regate_Packet_M4_M7.md`,  
`OpenChatZD_M4`…`M8_Evidence.md`, `OpenChatZD_M7_Runbooks.md`.

---

## Non-negotiables to spot-check

1. INDEX attestation only (no BOTH / target-user Gate 4).  
2. `RECEIPT_BODY_V1` frozen.  
3. Store-gate before any frozen / INDEX store.  
4. No public V3 verdicts on `/cvdr`.  
5. No irreversible UI delete before RevealWire ack.  
6. Frozen packages never deleted in ops.  
7. Logs: `receipt_id` prefix ≤8 hex.

---

## Commands (CD re-run)

```bash
cd open-chatZD
git fetch && git checkout antek && git rev-parse HEAD   # c8da2da89…
unset CARGO_TARGET_DIR

cargo test -p local_user_index_canister_impl --lib cvdr
# expect ~40 passed

./scripts/run-integration-tests.sh local 4 cvdr_
# Antoine: 15 passed, 6 ignored (legacy #[ignore])

# Optional focused:
./scripts/run-integration-tests.sh local 6 prepare_
./scripts/run-integration-tests.sh local 2 failed_stuck_survives_upgrade

# Verifier pin
git -C ../CVDR-Verify rev-list -n 1 v0.6.1
# expect ad16f2ae7c572c0007784ee45dd801dba35191dc
cd ../CVDR-Verify/mktd02/mktd02-verify && cargo test
# Pin v0.6.1: expect 58 passed / 0 failed / 0 ignored
# (Earlier pack text said ~74; that does not reproduce on this pin. Use 58.)
```

If `wasms/` stale: `unset CARGO_TARGET_DIR && ./scripts/generate-all-canister-wasms.sh`  
(or `./scripts/generate-wasm.sh local_user_index`).

---

## CC follow-ups to CD interim (2026-08-03)

1. **M2 claim restated:** same-window Docker dual-build identity only; not network-hermetic
   (mutable `ubuntu:24.04`, apt, rustup, `cargo install`, git deps). See local evidence
   `OpenChatZD_M2_Evidence.md`.
2. **Verify count:** pin `v0.6.1` = **58** passed (not 74).
3. **Migration inventory:** **No** known mainnet OpenChatZD FrozenWire packages (§14.6).
4. **`labels_match_sibling_cvdr_verify_when_present`:** no longer silent-pass when
   `CVDR-Verify` is present but `openchatzd/index_attestation.rs` is missing (pin gap fails loud).
5. **Naming:** OpenChatZD = ICP Tree corridor. Leaf demo / Zombie Sandbox / DaffyDefs is a
   separate track from this re-gate.

---

## High-signal paths

| Concern | Path |
|---------|------|
| Prepare | `backend/canisters/local_user_index/impl/src/updates/prepare_account_deletion.rs` |
| Delete gate | `…/jobs/delete_users.rs` |
| FailedStuck resume | same — `rebuild_receipt_tree_from_durable` |
| Self-finalize | `…/jobs/self_finalize_cvdr.rs` |
| Backstop | `…/updates/finalize_cvdr.rs` |
| INDEX capture | `…/jobs/self_capture_index_evidence.rs` |
| Dual Available | `…/queries/get_cvdr.rs` |
| Stuck metrics | `…/impl/src/lib.rs` |
| Frontend CVDR | `frontend/openchat-shared/src/domain/cvdr.ts` |
| Wizard | `frontend/app/src/components/home/profile/ConfirmDeleteAccount.svelte` |

---

## Known non-fails

- PocketIC cannot mint NNS-rooted INDEX `module_hash` (Gate B = unit + mainnet fixture).  
- Browser E2E not automated.  
- Six PocketIC tests remain `#[ignore]` (Slice 2/3 legacy / superseded).  
- “Receipt link after account gone” = Decision Log / product, not closed by this matrix.

---

## CD response template

```
SHA verified: ________
M4: PASS / FAIL / HOLD — notes:
M5: PASS / FAIL / HOLD — notes:
M6: PASS / FAIL / HOLD — notes:
M7: PASS / FAIL / HOLD — notes:
M8: PASS / FAIL / HOLD — notes:
Blockers (file:line):
```

Please reply on this PR (or TAV CD channel) with the template filled.
