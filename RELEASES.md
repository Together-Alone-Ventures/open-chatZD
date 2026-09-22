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

**Verifier:** CVDR-Verify `openchatzd-v5` — `9eb9614` (merge) → `37411f58` (verifier) →
`8b0d835` (corpus mirror), crate `mktd02-verify` 0.8.0, pinned in `.github/workflows/backend.yaml`.

**Cross-checks (Step 10):**
- C2 `86ddca205` rebuilt with the canonical recipe (`git_commit_id = 86ddca205d76932df6f175007e653dd5c0872943`):
  `local_user_index.wasm.gz` = `d1e0c49608013aeecd28a1927c6155ba6f5c1ad0b058efe826e210fe905945e5` —
  **equal to CD's independent build.**
- Step 10 tree, two same-window all-canister builds (distinct `build_nonce`,
  `git_commit_id = f314663c5945b699cee50250514a5a1e4dce935d-step10-worktree`): **all 23 hashes identical.**
  Those were determinism evidence for the tree; the **published hashes below** are the canonical
  recipe run at the Step 10 commit `8118d26aae9c4194b4bc3ded0e58be578648474a` itself (`git_commit_id` = that SHA).

| wasm | sha256 — canonical recipe at `8118d26aae9c4194b4bc3ded0e58be578648474a` (Step 10 commit; `scripts/docker-build-all-wasms.sh`) |
|---|---|
| airdrop_bot | `b440377919f45ff49400c56c0cf3289729d5b26953e2b6a7e8983997d6f859c7` |
| community | `ec62fdedc0ba697f811b99a7b856070ac6e042de88f5153ba9343c9ce822ea98` |
| cycles_dispenser | `eb3ad7c0155cd132e4f650bd9f66157657b54534c83eab8d49e4e373c71667b2` |
| escrow | `989145edd32c5b4cb83de4213dc31654aa1959b09d4f161add81935665db292d` |
| event_relay | `68cef218792340ebae6cc2bf9d431cb8794132eace21c3f3688613fbbd21e0d3` |
| group | `a549f4d906fcef4dac2faf403a2917cdf0cea9ba62f578c077d5c87f0d274827` |
| group_index | `f9c9e2ed7dcd4561c8a3ead4f5562d16dd55feec80aa22ddf988e3f2fe03ced8` |
| identity | `1b9d118d8cbb1789c3786e885f9e8fdfdda7de963f73c0b1b40517d25e4f3b19` |
| local_user_index | `6cfdb4ac075c601e8dd09104af7dbadde7f92108477228b9846e9999c03690c7` |
| market_maker | `7d008992d012647fa6fdd15ad3304fe6fd5c12eedea94f97df4a06db3339c759` |
| neuron_controller | `d66c7844d6ccad63edd60d6d46f0bf3c3366cfbd0dac7f67675e8aea7c8e51f2` |
| notifications_index | `5084add5c795f404a7d22356924e5f914bb1dd356c30689764e649937a71c878` |
| online_users | `491d687f9b41c476c69891cab563462db2bbc9a9176b23e83a6ef57bdd555462` |
| openchat_installer | `341cf8b1a48d5e5713eae9f98e0fd8f18030b76e030c661efb6128db302136d6` |
| proposal_validation | `418a4117e905a9e4bae8aaed8d0ed689c9cc03001ff12fde174b74ca85e83f8e` |
| proposals_bot | `699c0db701246c54e70480830494e69f5940d14304f996a9d5b12068c11e9b13` |
| registry | `0c87ea55cc6a0e37ec860aa4169046241b560678c4a3cbcce4794f1148982e0e` |
| sign_in_with_email | `c2b82a8be4045de96a64ae54d37fe5c379b1014cd970233896fbb7de928c8d95` |
| storage_bucket | `21e41dffa908850e502669fd3755542ccca534639d35bce2ac3097e0a658b35d` |
| storage_index | `14f3efd6531490ad34b9f5bfb73a3364807ef38f582008a79848ee574f8389c8` |
| translations | `74d4d6699c4e8411d5f01fe8a4798adc843ddd3c0fba1c48dc7976928cdadf0f` |
| user | `36cc902354360b30621fcfe6f473ce4d813d842152aa1131ec609fae24858f44` |
| user_index | `61395cd69840eca39298090930fe23519d2e80a05507870210384cb27d81f129` |

**End-to-end evidence:** a fresh `PortablePackageV3` served by the Step 10 `local_user_index` under
PocketIC (genuine commitment + `/module_hash` certificates) verified with the pinned verifier —
binary output in `docs/dev/v5/STEP10_E2E_VERIFY.txt`.

**Storage census:** MemoryIds 0, 3, 4 (retained), 5–6 (reserved), 7, 8–11, 12–14; 100–107
reserved by R-3, never reused. **Operators:** `docs/dev/v5/OPERATOR_NOTES.md`.

**Retired by this release:** `PROJECT_STATE` (stub), `CVDR_BUILD_SPEC_V1.md` (historical), the June
2026 P0/P1 packet docs (banners), `PortablePackageV2` (never emitted), `RECEIPT_BODY_V1` and
`OPENCHATZD_RECORD_ID_USER_V1` as live tags.
