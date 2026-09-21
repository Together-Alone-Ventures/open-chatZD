# OpenChatZD suite-v5 retrofit — Step 0 baseline @ c744de1

**Status:** COMPLETE for Brief B1 §0 — locked unit surface, same-window dual build, all-canister
Docker build with per-wasm hashes, and `concurrent_deletes_both_reach_awaiting_certificate` recorded
before any source edit. (§4b, the full pre-change integration suite, is an additional comparison
baseline for commit point A.)

Recorded by C, 2026-09-21. Pre-change means: `open-chatZD` `origin/antek` @
`c744de1a1124f9320354946d83cfc3da68133967`, nothing modified.

## 1. Source and environment

| Item | Value |
| --- | --- |
| Commit | `c744de1a1124f9320354946d83cfc3da68133967` (`origin/antek`; local `antek`, working branch `v5-retrofit` created from it) |
| Tree state | `git status --porcelain` empty before and after every run below |
| Host | WSL2 `Ubuntu` 24.04.3 on DESKTOP-9CJ40LT, kernel 6.6.87.2, 24 vCPU, 15 GiB |
| Toolchain authority | `rust-toolchain.toml` → 1.95.0 (`rustc 1.95.0 (59807616e 2026-04-14)`); host default 1.98.1 is not used inside the repo. Docker (`rust_version=1.95.0`, `ic-wasm 0.9.11`) is the reproducible-build authority |
| Docker | Engine 29.8.1, buildx v0.37.1, driver overlayfs |
| Private deps (lock) | `mktd02 0.4.1` @ `921d710c8e7295739ed6055991383bb0929e4cec` (tag `mktd02-v0.4.1`); `zombie-core 0.3.0` @ `508f2f8bb88f4395293168c6ef25c92a67dee894` (tag `zombie-core-v0.3.1`) |
| Host access to private deps | SSH via global `url.git@github.com:Together-Alone-Ventures/.insteadof` + `net.git-fetch-with-cli = true`; no token needed outside Docker |

Runs were made in a disposable layout (plan §2): `~/tav/_oczd_step0/open-chatZD` = `git worktree`
detached at `c744de1`; `~/tav/_oczd_step0/CVDR-Verify` = `git archive` export of CVDR-Verify
`ac64c1b881b46cda8ef909c4a23ae008f863536d` (the `backend.yaml` CI pin), so the cross-repo label
drift guard resolves the same sibling CI uses.

## 2. Locked unit surface — DONE

Command: `cargo test --locked --package local_user_index_canister_impl` (2026-09-21T07:40Z).

| Result | Count |
| --- | --- |
| Tests run / passed / failed / ignored | 80 / 80 / 0 / 0 |
| of which CVDR-named (`*cvdr*`) | 60 / 60 passed |
| `Cargo.lock` modified by the run | no |

Log: `~/tav/_oczd_step0/logs/unit_local_user_index.log`.

Sibling sensitivity: from the main checkout the drift guard resolves `~/tav/CVDR-Verify`
(branch `mktd02-v5` @ `560e483`), whose `openchatzd/` is the older `e884ac4` copy, not the CI pin.
Result of the same command there (2026-09-21T07:48Z, branch `v5-retrofit` @ `c744de1`,
`CARGO_TARGET_DIR` outside the repo): 80 / 80 passed, CVDR-named 60 / 60 — the guard's required
labels are present in both sibling revisions. Log:
`~/tav/_oczd_step0/logs/unit_local_user_index_main_checkout.log`.

## 3. Docker/WASM build and module hashes — DONE

The `gh_token` BuildKit secret is read from `~/.config/tav/gh_token` (mode 600, outside the repo).

**3.1 Same-window dual build** — `scripts/m2-repro-local-user-index.sh`, unmodified
(2026-09-21T09:28Z–09:40Z; rust 1.95.0, ic-wasm 0.9.11, linux/amd64, `--locked`, `build_nonce` 1 then 2).

| Build | `local_user_index.wasm.gz` SHA-256 (= on-chain module hash for a gzip install) | bytes |
| --- | --- | --- |
| build1 | `ee81c267a7b87f3bcb6a201e84a570ea49892e6fbc571a20772715e62605394f` | 3043859 |
| build2 | `ee81c267a7b87f3bcb6a201e84a570ea49892e6fbc571a20772715e62605394f` | 3043859 |

Verdict: **PASS — byte-identical.** SHA-256 of the gunzipped wasm:
`0839d0758d997accaccd75abe940f95f4244db741da4a74b8d1a679067a8d878`. (The M2 evidence hash `4d4a4128…`
was for `5b2077c`; `c744de1` contains later code, so a different value is expected.)
Artefacts: `~/tav/_oczd_step0/artifacts/m2-repro/`; log `logs/m2_dual.log`.

**3.2 All-canister Docker build** (2026-09-21T09:44Z–09:54Z), invoked directly:
`docker build --secret id=gh_token,src=~/.config/tav/gh_token --build-arg git_commit_id=c744de1… --build-arg canister_name= --platform linux/amd64 .`
Exit 0, 24 wasms. SHA-256 of each `.wasm.gz`:

```
24c87ebf74e99449989b9c9eca0628e6aa12dd69eebf9a11293a869b8fd7d8c3  airdrop_bot
699d0fc840061e4f40ed9e21248c44efcf465b79c6a6c87558f1e63070a581d8  community
9b95f6de728e05f759386abedb40b55db1cbc6398fadbb73f23213113ddeb8cc  cycles_dispenser
5fc681d5cae62da57330db29d1cd14efa8bcddc917731d78d4f283b366d7d59e  escrow
59f6bc6dd2d33d9f4598919df73f92484a813e17b3e7dc0fc8f404220cca41b7  event_relay
89eadd735f024cc2d63d8dedfb0ce9b59131f7f3b04ef2803c9c26e9ab428367  group
f507d4f57185d92c506ffa82aff9c0053ce1997585afd71126c4b993be960b75  group_index
aa485ec03bf7524e8ea56072cd8bc8bd4a507d184a5b6c22d9961b5961609289  identity
83601cf8e1c35f487011c94eb9a02519a785755240b175462bbf04264396a601  local_user_index
6ff21626258105de717b720865350b1e489baaec6bf1954c7050cdb1adbe981c  market_maker
0411e33b9b60c2f1985778bc19287be7d44b4d99a2425950ba4c5fd05039b18e  neuron_controller
d521df31cda6517850c718332b25c385985be331d4b15caa042657c8344040f1  notifications_index
206f4205106e4db8c7bc860914d7790919e1def8d07b019e7c30ff85a4779381  online_users
81e2aa0b295122bd66efefcfc7dcda72c1910b9378486f2682e63d33d4177b64  openchat_installer
a5aa86f8eab43117f1004556a8548be677f9f6b32e417d027df1ed8edfecfbe1  proposal_validation
695b912daf6eee4b2c95e7bb72b46c5f9bfaee645aad77aff7e279e55ce23d52  proposals_bot
2cbcb0286c9318f54f6b6be3221b07b88a87f9d819ea9ce84842359dee475df6  receipts
0049ff4b980c13fd49ebefb9c693e5569524f2d83a6cfb5d979f456f1b2c8f5c  registry
da61f351bda4ad035178885e3cb8ccb106f2f863493f992701ce77492b88441e  sign_in_with_email
53083b705c367ad4947990f111b354833e50deedbff094d26b934c1a680e8c8e  storage_bucket
e998e0d2fcb70e09826dab7e5c84c6ba4ad6eafec2a5ba2c6572c37869274802  storage_index
4a8de989f728e453c07bb4db067f7de6bb6927e0b8dd1416ab097390fa5860d3  translations
eec4762080e2bdd941cf3f9dcb8530665d6dfa9d2a8a9d81828ab4d066b80b45  user
f4c2d4c98fb9e5e56ab2f099859c35f237b3fbae3a8b66a858f11da53b053357  user_index
```

**Recipe dependence (provenance-relevant):** `local_user_index.wasm.gz` is `ee81c267…` from the
single-canister recipe (`canister_name=local_user_index`) but `83601cf8…` from the all-canister
recipe, same commit, toolchain and window — one `cargo build` over 24 packages unifies features
differently.

**Canonical recipe (ruling, Stef, 2026-09-21):** the **all-canister Docker recipe** of §3.2 is the
canonical reproducible-build path. A published module hash names that recipe, and the Step 10
same-window dual build is two all-canister Docker builds compared per wasm — not
`scripts/m2-repro-local-user-index.sh`, whose single-canister artefact (`ee81c267…`) is not the
deployed one. §3.1 stands as evidence that the build is deterministic, not as a publishable hash.

Known defects in the documented path (pre-existing, not fixed here):

1. `Dockerfile:42-53` runs under `set -euo pipefail`; with no `canister_name` build-arg the variable
   is unbound and the all-canister branch aborts (`canister_name: unbound variable`, observed
   09:43Z). Worked around above with an explicit empty `--build-arg canister_name=`; Dockerfile untouched.
   Ruling (Stef, 2026-09-21): fixed at plan Step 10 together with defect 2.
2. `scripts/docker-build-all-wasms.sh`
— the script `integration_tests.yaml` calls — runs `docker build` without
`--secret id=gh_token,...`, while `Dockerfile:42-45` hard-fails on a missing/empty secret. The
all-canister build will therefore be invoked directly with the secret flag; the script is left as is.
Ruling (Stef, 2026-09-21): fix at plan Step 10, not before, by passing
`--secret id=gh_token,src=~/.config/tav/gh_token` in `scripts/docker-build-all-wasms.sh`.

## 4. `concurrent_deletes_both_reach_awaiting_certificate` — DONE

`./scripts/run-integration-tests.sh local 6 concurrent_deletes_both_reach_awaiting_certificate`
over the §3.2 Docker-built wasms, PocketIC server 11.0.0 (2026-09-21T09:55Z): **1 passed; 0 failed;
384 filtered out; 70.25 s.** Log: `logs/it_concurrent_baseline.log`.

## 4b. Full pre-change integration suite (comparison baseline for point A)

`./scripts/run-integration-tests.sh local 6` over the §3.2 wasms (2026-09-21, 261.8 s):
**370 passed; 4 failed; 11 ignored** (385 tests). Log: `logs/it_full_baseline.log`.

| Failed at baseline (parallel, pooled env) | Assertion | In isolation (`--test-threads 1`, 2 rounds) |
| --- | --- | --- |
| `cvdr_tests::forged_or_stale_certificate_is_rejected` | `cvdr_tests.rs:1067` tampered cert not `Rejected` | pass, pass |
| `cvdr_tests::prepare_fails_after_user_deleted` | `cvdr_tests.rs:1392` | pass, pass |
| `delete_user_tests::deleted_user_removed_from_groups_and_communities` | `delete_user_tests.rs:109` | pass, pass |
| `mktd_deletion_tests::round_trip_a_simb_c` | `"mainnet"` vs `"local-dev"` — Docker wasms are built without `user_canister_impl/local-replica` | n/a (deterministic under the Docker recipe; test removed by R-3) |

The three CVDR/delete failures are order/parallelism-sensitive on the shared pooled environment, not
deterministic defects; they are pre-existing at `c744de1` and are to be compared, not attributed, at
later commit points. Ignored at baseline (11): six `cvdr_tests` (Slice 2/3 markers) and five
`receipts_tests` (P2 export path, banked).

## Appendix A. Step 1 closures that needed repo access (read-only)

**A.1 DaffyDefs `VENDORED_SOURCES.md` (daffydefs `0d21915`) vs upstream, by git blob hash.**

| Vendored tree | Declared upstream | Verdict |
| --- | --- | --- |
| `vendor/mktd02` (`2c17b53d…`) | ICP-Delete-Leaf `2a10bf3…`:`mktd02/` (`a44e6484…`) | 12/13 files blob-identical; `Cargo.toml` = cargo-vendor normalisation only (workspace values resolved to 0.6.0 / 2021 / Apache-2.0; same deps; zombie-core `rev = 2237238…` preserved); `+.cargo-checksum.json` (13/13 SHA-256 self-consistent) |
| `vendor/zombie-core` and `tools/cvdr-verify/vendor/zombie-core` (both `f3eade77…`) | zombie-core `22372388…` root (`e98de321…`) | 65/68 blob-identical; `Cargo.toml` normalisation only; `+.cargo-checksum.json` (66/66); absent upstream files: `.gitignore` (declared) and `docs/dev/phase6/.gitattributes` (**not declared** in VENDORED_SOURCES.md) |
| `tools/cvdr-verify` | CVDR-Verify `560e483…`:`mktd02/mktd02-verify/` (`873a0f3f…`) | 46/49 blob-identical; deltas: `Cargo.toml` `+[workspace]` (declared), `README.md` +5-line preface (**not declared**), `tests/retired_label.rs` walk skips `vendor` (**not declared**); plus added `.cargo/`, `bin/`, `vendor/` |

Uniqueness: zombie-core root tree `e98de321…` occurs at `2237238` only (v5 tip `6adc37f` is one
docs-only commit later; `src/` + `Cargo.toml` identical across `36782a0..6adc37f`). The `mktd02/`
subtree `a44e6484…` is shared by four ICP-Delete-Leaf commits — `45a3deb`, `958679b`, `eeb55b6`,
`2a10bf3` — with identical root `Cargo.toml`/`Cargo.lock`; they differ only in `harness/`, `docs/`,
`RELEASES.md`. The vendored tree therefore fixes the engine source exactly but does not by itself
single out `2a10bf3`; the declaration does.

**A.2 Release tags.** None exist (local or `ls-remote`) at ICP-Delete-Leaf `2a10bf3` (nearest
`mktd02-v0.5.0`+58), zombie-core `2237238` (nearest `zombie-core-v0.4.1`+58) or CVDR-Verify `560e483`
(nearest `v0.6.1`+32). `RELEASES.md` in both engine repos marks 0.6.0 / 0.5.0 "DRAFT — not tagged".
Moot for OpenChatZD after R-3 option (i); still open for the suite.

**A.3 OpenChat verifier lineage (CVDR-Verify).** `ac64c1b` (branch
`openchatzd-portable-v2-bytes-receipt-id`, 4 commits, crate 0.7.0, untagged, unmerged) and `560e483`
(branch `mktd02-v5`, 29 commits, crate 0.8.0 DRAFT) are divergent siblings of `main` @ `e884ac4`;
neither contains the other. `mktd02-v5` never touched `src/openchatzd/`, so its copy lacks the four
OpenChat fixes (FrozenWire bytes-only, `receipt_id` recompute, nested-bytes retention, tests). The
OpenChat module couples to generic code only through `crate::v2_certificate`
(`verify_certificate_over_certified_data`, module-hash certificate path) and `main.rs` dispatch;
`v2_certificate.rs` is the single file both lines changed. Trial merge in a throwaway clone: clean
(tree `632d7a92…`), builds, `cargo test --locked` 139/139 passed incl. 56/56 `openchatzd` (rustc 1.97.1).
Wire at `ac64c1b`: frozen schema v1, portable schema `openchatzd.cvdr.portable_package` v2.

## 5. Step-3 upgrade interlock — design, stated before coding (Brief B1 R-2; ruling (c), Stef 2026-09-21)

**What must hold.** V3A = "the authenticated `/canister/<LUI>/module_hash` certificate names the code
that performed the deletion". The certificate is fetched *after* the uninstall, so that claim is true
only if the Index was not upgraded between `uninstall_completed_at` and the certificate `/time`.
The interlock guarantees exactly that interval; it guarantees nothing else.

**Observed constraint (point B, PocketIC).** An upgrade is stop → `install_code` → start. A canister
with an open call context (e.g. the self-finalisation HTTP outcall) does not finish *stopping*, so
no hook — old or new — runs until its outcalls resolve. The interlock therefore must not depend on
the stop path, and no test may treat "still Stopping" as "refused".

**Blocking set.** A draft blocks an upgrade iff all of:
1. stage ∈ {`Uninstalled`, `AwaitingCertificate`, `FailedStuck`} (`Prepared` has no receipt and no
   uninstall — it never blocks; it is resumed or TTL-purged as today);
2. no index evidence is stored for its `receipt_id`;
3. its evidence window is still open: `now − anchor ≤ 24 h`, anchor = `uninstall_completed_at`
   (falling back to `receipt_committed_at`). After the existing 24 h give-up the receipt is
   permanently V3A-UNAVAILABLE whatever happens next, so blocking longer protects nothing and
   would make the Index un-upgradable for ever. Computed from durable draft fields, not heap state.

**Two checks, same predicate (`CvdrStore::upgrade_blockers(now_ns)`), both before any state change.**
- `pre_upgrade` (running wasm): first statement, before `take_state()`/serialisation — trap with the
  refusal text. Authoritative: it holds whatever wasm is being installed.
- `post_upgrade` (incoming wasm): in `resume_in_flight_drafts`, next to the pre-V2 refusal and before
  the tree rebuild / re-queue / `certified_data_set` — covers upgrades *from* a wasm that predates
  the `pre_upgrade` check. A trap in either hook fails `install_code`; the old wasm and its state
  stay installed and `user_index` restarts it.
- Refusal text: count, each blocker as `prefix(Stage)` (§11.6 prefix, max 20), and the operator
  path: leave the current wasm running, the evidence sweep captures within seconds-to-minutes
  (or the window lapses at 24 h), then upgrade again.

**Evidence capture moves earlier.** Today capture starts only once a frozen package exists, which
leaves `Uninstalled → package stored` unprotected. Step 3 starts the sweep at `Uninstalled`, keys
evidence by `receipt_id` for a draft *or* a package, stores the certificate **and** the extracted
`index_module_hash`, and replaces "cert `/time` ≥ commitment certificate time" with
"cert `/time` ≥ `uninstall_completed_at`" (hash-bound in RECEIPT_BODY_V2, so a verifier can re-check
it). One fetched certificate serves every draft it post-dates. The deployer value survives only as
`expected_index_module_hash: Option<Hash>` — compared with the extracted hash for an ops warning +
metric, never stored as evidence, never in a preimage; `Option` so a not-yet-upgraded `user_index`
(still sending `executor_module_hash`) can upgrade a new Index.

**Tests (point C).** Unit: predicate truth table (stage × evidence × window), refusal text, stored
evidence struct decode of the pre-step-3 shape. PocketIC: (i) draft without evidence → upgrade
refused, refusal names the receipt, and the **old wasm is still serving** (`/cvdr`, `/metrics`,
a new registration) afterwards; (ii) evidence stored → same upgrade succeeds and the draft
finalises; (iii) window lapsed → upgrade succeeds; outcalls are answered in every polling loop.

**Known liveness limit (proposed ruling, not built in step 3).** A steady stream of deletions can
keep some draft inside its first seconds-without-evidence at any instant. Decision proposed: accept
for now (upgrade tooling retries; the per-draft block lasts seconds), and add an operator
"quiesce new uninstalls" switch only if Step 9 upgrade tests show real contention.
