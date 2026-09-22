# OpenChatZD suite v5 — operator notes (`local_user_index`)

Companion to `CVDR_BUILD_SPEC.md` §9. Everything here is observable in `/metrics` and the canister
log; nothing here changes what is stored.

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
