# OpenChatZD — releases

Tags are minted once, at the final release commit; an entry here without a tag is the release
number reserved for the line. Module hashes name the **canonical all-canister Docker recipe**
(`scripts/docker-build-all-wasms.sh`; `CVDR_BUILD_SPEC.md` §8). `GIT_COMMIT_ID` is embedded in
every wasm, so a hash is a function of the committed tree *and* the commit id passed at build time.

## v0.8.0 — suite v5 retrofit "Zombie Delete" (branch `v5-retrofit`; UNTAGGED)

Base `origin/antek` @ `c744de1`. Rulings R-1…R-6 (Brief B1, adopted 2026-09-21) and the C2
amendment (2026-09-22). Governing spec `CVDR_BUILD_SPEC.md`; record `docs/dev/v5/BASELINE_c744de1.md`.

| Commit point | SHA | Content |
|---|---|---|
| A | `3c2aa381d` | R-3: ceremonial MKTd02/MKTd03 dependency removed; baseline doc; pinned baseline wasms |
| B | `b798bfb01` | R-1 `record_id_v2` + `record_salt` / RevealWire v2; R-4 `RECEIPT_BODY_V2`; R-2 body |
| C | `d74d238ef` | R-2 evidence capture from `Uninstalled`, code epoch, upgrade interlock (pre/post_upgrade) |
| D | `5e85b4d30` | R-6 `PortablePackageV3`, trust-root selector stamped at the store-gate |
| C2 | `86ddca205` | certified-time store-gate (epoch, 24 h window, absence) — G invariant verbatim |
| E | `5f42fd31f`, `ac2125a05` | verifier companion (claim wording, drift guards); CI pin to CVDR-Verify `openchatzd-v5` |
| Step 9 | `f314663c5` | vector corpus (hash-gated), Antoine regression invariants, flake isolation |
| Step 10 | `8118d26aa` | build-recipe fixes, docs regeneration, operator notes, release number |
| Review follow-ups | `8775c1cb6` … `0740610b8` | Antoine review 2026-09-30: serve-path tests (`8775c1cb6` Antoine, `6814cbc57`), hash provenance labels + CD final-gate evidence (F1), `upgrade-baseline` CI job (F2), CVDR-Verify sibling precondition, pre-R-2 evidence / operator checklist (F4), stale body comment (F5), Docker `gh_token` secret removed |

**Verifier:** CVDR-Verify `openchatzd-v5` — `9eb9614` (merge) → `37411f58` (verifier) →
`8b0d835` (corpus mirror), crate `mktd02-verify` 0.8.0, pinned in `.github/workflows/backend.yaml`.

**Cross-checks (Step 10):**
- C2 `86ddca205` rebuilt with the canonical recipe (`git_commit_id = 86ddca205d76932df6f175007e653dd5c0872943`):
  `local_user_index.wasm.gz` = `d1e0c49608013aeecd28a1927c6155ba6f5c1ad0b058efe826e210fe905945e5` —
  **equal to CD's independent build.**
- Step 10 tree, two same-window all-canister builds (distinct `build_nonce`,
  `git_commit_id = f314663c5945b699cee50250514a5a1e4dce935d-step10-worktree`): **all 23 hashes identical.**
  Those were determinism evidence for the tree; the canonical recipe run at the Step 10 commit
  `8118d26aae9c4194b4bc3ded0e58be578648474a` itself gave the v0.8.0 table that CD's final gate
  reproduced 23/23 (LUI `6cfdb4ac…`); that table is kept in `docs/evidence/2026-09-23-cd-final-gate.md`.

**Cross-checks (review follow-ups, 2026-10-01):**
- The `8118d26a` tree, built with the secret-free Dockerfile on a host with no GitHub token,
  reproduces the `8118d26a` table 23/23 (`git_commit_id` = `8118d26a…`) and the Step 10 set 23/23
  (`git_commit_id` = `f314663c5…-step10-worktree`, LUI `1ea8f3cd…`): the removed secret never
  entered the output.
- Two same-window all-canister builds AT `0740610b8fda284624566d492ce62ec9821e454f` (`scripts/docker-build-all-wasms.sh`, then the
  same recipe with a busted `build_nonce`): **all 23 hashes identical** — the **published hashes
  below**.

| wasm | sha256 — canonical recipe at `0740610b8fda284624566d492ce62ec9821e454f` (review follow-ups; `scripts/docker-build-all-wasms.sh`) |
|---|---|
| airdrop_bot | `f8207677404e448dec8ad80a160431c7c55cde74196fc0285ce055e474684d0a` |
| community | `c118e5fd104461cf3bfb06b6f12174350c51351b1b46e3ca50097301b2624b09` |
| cycles_dispenser | `f7b301b8d4263436dd168ebd52666526950e6a8580ff24a1d5ab399267e727fc` |
| escrow | `c4c73bb5ddc31cfe509fb3686e6bbfa7488880945b6a8bd35535b0031b6b69a0` |
| event_relay | `ec1b661388d3ac720141120fe0f3cd127a92a8496d54ed0214648af07e541572` |
| group | `d0c3190684c4ed2068cf9be07717aa5f2b2c42eb774ec32758f7f51d15102fd0` |
| group_index | `f8d833d2310b08817577bbac8bf6b9cd7cc69e87267e5778a2f6b2ac5976e40a` |
| identity | `695be1cccc26664b31232b6bb7e19b215b5bd1f1c5d61dff1ae31b7e0ec364b3` |
| local_user_index | `2d2210d8028ac06cb85bba71aa094081a741863a8708ce691c77cbfd02bfc416` |
| market_maker | `8b820c99b25ff57619332f60880f5acc8bc9b2cbb0f23f7d4aeaba3cc3cfc0eb` |
| neuron_controller | `b2c8fe1b169deef17a7682c9874b706dfeab6a59bb2b60dae71f70bc143672eb` |
| notifications_index | `d19d17f5b17c00a61ca36cd46cb4a15718fc80bd5ae37ad932e5f38d9292a61b` |
| online_users | `363967cc6b1e0b6ee22100a650382860a96e34e519357d4b5cd358a68c53e661` |
| openchat_installer | `1bce70d92f21593685adf4540a5288f2855f49c67d879ad7cac78ea4bb580dc4` |
| proposal_validation | `418a4117e905a9e4bae8aaed8d0ed689c9cc03001ff12fde174b74ca85e83f8e` |
| proposals_bot | `5ab1ba9a8286a07cd91e1ccd105443e0d451136237ac69115da317db1b7bb163` |
| registry | `18fe15d281c0a220b0ce957020fe4ee82928ed84a2bc3e16768b541f73610d27` |
| sign_in_with_email | `c2b82a8be4045de96a64ae54d37fe5c379b1014cd970233896fbb7de928c8d95` |
| storage_bucket | `678825ed75fd00594be3a57d14e4199554141173e9df8c653fbd07a71498c3b1` |
| storage_index | `8a7e2845e603fd8fa669ae4415781aaa0ec1a6fa9cb7514d42516467910a18c2` |
| translations | `63ae0bcff754b00055154b5760f99ed4fb0988ea106ac02a2bf76ba2b3bb8c97` |
| user | `e15bf18a218a16e5fef408770222454b433c7ce6bb9fb6d27367a6a98184fcff` |
| user_index | `4b8280a4d21dba0a4125e36c1e7010110f5ccd3d0cef385f764267017e5622a7` |

**Reproduce:** `git checkout 0740610b8 && bash scripts/docker-build-all-wasms.sh` (no token) — hashes
embed `git_commit_id`, so only a build at that commit reproduces this table.

**End-to-end evidence:** a fresh `PortablePackageV3` served by the Step 10 `local_user_index` under
PocketIC (genuine commitment + `/module_hash` certificates) verified with the pinned verifier —
binary output in `docs/dev/v5/STEP10_E2E_VERIFY.txt` (Step 10 working-tree wasms, LUI `1ea8f3cd…`). On the
`8118d26a` wasms: `docs/evidence/2026-09-23-cd-final-gate.md`. On the wasms below (2026-10-01): the
`cvdr_` PocketIC suites pass (20 passed, 5 ignored, one thread) and their fresh `PortablePackageV3`
(sha256 `ce8af407…099413`) verifies with CVDR-Verify `c27771b3` as `validity: PASS`, `V3A_PASS`,
certified module hash `2d2210d8…` = `local_user_index` below; both ignored c744de1 upgrade tests pass.

**Storage census:** MemoryIds 0, 3, 4 (retained), 5–6 (reserved), 7, 8–11, 12–14; 100–107
reserved by R-3, never reused. **Operators:** `docs/dev/v5/OPERATOR_NOTES.md`.

**Retired by this release:** `PROJECT_STATE` (stub), `CVDR_BUILD_SPEC_V1.md` (historical), the June
2026 P0/P1 packet docs (banners), `PortablePackageV2` (never emitted), `RECEIPT_BODY_V1` and
`OPENCHATZD_RECORD_ID_USER_V1` as live tags.
