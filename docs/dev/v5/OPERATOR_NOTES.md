# OpenChatZD suite v5 — operator notes (`local_user_index`)

Companion to `CVDR_BUILD_SPEC.md` §9. Everything here is observable in `/metrics` and the canister
log; nothing here changes what is stored.

## Pre-deploy checklist — first v5 deploy over a pre-v5 wasm

1. **No in-flight pre-V2 drafts.** Every draft the running wasm wrote must be terminal
   (`CertificateCaptured` / `LateFinalized`) or purged; otherwise the incoming `post_upgrade` traps
   with `pre_v2_upgrade_refusal` (`jobs/delete_users.rs:340-342`, predicate
   `CvdrDraft::is_legacy_pre_v2`). On the pre-v5 wasm: `cvdr_awaiting_certificate_count == 0` and
   `cvdr_failed_stuck_count == 0`; it exports no per-stage count for `Prepared` / `Captured` /
   `Uninstalled` (`cvdr_drafts_in_flight` includes terminal drafts), so the trap is the
   authoritative check.
2. **No pending evidence — `cvdr_upgrade_blockers == 0`.** Exported from the v5 wasm on; a pre-v5
   wasm does not export it. Against a pre-v5 wasm the incoming `post_upgrade` evaluates it with code
   epoch 0, so every receipt past its uninstall, inside its 24 h window and without an evidence row
   blocks (`jobs/delete_users.rs:343-353`). A certificate-only row written by the pre-v5 wasm counts
   as evidence here, but is never served as `PortablePackageV3` (BASELINE §6).
3. **Init / upgrade `Args` changed on `v5-retrofit`** (base `c744de1`):

   | Canister · Args | Change | Sent by | Note |
   |---|---|---|---|
   | `local_user_index` `init::Args` | `executor_module_hash: Hash` → `expected_index_module_hash: Option<Hash>` (R-2: ops guard, never evidence) | `user_index` `add_local_user_index_canister` (`Some(wasm_hash)`) | new installs |
   | `local_user_index` `init::Args` | `receipts_canister_id: Option<CanisterId>` removed (R-3) | `user_index` | new installs |
   | `local_user_index` `post_upgrade::Args` | `executor_module_hash: Hash` → `expected_index_module_hash: Option<Hash>` | `user_index` `jobs/upgrade_canisters.rs` (`Some(new wasm hash)`) | a not-yet-upgraded `user_index` still sends `executor_module_hash`; the new Index decodes `None` (guard off) |
   | `user_index` `init::Args` | `receipts_canister_id: Option<CanisterId>` removed (R-3) | `openchat_installer` | new installs; the stored `Data` field is dropped on upgrade |
   | `user_index` `post_upgrade::Args` | unchanged | — | — |
   | `openchat_installer` `init::Args` | `receipts_canister_id` removed (R-3) | controller | new installs |
   | `user` `init::Args`, `post_upgrade::Args` | `mktd_module_hash: Option<[u8; 32]>` removed (R-3) | `local_user_index` | — |

4. **Order** (as exercised by `r3_upgrade_tests::r3_upgrade_from_c744de1_keeps_state_and_function`):
   `openchat_installer` → `user_index` (via the installer) → `local_user_index` (via `user_index`) →
   user canisters (via `local_user_index`).

## Upgrading `local_user_index`

1. **Check `cvdr_upgrade_blockers` in `/metrics` and wait for `0`.** A blocker is a deletion past
   `uninstall_completed_at` that has no Index module-hash evidence yet and is still inside its
   24 h window. The self-capture sweep normally stores evidence within seconds to minutes of the
   uninstall; the block lapses on its own at wall-clock 24 h.
2. **A refused install is safe** — `pre_upgrade` (or `post_upgrade` when upgrading from a
   pre-interlock wasm) traps with the refusal text (blocker count, `receipt_id` prefixes with
   stages, and this operator path); `install_code` fails and the *old wasm keeps serving*.
   `user_index` restarts it if needed.
3. **Install allowance / rate limit.** Every refused `install_code` still consumes the IC's
   install allowance for the canister; after a refusal the next attempt is rate-limited for
   several minutes (`Canister … is rate limited because it executed too many instructions in the
   previous install_code messages`). Do not retry in a loop — wait for `cvdr_upgrade_blockers == 0`.
4. **Pre-V2 drafts** (created by a wasm before R-1, no `record_salt`) also refuse the upgrade
   (`pre_v2_upgrade_refusal`): let them finalise or purge under the current wasm first. None
   existed on mainnet at c744de1.

## Evidence capture health

| Signal | Meaning |
|---|---|
| `cvdr_index_evidence_count` | receipts with stored, verified module-hash evidence (insert-only, first wins) |
| `cvdr_upgrade_blockers` | receipts still needing evidence inside their window (gates upgrades) |
| log `cvdr_index_evidence_discarded{reason}` | a verified certificate was **not** stored: `index_evidence_epoch_changed` (an upgrade happened between outcall and reply), `index_cert_time_after_window`, `index_cert_time_before_uninstall`, `index_evidence_already_stored` |
| `expected_index_module_hash` mismatch warning | the deployer's expectation differs from the certified hash — an ops warning only, never evidence, never in a preimage |

- `cvdr_index_evidence_count` also counts certificate-only rows written before R-2. Such a receipt
  does not block upgrades and is never re-captured (the store is insert-only), but it is served as
  `FrozenWire`, never `PortablePackageV3` (BASELINE §6).

- Canister-HTTP outcalls to `read_state` **expire after ~30 s** if unanswered by the subnet; the
  sweep simply re-issues them. In PocketIC, a test that jumps the clock must answer or drain
  pending outcalls (503) or the canister never finishes stopping for an upgrade.
- A receipt whose window lapsed without evidence is **permanently V3A-unavailable**: served as
  `FrozenWire`, verified as `INCOMPLETE / V3A_PERMANENTLY_UNAVAILABLE` — never a failure. Nothing
  an operator does later can add evidence for it.

## Serving

- `GET /cvdr/<receipt_id>` (raw domain) and `get_cvdr` return byte-identical packages
  (`PortablePackageV3` once evidence is stored, `FrozenWire` before / never). Pending is `202`.
- `trust_root_key_id` in served packages is stamped from the Index's configured root key
  (`"mainnet"` on mainnet). A verifier that sees `non-production-test-root` on a production
  package should treat the deployment as misconfigured.

## Test runner

The integration suite runs at the repo-prescribed six workers
(`./scripts/run-integration-tests.sh local 6`); the Step 9 flake isolation (dedicated PocketIC envs) was
validated at that concurrency.

## Toolchain note

`cargo check -p local_user_index_canister_impl` on rustc 1.95.0 hits a compiler ICE while
emitting dead-code warnings for the cdylib (pre-existing at c744de1). `cargo build`,
`cargo clippy --tests` and the Docker build are unaffected; `RUSTFLAGS=-Adead_code cargo check`
works around it.
