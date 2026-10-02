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
| G decisions 2026-10-01 | `221d2420a` … `9c77719dd` | timing label `BEFORE_COMMITMENT_CERTIFICATE` + historical `NOT_ATTESTED` (follow CVDR-Verify X), label guard matches definitions, placeholder evidence fails closed (hash / root, then certificate), CI pin → X |

**Release identifiers** (tags applied by Stef at freeze):
- **R** = `9c77719dd378826c96586649c79ae42828480037` — the open-chatZD release commit. Tag `v0.8.0` goes
  here: the hashes below reproduce only when built AT R.
- **Y** — the records commit on `v5-retrofit`: R..tip touches only `RELEASES.md`, the evidence file
  (`docs/evidence/2026-09-23-cd-final-gate.md`) and `.github/workflows/*` — no build input; R and X
  are the authoritative pins (full SHAs). `git diff --stat v0.8.0 HEAD` shows only those files. The SHA of Y itself is recorded outside the tree (release ledger, tag annotation).
- **X** = `b2547880c1dfda6304a097e97fb74fb640002f9f` — CVDR-Verify `openchatzd-v5`, tag `openchatzd-v0.8.0`;
  the `.github/workflows/backend.yaml` pin.

**Verifier:** CVDR-Verify `openchatzd-v5` — `9eb9614` (merge) → `37411f58` (verifier) →
`8b0d835` (corpus mirror) → `700a4bd1` (timing label rename) → `b2547880` (historical V2 →
`NOT_ATTESTED`) = **`b2547880c1dfda6304a097e97fb74fb640002f9f`**, tag `openchatzd-v0.8.0`, applied by
Stef at freeze; crate `mktd02-verify` 0.8.0, pinned in `.github/workflows/backend.yaml`.

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
- Two same-window all-canister builds AT `0740610b8fda284624566d492ce62ec9821e454f`: all 23 identical
  (LUI `2d2210d8…`); that table is kept in `docs/evidence/2026-09-23-cd-final-gate.md`.

**Cross-checks (G decisions, 2026-10-02):**
- Two same-window all-canister builds AT `84b2ba866a60ca929905eed364b207544da92385`: all 23 identical
  (LUI `00b09636…`); that table is kept in `docs/evidence/2026-09-23-cd-final-gate.md`.
- Two same-window all-canister builds AT R `9c77719dd378826c96586649c79ae42828480037` (`scripts/docker-build-all-wasms.sh`,
  then the same recipe with a busted `build_nonce`): **all 23 hashes identical** — the **published
  hashes below**, for a build AT R.

| wasm | sha256 — canonical recipe AT R `9c77719dd378826c96586649c79ae42828480037` (v0.8.0; `scripts/docker-build-all-wasms.sh`) |
|---|---|
| airdrop_bot | `462d5c4b6f932b160efef6e408ccbf45d78eadb8066ec1afedbb27285c47e23c` |
| community | `e4c9f5c6acd2d24a4b5858ecb6e65611b34d0f82bbf334e4268b6f8d559f086b` |
| cycles_dispenser | `3a5a983c25812ded47879926488fe3866a699ff137cd90cb7d0c6524ced918d0` |
| escrow | `62a1962934d4c18f8037bc5f3be38381b52985a433f643b2fd64256ec9599f40` |
| event_relay | `aeb57927bacbd1f8fbe301f2bfe14a0d877d33578e1415fe4374d73b3d756f73` |
| group | `ad9a691583280c1950e373312b11bc335e856f272552b6ffca26657329b758b3` |
| group_index | `87010b83ec0b48acec088a88cac787dc31a3fcc0e3f5ed815badc82672b623c6` |
| identity | `d091e87cf1581d41dcfbfbb0688f789592eaec27db31b85ef5a6a22cdcc79db7` |
| local_user_index | `bfcc1346b46604b18405afdd5ff0b7c4b4ee743cde882dd963d8f3c0053d48bb` |
| market_maker | `ae9e3c7e3b10e88488a0ea4457bb706c23197f64cd643095482c01720c9df9e8` |
| neuron_controller | `4c38f78dc3a6f0374154a9755106ef52f95079c8fae0bff41a460decfdcde6a2` |
| notifications_index | `590f30feac6f045dfdb51295e44e2d3a77b081c34e7e0adc4cea8829497e44f8` |
| online_users | `ed9142b64c55b34278cd8e56594b541a25265e794b9438e13c280d72ae5f8fa0` |
| openchat_installer | `bd8e69d742723f6f04de145e2f3d22dbcd5a93c24b0851e8b266f81b36bda463` |
| proposal_validation | `418a4117e905a9e4bae8aaed8d0ed689c9cc03001ff12fde174b74ca85e83f8e` |
| proposals_bot | `9bbcb2ea3a305d41fe5f6b798963d0f389233de293ee00fbe6f0ab400798ff65` |
| registry | `dd8995f896fc20d4066ea487c527d909a3195c16209f078d5864d29f633c7a83` |
| sign_in_with_email | `c2b82a8be4045de96a64ae54d37fe5c379b1014cd970233896fbb7de928c8d95` |
| storage_bucket | `119999a4f8d5831bdd7f59beff8da95e7109e7a067a83f1ccd2e808aa1f9642b` |
| storage_index | `398670fa243e6b4e9fa2d423201c173907f394ea9d0f3445acf2b875d8eecb86` |
| translations | `592c0d2f3acc20295b83d9d495d0addbf8aecd2c9ec8155b5c05c0219f2e3039` |
| user | `8c7dca35af2d9b54ca81f889b245cbb2ff377010f46cf347b355144895d488c2` |
| user_index | `ed846b5bfb1ce5054f5eb383cbb54339d35ad8afad705969befbe27f1178e4d2` |

**Reproduce:** `git checkout 9c77719dd && bash scripts/docker-build-all-wasms.sh` (R; tag `v0.8.0` once applied; no token) — hashes
embed `git_commit_id`, so only a build at that commit reproduces this table.

**End-to-end evidence:** a fresh `PortablePackageV3` served by the Step 10 `local_user_index` under
PocketIC (genuine commitment + `/module_hash` certificates) verified with the pinned verifier —
binary output in `docs/dev/v5/STEP10_E2E_VERIFY.txt` (Step 10 working-tree wasms, LUI `1ea8f3cd…`). On the
`8118d26a`, `0740610b8` and `84b2ba866` wasms: `docs/evidence/2026-09-23-cd-final-gate.md`. On the wasms below
(2026-10-02): the `cvdr_` PocketIC suites pass (20 passed, 0 failed, 5 ignored, one thread) and their fresh
`PortablePackageV3` (sha256 `4a3fe874…75b2be`) verifies with CVDR-Verify X as `validity: PASS`,
`V3A_PASS`, timing `BEFORE_COMMITMENT_CERTIFICATE`, certified module hash `bfcc1346…` =
`local_user_index` below; both ignored c744de1 upgrade tests pass. Per-package counts:
the evidence file.

**CI scope:** PR CI no longer supplies the PocketIC gate; the mandatory CD re-gate does, and green
GitHub CI is not the full release gate.

**Storage census:** MemoryIds 0, 3, 4 (retained), 5–6 (reserved), 7, 8–11, 12–14; 100–107
reserved by R-3, never reused. **Operators:** `docs/dev/v5/OPERATOR_NOTES.md`.

**Retired by this release:** `PROJECT_STATE` (stub), `CVDR_BUILD_SPEC_V1.md` (historical), the June
2026 P0/P1 packet docs (banners), `PortablePackageV2` (never emitted), `RECEIPT_BODY_V1` and
`OPENCHATZD_RECORD_ID_USER_V1` as live tags.
