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
| G decisions 2026-10-01 | `221d2420a` … `84b2ba866` | timing label `BEFORE_COMMITMENT_CERTIFICATE` + historical `NOT_ATTESTED` (follow CVDR-Verify X), label guard matches definitions, placeholder evidence fails closed, CI pin → X |

**Release:** open-chatZD `84b2ba866a60ca929905eed364b207544da92385` — tag `v0.8.0`, applied by Stef at
freeze. The hashes below reproduce only at that commit; the docs commit that records them follows it.

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
- Two same-window all-canister builds AT `84b2ba866a60ca929905eed364b207544da92385` (`scripts/docker-build-all-wasms.sh`,
  then the same recipe with a busted `build_nonce`): **all 23 hashes identical** — the **published
  hashes below**, for a build AT `84b2ba866`.

| wasm | sha256 — canonical recipe at `84b2ba866a60ca929905eed364b207544da92385` (v0.8.0; `scripts/docker-build-all-wasms.sh`) |
|---|---|
| airdrop_bot | `139f553429b474dd762ea13eed8fdc79c8d819c82aaa7ee670d22e4fa7393e49` |
| community | `bd604332ac8374806a8e6be3e265515ab4df029f2d97b9dc8e5ab0a6ac9ba8ed` |
| cycles_dispenser | `51454e4f87fe6c3733b31566fb2bb4236c6f31b7248f4e22229514569de01715` |
| escrow | `dc2aa416c18640328b75ff5e52807981fe27a38ad5251a861dd7aea2dafe55ac` |
| event_relay | `92800ceb17c9fa7c7d4898ed1ba99217372db56f3ab270373d16ef14b40c8f2b` |
| group | `a9d53f391f980e123e8034640e61b5cc0d24bd7f7bf9e04adf52ded2c8f2bdf1` |
| group_index | `de3ec201ad3b57ae7f6af0c3600d10dff32d080fd984ce08943a70d8c601fac3` |
| identity | `acfe5ae355c6fad351141a56a0dabe406a5a193ee0fffdfe0b509eac2d92710b` |
| local_user_index | `00b09636e1ac27cd4fc281cfb0e862ad8fc507733bca0e72ea3ed2e9d81b40e4` |
| market_maker | `7f6f022f002bd44fc84143ff1829ac68e0ef763108e55dcf16eaf40cbe75d974` |
| neuron_controller | `6666e358eab7b1ce8f74e9f86a1ca327b9cb9f8d622971f360af21e48f1e0c2f` |
| notifications_index | `cf8c4c9f261671b98c655f4733cabffeade378fd8f3cd886738f3b7f303f9cf0` |
| online_users | `f45b87678275fd0499ea50bbbbe6f5165e027a111393999e111cbd1dd64d2b61` |
| openchat_installer | `e9764f32be44bbe604fb4e70276e39d2290e49aeff2f4e02bcf25a7756885205` |
| proposal_validation | `418a4117e905a9e4bae8aaed8d0ed689c9cc03001ff12fde174b74ca85e83f8e` |
| proposals_bot | `08a8a48bbfe3181af330f4313962ce1bbbf1e64a89f2611ff72163ddf281502d` |
| registry | `32a0b92ce522dd0331e0aba5e7909ea972940bf733b55eeda462fb08afe2305b` |
| sign_in_with_email | `c2b82a8be4045de96a64ae54d37fe5c379b1014cd970233896fbb7de928c8d95` |
| storage_bucket | `7ccd61a64079fd63d9cf830514c5cee802252f1acabc16d5d8fd244d63a3caee` |
| storage_index | `c93fd8e6a5fc9db2414410142c6175ae595d093e442b3da9cb41a13152ea1ca7` |
| translations | `4f05ea81ed5c963aa2203fcf566f406feb577a2867e5a9e6f59a1b95b12fb6a3` |
| user | `95485245466efc100ad4dfb64762ae166a661bf8ed5e61ca725599d42a0632de` |
| user_index | `92f9e93c20021147e8eacaa1441b983ee533cd1eaabd518051ec3a954cfd8150` |

**Reproduce:** `git checkout 84b2ba866 && bash scripts/docker-build-all-wasms.sh` (no token) — hashes
embed `git_commit_id`, so only a build at that commit reproduces this table.

**End-to-end evidence:** a fresh `PortablePackageV3` served by the Step 10 `local_user_index` under
PocketIC (genuine commitment + `/module_hash` certificates) verified with the pinned verifier —
binary output in `docs/dev/v5/STEP10_E2E_VERIFY.txt` (Step 10 working-tree wasms, LUI `1ea8f3cd…`). On the
`8118d26a` and `0740610b8` wasms: `docs/evidence/2026-09-23-cd-final-gate.md`. On the wasms below
(2026-10-02): the `cvdr_` PocketIC suites pass (20 passed, 0 failed, 5 ignored, one thread) and their fresh
`PortablePackageV3` (sha256 `d7ab1a72…100300`) verifies with CVDR-Verify X as `validity: PASS`,
`V3A_PASS`, timing `BEFORE_COMMITMENT_CERTIFICATE`, certified module hash `00b09636…` =
`local_user_index` below; both ignored c744de1 upgrade tests pass. Per-package counts:
the evidence file.

**Storage census:** MemoryIds 0, 3, 4 (retained), 5–6 (reserved), 7, 8–11, 12–14; 100–107
reserved by R-3, never reused. **Operators:** `docs/dev/v5/OPERATOR_NOTES.md`.

**Retired by this release:** `PROJECT_STATE` (stub), `CVDR_BUILD_SPEC_V1.md` (historical), the June
2026 P0/P1 packet docs (banners), `PortablePackageV2` (never emitted), `RECEIPT_BODY_V1` and
`OPENCHATZD_RECORD_ID_USER_V1` as live tags.
