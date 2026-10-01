# CD final gate — OpenChatZD v0.8.0 release candidate (2026-09-23)

Source: CD final-gate report of 2026-09-23 (facts transcribed; no new claims) plus one re-verification
of CD's own artefact on 2026-10-01, marked as such. Hash provenance rule: `docs/dev/v5/BASELINE_c744de1.md`.

## Refs

| Item | Value |
|---|---|
| WASM source (build AT) | `8118d26aae9c4194b4bc3ded0e58be578648474a` (Step 10) |
| Release-docs source | `2fb08ee85911e6562cecfa8bd2b066619aae9554` (`RELEASES.md` hash table) |
| CVDR-Verify | `c27771b3fbd4c58f4cb607f48b7611926db0a423` (`openchatzd-v5` tip; CI pin `8b0d835` is its ancestor) |
| Recipe | `bash scripts/docker-build-all-wasms.sh` at `8118d26a`, `git_commit_id` = `8118d26aae9c4194b4bc3ded0e58be578648474a` — exit 0 |

## Module hashes — 23/23 equal to `RELEASES.md` at `2fb08ee8`

`local_user_index.wasm.gz` = `6cfdb4ac075c601e8dd09104af7dbadde7f92108477228b9846e9999c03690c7`.

```
b440377919f45ff49400c56c0cf3289729d5b26953e2b6a7e8983997d6f859c7  airdrop_bot.wasm.gz
ec62fdedc0ba697f811b99a7b856070ac6e042de88f5153ba9343c9ce822ea98  community.wasm.gz
eb3ad7c0155cd132e4f650bd9f66157657b54534c83eab8d49e4e373c71667b2  cycles_dispenser.wasm.gz
989145edd32c5b4cb83de4213dc31654aa1959b09d4f161add81935665db292d  escrow.wasm.gz
68cef218792340ebae6cc2bf9d431cb8794132eace21c3f3688613fbbd21e0d3  event_relay.wasm.gz
a549f4d906fcef4dac2faf403a2917cdf0cea9ba62f578c077d5c87f0d274827  group.wasm.gz
f9c9e2ed7dcd4561c8a3ead4f5562d16dd55feec80aa22ddf988e3f2fe03ced8  group_index.wasm.gz
1b9d118d8cbb1789c3786e885f9e8fdfdda7de963f73c0b1b40517d25e4f3b19  identity.wasm.gz
6cfdb4ac075c601e8dd09104af7dbadde7f92108477228b9846e9999c03690c7  local_user_index.wasm.gz
7d008992d012647fa6fdd15ad3304fe6fd5c12eedea94f97df4a06db3339c759  market_maker.wasm.gz
d66c7844d6ccad63edd60d6d46f0bf3c3366cfbd0dac7f67675e8aea7c8e51f2  neuron_controller.wasm.gz
5084add5c795f404a7d22356924e5f914bb1dd356c30689764e649937a71c878  notifications_index.wasm.gz
491d687f9b41c476c69891cab563462db2bbc9a9176b23e83a6ef57bdd555462  online_users.wasm.gz
341cf8b1a48d5e5713eae9f98e0fd8f18030b76e030c661efb6128db302136d6  openchat_installer.wasm.gz
418a4117e905a9e4bae8aaed8d0ed689c9cc03001ff12fde174b74ca85e83f8e  proposal_validation.wasm.gz
699c0db701246c54e70480830494e69f5940d14304f996a9d5b12068c11e9b13  proposals_bot.wasm.gz
0c87ea55cc6a0e37ec860aa4169046241b560678c4a3cbcce4794f1148982e0e  registry.wasm.gz
c2b82a8be4045de96a64ae54d37fe5c379b1014cd970233896fbb7de928c8d95  sign_in_with_email.wasm.gz
21e41dffa908850e502669fd3755542ccca534639d35bce2ac3097e0a658b35d  storage_bucket.wasm.gz
14f3efd6531490ad34b9f5bfb73a3364807ef38f582008a79848ee574f8389c8  storage_index.wasm.gz
74d4d6699c4e8411d5f01fe8a4798adc843ddd3c0fba1c48dc7976928cdadf0f  translations.wasm.gz
36cc902354360b30621fcfe6f473ce4d813d842152aa1131ec609fae24858f44  user.wasm.gz
61395cd69840eca39298090930fe23519d2e80a05507870210384cb27d81f129  user_index.wasm.gz
```

`proposal_validation` and `sign_in_with_email` reference no `GIT_COMMIT_ID`/`git_commit_id`; their
hashes do not depend on the commit id.

## Tests

| Surface | Command | Result |
|---|---|---|
| LUI units | `cargo test --locked -p local_user_index_canister_impl` | exit 0 — 95 passed, 0 failed, 0 ignored |
| Workspace units | `cargo test --locked --workspace --exclude open-chat --exclude tauri-plugin-oc --exclude integration_tests` | exit 0 — 323 passed, 0 failed |
| Clippy | `cargo clippy --locked --workspace --exclude open-chat --exclude tauri-plugin-oc --tests -- -D warnings` | exit 0 |
| Ignored upgrade — R-3 | `r3_upgrade_tests::r3_upgrade_from_c744de1_keeps_state_and_function` (`--ignored`) | exit 0 — 1 passed |
| Ignored upgrade — V1 | `cvdr_v2_upgrade_tests::v1_in_flight_draft_blocks_upgrade_then_terminal_draft_survives_it` (`--ignored`) | exit 0 — 1 passed |
| PocketIC integration, incl. every `cvdr_` suite (`cvdr_tests`, `cvdr_v2_upgrade_tests`, `cvdr_evidence_gate_tests`) | `cargo test --locked -p integration_tests -- --nocapture --test-threads 6` | exit 0 — 359 passed, 0 failed, 6 ignored |
| PocketIC integration, default threads (first attempt) | same, unbounded threads | 32 passed, 327 failed, 6 ignored — resource contention; `scripts/run-integration-tests.sh` prescribes 6 workers |
| CVDR-Verify CI | `bash ./ci.sh` in a disposable copy of `c27771b3` | exit 0 — fmt, clippy, audit, tests, corpus |

## Fresh PortablePackageV3 on the `8118d26a` wasms

| Item | Value |
|---|---|
| Producer | `cvdr_v2_upgrade_tests::stored_index_evidence_unblocks_upgrade_and_draft_finalises` on the `6cfdb4ac…` LUI (exact `GET /cvdr` bytes) |
| Package | `pocketic_e2e_portable_v3.json`, sha256 `7a53f7425902b9a20cf0191feff6ee42494cf52afe9a6dd672fcc7d76fcedc5a`; `trust_root_key_id` = `non-production-test-root` |
| CD, 2026-09-23 | `mktd02-verify --package … --allow-fixture-root-key --fixture-root-key-hex <PocketIC root>` — exit 0, `validity: PASS` |
| Re-verified 2026-10-01 (CD's `c27771b3` verifier build, same package and root) | exit 0 — `validity: PASS`; V3A outcome `V3A_PASS`; certified module_hash `6cfdb4ac075c601e8dd09104af7dbadde7f92108477228b9846e9999c03690c7` = built `local_user_index.wasm.gz`; timing `PREDATES_COMMITMENT` (non-gating); trust root non-production (test verdict only) |

## Result

CD: PASS, no release-blocking finding. All runs in detached/disposable worktrees and `/tmp`.
