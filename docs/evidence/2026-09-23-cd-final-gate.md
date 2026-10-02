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

## Re-run 2026-10-01 — ignored baseline upgrade tests (review follow-up B)

| Item | Value |
|---|---|
| Source | `b4088c135` (`v5-retrofit`; integration-test code as at `8118d26a`) |
| Current wasms | `8118d26a` tree, all-canister recipe, `git_commit_id` = `f314663c5945b699cee50250514a5a1e4dce935d-step10-worktree` — the Step 10 set, LUI `1ea8f3cd…` (no LUI/user_index/user code change since) |
| Baseline wasms | `wasms/baseline_c744de1/` from CD's 2026-09-22 copy; SHA-256 = BASELINE §3.2 (checked on load by `wasms::baseline_c744de1`) |
| PocketIC | server 11.0.0 |
| Command | `cargo test --locked -p integration_tests -- --ignored --test-threads 1 --nocapture r3_upgrade v1_in_flight_draft_blocks_upgrade` |
| Result | exit 0 — 2 passed, 0 failed (`r3_upgrade_from_c744de1_keeps_state_and_function`, `v1_in_flight_draft_blocks_upgrade_then_terminal_draft_survives_it`), 219.85 s |

## Superseded 2026-10-02 — build AT `0740610b8fda284624566d492ce62ec9821e454f` (2026-10-01)

Published in `RELEASES.md` from `dbbcb15a0` until the G-decision commits; same-window dual build,
23/23 identical.

```
f8207677404e448dec8ad80a160431c7c55cde74196fc0285ce055e474684d0a  airdrop_bot.wasm.gz
c118e5fd104461cf3bfb06b6f12174350c51351b1b46e3ca50097301b2624b09  community.wasm.gz
f7b301b8d4263436dd168ebd52666526950e6a8580ff24a1d5ab399267e727fc  cycles_dispenser.wasm.gz
c4c73bb5ddc31cfe509fb3686e6bbfa7488880945b6a8bd35535b0031b6b69a0  escrow.wasm.gz
ec1b661388d3ac720141120fe0f3cd127a92a8496d54ed0214648af07e541572  event_relay.wasm.gz
d0c3190684c4ed2068cf9be07717aa5f2b2c42eb774ec32758f7f51d15102fd0  group.wasm.gz
f8d833d2310b08817577bbac8bf6b9cd7cc69e87267e5778a2f6b2ac5976e40a  group_index.wasm.gz
695be1cccc26664b31232b6bb7e19b215b5bd1f1c5d61dff1ae31b7e0ec364b3  identity.wasm.gz
2d2210d8028ac06cb85bba71aa094081a741863a8708ce691c77cbfd02bfc416  local_user_index.wasm.gz
8b820c99b25ff57619332f60880f5acc8bc9b2cbb0f23f7d4aeaba3cc3cfc0eb  market_maker.wasm.gz
b2c8fe1b169deef17a7682c9874b706dfeab6a59bb2b60dae71f70bc143672eb  neuron_controller.wasm.gz
d19d17f5b17c00a61ca36cd46cb4a15718fc80bd5ae37ad932e5f38d9292a61b  notifications_index.wasm.gz
363967cc6b1e0b6ee22100a650382860a96e34e519357d4b5cd358a68c53e661  online_users.wasm.gz
1bce70d92f21593685adf4540a5288f2855f49c67d879ad7cac78ea4bb580dc4  openchat_installer.wasm.gz
418a4117e905a9e4bae8aaed8d0ed689c9cc03001ff12fde174b74ca85e83f8e  proposal_validation.wasm.gz
5ab1ba9a8286a07cd91e1ccd105443e0d451136237ac69115da317db1b7bb163  proposals_bot.wasm.gz
18fe15d281c0a220b0ce957020fe4ee82928ed84a2bc3e16768b541f73610d27  registry.wasm.gz
c2b82a8be4045de96a64ae54d37fe5c379b1014cd970233896fbb7de928c8d95  sign_in_with_email.wasm.gz
678825ed75fd00594be3a57d14e4199554141173e9df8c653fbd07a71498c3b1  storage_bucket.wasm.gz
8a7e2845e603fd8fa669ae4415781aaa0ec1a6fa9cb7514d42516467910a18c2  storage_index.wasm.gz
63ae0bcff754b00055154b5760f99ed4fb0988ea106ac02a2bf76ba2b3bb8c97  translations.wasm.gz
e15bf18a218a16e5fef408770222454b433c7ce6bb9fb6d27367a6a98184fcff  user.wasm.gz
4b8280a4d21dba0a4125e36c1e7010110f5ccd3d0cef385f764267017e5622a7  user_index.wasm.gz
```

## Superseded 2026-10-02 — build AT `84b2ba866a60ca929905eed364b207544da92385`

Published in `RELEASES.md` from `d7fd34c70` until the empty-certificate fail-closed commit (R below).

| Item | Value |
|---|---|
| open-chatZD | `84b2ba866a60ca929905eed364b207544da92385` (release candidate until R) |
| CVDR-Verify | `b2547880c1dfda6304a097e97fb74fb640002f9f` (`openchatzd-v5`; `700a4bd1` label rename, `b2547880` historical V2 → `NOT_ATTESTED`) — tag `openchatzd-v0.8.0`, applied by Stef at freeze; = the `backend.yaml` pin |
| Recipe | `scripts/docker-build-all-wasms.sh` in a clean worktree AT the release commit (no token), then the same recipe with a busted `build_nonce` — 23/23 identical |
| `local_user_index.wasm.gz` | `00b09636e1ac27cd4fc281cfb0e862ad8fc507733bca0e72ea3ed2e9d81b40e4` |

```
139f553429b474dd762ea13eed8fdc79c8d819c82aaa7ee670d22e4fa7393e49  airdrop_bot.wasm.gz
bd604332ac8374806a8e6be3e265515ab4df029f2d97b9dc8e5ab0a6ac9ba8ed  community.wasm.gz
51454e4f87fe6c3733b31566fb2bb4236c6f31b7248f4e22229514569de01715  cycles_dispenser.wasm.gz
dc2aa416c18640328b75ff5e52807981fe27a38ad5251a861dd7aea2dafe55ac  escrow.wasm.gz
92800ceb17c9fa7c7d4898ed1ba99217372db56f3ab270373d16ef14b40c8f2b  event_relay.wasm.gz
a9d53f391f980e123e8034640e61b5cc0d24bd7f7bf9e04adf52ded2c8f2bdf1  group.wasm.gz
de3ec201ad3b57ae7f6af0c3600d10dff32d080fd984ce08943a70d8c601fac3  group_index.wasm.gz
acfe5ae355c6fad351141a56a0dabe406a5a193ee0fffdfe0b509eac2d92710b  identity.wasm.gz
00b09636e1ac27cd4fc281cfb0e862ad8fc507733bca0e72ea3ed2e9d81b40e4  local_user_index.wasm.gz
7f6f022f002bd44fc84143ff1829ac68e0ef763108e55dcf16eaf40cbe75d974  market_maker.wasm.gz
6666e358eab7b1ce8f74e9f86a1ca327b9cb9f8d622971f360af21e48f1e0c2f  neuron_controller.wasm.gz
cf8c4c9f261671b98c655f4733cabffeade378fd8f3cd886738f3b7f303f9cf0  notifications_index.wasm.gz
f45b87678275fd0499ea50bbbbe6f5165e027a111393999e111cbd1dd64d2b61  online_users.wasm.gz
e9764f32be44bbe604fb4e70276e39d2290e49aeff2f4e02bcf25a7756885205  openchat_installer.wasm.gz
418a4117e905a9e4bae8aaed8d0ed689c9cc03001ff12fde174b74ca85e83f8e  proposal_validation.wasm.gz
08a8a48bbfe3181af330f4313962ce1bbbf1e64a89f2611ff72163ddf281502d  proposals_bot.wasm.gz
32a0b92ce522dd0331e0aba5e7909ea972940bf733b55eeda462fb08afe2305b  registry.wasm.gz
c2b82a8be4045de96a64ae54d37fe5c379b1014cd970233896fbb7de928c8d95  sign_in_with_email.wasm.gz
7ccd61a64079fd63d9cf830514c5cee802252f1acabc16d5d8fd244d63a3caee  storage_bucket.wasm.gz
c93fd8e6a5fc9db2414410142c6175ae595d093e442b3da9cb41a13152ea1ca7  storage_index.wasm.gz
4f05ea81ed5c963aa2203fcf566f406feb577a2867e5a9e6f59a1b95b12fb6a3  translations.wasm.gz
95485245466efc100ad4dfb64762ae166a661bf8ed5e61ca725599d42a0632de  user.wasm.gz
92f9e93c20021147e8eacaa1441b983ee533cd1eaabd518051ec3a954cfd8150  user_index.wasm.gz
```

| Surface | Command | Result |
|---|---|---|
| fmt | `cargo fmt --all -- --check` | clean |
| Clippy | `cargo clippy --locked --workspace --exclude open-chat --exclude tauri-plugin-oc --tests -- -D warnings` | exit 0 |
| Workspace units | `cargo test --locked --workspace --exclude open-chat --exclude tauri-plugin-oc --exclude integration_tests`, `CVDR_VERIFY_SIBLING` = CVDR-Verify at X | exit 0 — 339 passed, 0 failed, 0 ignored (per package below) |
| LUI units | `cargo test --locked -p local_user_index_canister_impl`, sibling at X | exit 0 — 101 passed, 0 failed (label + corpus-mirror guards ran against X) |
| PocketIC `cvdr_` suites | `./scripts/run-integration-tests.sh local 1 'cvdr_'` | 20 passed, 0 failed, 5 ignored |
| Ignored upgrade tests | `cargo test --locked -p integration_tests -- --ignored --test-threads 1 r3_upgrade v1_in_flight_draft_blocks_upgrade` (baseline wasms = BASELINE §3.2 hashes) | exit 0 — 2 passed, 0 failed (128.97 s) |
| `cvdr.spec.ts` | `npx vitest --run src/domain/cvdr.spec.ts` (`frontend/openchat-shared`) | 22 passed, 0 failed |
| CVDR-Verify CI at X | `bash ./ci.sh` (clone at X; open-chatZD visible as its sibling, so its reverse label guard ran) | exit 0 — fmt, clippy clean; audit zero vulnerabilities; 157 passed, 0 failed (148 + the 9-test corpus acceptance re-run) |
| CVDR-Verify corpus consumer | `cargo test --locked --test openchatzd_v5_corpus` at X | 7 passed, 0 failed |
| Fresh `PortablePackageV3` | from the `cvdr_` run (`stored_index_evidence_unblocks_upgrade_and_draft_finalises`), sha256 `d7ab1a720fc99208872fcb41491eedb6023d7cb04690f73d3ace616ffe100300`, verified with X `--allow-fixture-root-key` | exit 0 — `validity: PASS`, `V3A_PASS`, timing `BEFORE_COMMITMENT_CERTIFICATE`, certified module_hash `00b09636…` = built `local_user_index`; without the flag `validity: FAIL` (exit 1) |

Workspace units per package (passed/failed/ignored; packages with no tests omitted). The non-LUI
packages total 238; between `8118d26a` and this build the only Rust changes outside
`local_user_index` are doc comments in `integration_tests` (not in this run), and LUI went 95 → 101 —
so the same command at `8118d26a` counts 95 + 238 = 333.

| Package | p/f/i |
|---|---|
| `local_user_index_canister_impl` | 101/0/0 |
| `chat_events` | 50/0/0 |
| `utils` | 29/0/0 |
| `user_index_canister_impl` | 26/0/0 |
| `market_maker_canister_impl` | 21/0/0 |
| `user_canister_impl` | 15/0/0 |
| `translations_canister_impl` | 14/0/0 |
| `stable_memory_map` | 13/0/0 |
| `constants` | 9/0/0 |
| `storage_bucket_canister_impl` | 9/0/0 |
| `gated_groups` | 8/0/0 |
| `http_request` | 7/0/0 |
| `search` | 7/0/0 |
| `community_canister_impl` | 4/0/0 |
| `group_index_canister_impl` | 4/0/0 |
| `ledger_utils` | 4/0/0 |
| `group_chat_core` | 3/0/0 |
| `storage_index_canister_impl` | 3/0/0 |
| `ckbtc_minter_canister` | 2/0/0 |
| `local_user_index_canister` | 2/0/0 |
| `airdrop_bot_canister_impl` | 1/0/0 |
| `email_magic_links` | 1/0/0 |
| `instruction_counts_log` | 1/0/0 |
| `jwt` | 1/0/0 |
| `notification_pusher_core` | 1/0/0 |
| `proof_of_unique_personhood` | 1/0/0 |
| `sign_in_with_email_canister_impl` | 1/0/0 |
| `types` | 1/0/0 |

## Release build 2026-10-02 — AT R `9c77719dd378826c96586649c79ae42828480037`

| Identifier | Value |
|---|---|
| R (open-chatZD release; tag `v0.8.0` at freeze) | `9c77719dd378826c96586649c79ae42828480037` — the hashes below reproduce only when built AT R |
| Y (records commit) | on `v5-retrofit`, R..tip touches only `RELEASES.md`, the evidence file (this file) and `.github/workflows/*` — no build input; `git diff --stat v0.8.0 HEAD` shows only those files. Its SHA is recorded outside the tree (release ledger, tag annotation); R and X are the authoritative pins |
| X (CVDR-Verify; tag `openchatzd-v0.8.0` at freeze) | `b2547880c1dfda6304a097e97fb74fb640002f9f` = the `backend.yaml` pin |
| Recipe | `scripts/docker-build-all-wasms.sh` in a clean worktree AT R (no token), then the same recipe with a busted `build_nonce` — 23/23 identical |
| `local_user_index.wasm.gz` | `bfcc1346b46604b18405afdd5ff0b7c4b4ee743cde882dd963d8f3c0053d48bb` |

```
462d5c4b6f932b160efef6e408ccbf45d78eadb8066ec1afedbb27285c47e23c  airdrop_bot.wasm.gz
e4c9f5c6acd2d24a4b5858ecb6e65611b34d0f82bbf334e4268b6f8d559f086b  community.wasm.gz
3a5a983c25812ded47879926488fe3866a699ff137cd90cb7d0c6524ced918d0  cycles_dispenser.wasm.gz
62a1962934d4c18f8037bc5f3be38381b52985a433f643b2fd64256ec9599f40  escrow.wasm.gz
aeb57927bacbd1f8fbe301f2bfe14a0d877d33578e1415fe4374d73b3d756f73  event_relay.wasm.gz
ad9a691583280c1950e373312b11bc335e856f272552b6ffca26657329b758b3  group.wasm.gz
87010b83ec0b48acec088a88cac787dc31a3fcc0e3f5ed815badc82672b623c6  group_index.wasm.gz
d091e87cf1581d41dcfbfbb0688f789592eaec27db31b85ef5a6a22cdcc79db7  identity.wasm.gz
bfcc1346b46604b18405afdd5ff0b7c4b4ee743cde882dd963d8f3c0053d48bb  local_user_index.wasm.gz
ae9e3c7e3b10e88488a0ea4457bb706c23197f64cd643095482c01720c9df9e8  market_maker.wasm.gz
4c38f78dc3a6f0374154a9755106ef52f95079c8fae0bff41a460decfdcde6a2  neuron_controller.wasm.gz
590f30feac6f045dfdb51295e44e2d3a77b081c34e7e0adc4cea8829497e44f8  notifications_index.wasm.gz
ed9142b64c55b34278cd8e56594b541a25265e794b9438e13c280d72ae5f8fa0  online_users.wasm.gz
bd8e69d742723f6f04de145e2f3d22dbcd5a93c24b0851e8b266f81b36bda463  openchat_installer.wasm.gz
418a4117e905a9e4bae8aaed8d0ed689c9cc03001ff12fde174b74ca85e83f8e  proposal_validation.wasm.gz
9bbcb2ea3a305d41fe5f6b798963d0f389233de293ee00fbe6f0ab400798ff65  proposals_bot.wasm.gz
dd8995f896fc20d4066ea487c527d909a3195c16209f078d5864d29f633c7a83  registry.wasm.gz
c2b82a8be4045de96a64ae54d37fe5c379b1014cd970233896fbb7de928c8d95  sign_in_with_email.wasm.gz
119999a4f8d5831bdd7f59beff8da95e7109e7a067a83f1ccd2e808aa1f9642b  storage_bucket.wasm.gz
398670fa243e6b4e9fa2d423201c173907f394ea9d0f3445acf2b875d8eecb86  storage_index.wasm.gz
592c0d2f3acc20295b83d9d495d0addbf8aecd2c9ec8155b5c05c0219f2e3039  translations.wasm.gz
8c7dca35af2d9b54ca81f889b245cbb2ff377010f46cf347b355144895d488c2  user.wasm.gz
ed846b5bfb1ce5054f5eb383cbb54339d35ad8afad705969befbe27f1178e4d2  user_index.wasm.gz
```

| Surface | Command | Result |
|---|---|---|
| fmt | `cargo fmt --all -- --check` | clean |
| Clippy | `cargo clippy --locked --workspace --exclude open-chat --exclude tauri-plugin-oc --tests -- -D warnings` | exit 0 |
| Workspace units | `cargo test --locked --workspace --exclude open-chat --exclude tauri-plugin-oc --exclude integration_tests`, sibling at X | exit 0 — 339 passed, 0 failed, 0 ignored — per package identical to the `84b2ba866` table above |
| LUI units | `cargo test --locked -p local_user_index_canister_impl`, sibling at X | exit 0 — 101 passed, 0 failed (label + corpus-mirror guards ran against X) |
| PocketIC `cvdr_` suites | `./scripts/run-integration-tests.sh local 1 'cvdr_'` | 20 passed, 0 failed, 5 ignored |
| Ignored upgrade tests | both, `--ignored --test-threads 1`, baseline wasms = BASELINE §3.2 hashes | exit 0 — 2 passed, 0 failed (128.82 s) |
| `cvdr.spec.ts` | `npx vitest --run src/domain/cvdr.spec.ts` | 22 passed, 0 failed |
| CVDR-Verify CI at X | `bash ./ci.sh` (open-chatZD visible as its sibling) | exit 0 — fmt, clippy clean; audit zero vulnerabilities; 157 passed, 0 failed (148 + the 9-test corpus acceptance re-run); reverse label guard ran against open-chatZD |
| CVDR-Verify corpus consumer | `cargo test --locked --test openchatzd_v5_corpus` at X | 7 passed, 0 failed |
| Fresh `PortablePackageV3` | from the `cvdr_` run, sha256 `4a3fe8749f914a0f900a827b35c323c5f21de8e7135098e0d5a409214b75b2be`, verified with X `--allow-fixture-root-key` | exit 0 — `validity: PASS`, `V3A_PASS`, timing `BEFORE_COMMITMENT_CERTIFICATE`, certified module_hash `bfcc1346…` = built `local_user_index`; without the flag `validity: FAIL` (exit 1) |

## Baseline wasms for the ignored r3 upgrade tests

Authoritative identity: `c744de1a1124f9320354946d83cfc3da68133967`, canonical all-canister Docker
recipe (`docs/dev/v5/BASELINE_c744de1.md` §3.2). The four hashes below are pinned in
`backend/integration_tests/src/wasms.rs` (`BASELINE_C744DE1_SHA256`). They are what
`r3_upgrade_tests::r3_upgrade_from_c744de1_keeps_state_and_function` and
`cvdr_v2_upgrade_tests::v1_in_flight_draft_blocks_upgrade_then_terminal_draft_survives_it` load.

| Asset filename | sha256 |
|---|---|
| `openchat_installer.wasm.gz` | `81e2aa0b295122bd66efefcfc7dcda72c1910b9378486f2682e63d33d4177b64` |
| `user_index.wasm.gz` | `f4c2d4c98fb9e5e56ab2f099859c35f237b3fbae3a8b66a858f11da53b053357` |
| `local_user_index.wasm.gz` | `83601cf8e1c35f487011c94eb9a02519a785755240b175462bbf04264396a601` |
| `user.wasm.gz` | `eec4762080e2bdd941cf3f9dcb8530665d6dfa9d2a8a9d81828ab4d066b80b45` |

The files on release `baseline-c744de1-wasms` are the retained CD baseline artefacts. Their
SHA-256 values match the canonical all-canister c744de1 build hashes above. The hash equality is the
evidence; the release is transport only, and the release page is not part of the trust model. The
`upgrade-baseline` job accepts a downloaded file only if its SHA-256 equals a pin in `wasms.rs`, and
`wasms::baseline_c744de1` checks again on load.
