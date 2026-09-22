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
| Step 10 | *(this commit)* | build-recipe fixes, docs regeneration, operator notes, release number |

**Verifier:** CVDR-Verify `openchatzd-v5` — `9eb9614` (merge) → `37411f58` (verifier) →
`8b0d835` (corpus mirror), crate `mktd02-verify` 0.8.0, pinned in `.github/workflows/backend.yaml`.

**Cross-checks (Step 10):**
- C2 `86ddca205` rebuilt with the canonical recipe (`git_commit_id = 86ddca205d76932df6f175007e653dd5c0872943`):
  `local_user_index.wasm.gz` = `d1e0c49608013aeecd28a1927c6155ba6f5c1ad0b058efe826e210fe905945e5` —
  **equal to CD's independent build.**
- Step 10 tree, two same-window all-canister builds (distinct `build_nonce`,
  `git_commit_id = f314663c5945b699cee50250514a5a1e4dce935d-step10-worktree`): **all 23 hashes identical.**
  These are determinism evidence for the tree; the publishable hashes are produced by the recipe
  at the Step 10 commit itself (`git_commit_id` = that SHA) and are recorded below once built.

| wasm | sha256 (Step 10 tree, `…-step10-worktree` commit id) |
|---|---|
| airdrop_bot | `7fd5b8ca86399be84f6abc72801a6fffcaff18e69429779df4d8ce86d0e996c0` |
| community | `ba1e01b57e5fbfd3ef47e1e36a2855386630456bf494b5856f6c78613128fb08` |
| cycles_dispenser | `438ccb03f0f83120a7c42907ddeb47a70c67401380e837da9df2bb98d172e520` |
| escrow | `1aa0836752c6462b24bc8d4d02f2e6215a0e7983dc8153734ab087da61e5ea95` |
| event_relay | `2292efb00449401a9f80545d3ccb8920a7f41b12c94f020a74ff5ee506b63c4f` |
| group | `0b286184a6be79392167de2db54e79dbe0fd321efe93dd483e6e7281dd1b702f` |
| group_index | `192a5f78982d933181328b0c3dfc0d66d4f5ca09b103b6aa35a8487cfb48f1bc` |
| identity | `3cffcc8d4c0fbbac2e273325b139282cf4c955232379c946437345303c261aa5` |
| local_user_index | `1ea8f3cdad74c40661ced89c8823dae6817b50689cfd32741f285d96136dcfd2` |
| market_maker | `0827b4a275ec420b4779b51e98e873be2c282e2a51d5fa758dfa26e683e64a2d` |
| neuron_controller | `aab02e59512d7281a9d1e39918687a702b0575bb5a1696671761f6ad963c65a1` |
| notifications_index | `ee90b5d57982803b1901e387d23383b99667ad50373e38fdbf75971fa8ad2148` |
| online_users | `00e0c09d475f393ffdadb3b9a561ebd460f50352094eb55a4faa498f946d8910` |
| openchat_installer | `d70c6ad7f272860004b500bf0a84f5fe21b0ea27d16181a2f22bc6153363b850` |
| proposal_validation | `418a4117e905a9e4bae8aaed8d0ed689c9cc03001ff12fde174b74ca85e83f8e` |
| proposals_bot | `55df571bedcc4eb88de60b1db57748f23774fc786bd46fad493195758dc458ec` |
| registry | `d4567cc8e5242f239418428d2ed895aefe8b5dcef2cc81ea8a631c83fcedc09e` |
| sign_in_with_email | `c2b82a8be4045de96a64ae54d37fe5c379b1014cd970233896fbb7de928c8d95` |
| storage_bucket | `697a84ef7030411c4f6516a5d905e9361eeeffe2a1ebe000c386ee2703cec37f` |
| storage_index | `47bdb04d622aa976bfe027e16b48e9cbee2211a56d1dbe1ad383202ff2e48113` |
| translations | `0c2ba39ddc545b9f60d264e24cd7acd81937b550c3c1a454ac3bcf13d5281e27` |
| user | `ffda0238ba624dc4fe7c82479b42aa6e765191e08eb42dfc89cc0acea098592c` |
| user_index | `2eceade8a387e8f73f16165bd164e556fea67517a7228bdbdf6583d7c142483f` |

**End-to-end evidence:** a fresh `PortablePackageV3` served by the Step 10 `local_user_index` under
PocketIC (genuine commitment + `/module_hash` certificates) verified with the pinned verifier —
binary output in `docs/dev/v5/STEP10_E2E_VERIFY.txt`.

**Storage census:** MemoryIds 0, 3, 4 (retained), 5–6 (reserved), 7, 8–11, 12–14; 100–107
reserved by R-3, never reused. **Operators:** `docs/dev/v5/OPERATOR_NOTES.md`.

**Retired by this release:** `PROJECT_STATE` (stub), `CVDR_BUILD_SPEC_V1.md` (historical), the June
2026 P0/P1 packet docs (banners), `PortablePackageV2` (never emitted), `RECEIPT_BODY_V1` and
`OPENCHATZD_RECORD_ID_USER_V1` as live tags.
