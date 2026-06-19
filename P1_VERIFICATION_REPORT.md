# Independent Verification — OpenChatZD P1 (CD's Phase 1)

**Role:** independent verifier (I did not implement). **Repo:** `master` @ `7b5ac7ccb` + CD's uncommitted Phase 1 working tree. **Engine:** `mktd02` @ v0.4.1 (`921d710`). **Method:** `git fetch` done; inspected actual files; ran unit + PocketIC suites on a fresh wasm build. **No changes, nothing staged.**

## HEADLINE DIVERGENCE
**Two ratified-scope requirements are NOT met:**

1. **Encoder version is bumped but NOT recorded** (Item: "encoder version bump … recorded in receipt/meta"). `PII_ENCODER_VERSION = "OPENCHATZD_USER_PII_V2"` exists (`mktd.rs:61`) but its only other use is a **dead binding** `let _schema_version = PII_ENCODER_VERSION;` (`mktd.rs:319`) — discarded before `encode_pii_state(&pii)`. It is **not** in the preimage, the receipt, or meta. The deliverable claims "historical receipts reproduce against the version recorded at issue time" (`MKTd02_P1_Deliverable.md:42-43`) — unsupported by code; the doc header also still reads `…PII_V1` (`:38`) while the body says V2 (`:45`).

2. **No real "shared CVDR-Verify (V1–V3)" round-trip exists** (Item 4). There is **no verifier** in `mktd02` (only `init/execute/finalize/get_receipt/…` — `mktd02/src/lib.rs:71-279`), none in `zombie_core`, none in the repo. The export test (`round_trip_a_simb_c:163-167`) is a **struct-field check** (`receipt_id`, `canister_id`, `record_id.len()==32`, `bls_certificate.is_some()`, `trust_root_key_id` non-empty) — exactly the "not a struct check" bar the packet forbids. The verifier round-trip is therefore **unverified and unverifiable** in this dependency set.

## Per-item results

| # | Item | Verdict | Evidence |
|---|---|---|---|
| 1 | Manifest ≡ exactly the 13, defined order; pin_number/favourite_chats/17 absent from manifest **and** projection | **PASS** | Manifest `mktd.rs:290-315` = the 13 in order; `PiiState` `mktd.rs:112-127` = same 13; `pin_number`, `favourite_chats`, and all 17 absent from both. Unit test `manifest_matches_the_ratified_13_field_projection` green. |
| 2 | Projections = actual contents, not counts/summaries | **PASS** | `PiiState` `mktd.rs:112-127`: contacts = sorted `(id bytes, nickname)` tuples (`:155-165`; `contacts.rs:16-17,65`), blocked_users = sorted id bytes (`:167-173`), achievements/external = sorted full sets (`:175-179`), message_activity = **every event + (read_up_to,last_updated)** (`message_activity_events.rs:62-64`), avatar/profile_background = full `Option<Document>`, unique_person_proof = full proof. No count/presence reduction. |
| 3 | Canonical tombstones; is_tombstoned checks all 13; pin cleared; none of the 17 written | **PASS (with mechanism note)** | `tombstone_state` `mktd.rs:325-344` clears the 13 heap fields to empty/default **+ sets `pii_tombstoned`**; the **canonical `tombstone_constant()` is emitted by the projection** (`PiiState::tombstoned()` via the `pii_tombstoned` short-circuit `:151-152, 130-146`) — i.e. the hashed post-state is all-13-constant (DaffyDefs pattern), not literal field writes. `is_tombstoned` `:347-366` checks all 13 == constant **and** heap cleared. `pin_number` cleared `:334` / checked `:356`. **None of the 17 written** (verified statically + behaviorally by Item 6). |
| 4 | Export = finalized engine receipt, refused while pending; **verifier round-trip V1–V3** | **PARTIAL — FLAG** | Export contract PASS: `mktd_get_receipt` returns `mktd02::DeletionReceipt` only if `bls_certificate.is_some()`, else `NotFinalized` (`impl/queries/mktd_get_receipt.rs:12-16`); tests `round_trip_a_simb_c` + `pending_receipt_is_not_exported_and_pin_is_destroyed` confirm. **Verifier round-trip NOT done / not possible** — see Headline #2. |
| 5 | Encoder version bump recorded in receipt/meta | **FLAG** | See Headline #1 (`mktd.rs:61,319`). Bumped but dead-bound; not recorded. |
| 6 | GC test asserts correct Phase-A (direct_chats untouched), not weakened | **PASS** | `phase_a_leaves_direct_chat_stable_memory_untouched` (`mktd_deletion_tests.rs:273-300`): sends a real DM, asserts stable-map (MemoryId 3) grew, then asserts size **unchanged** across Phase A. Real assertion, not weakened. Confirms `tombstone_state` no longer purges direct_chats (out of scope). |
| 7 | Boundary disclosure = G verbatim; refresh deviation documented, not flagged A2 | **PASS (one caveat)** | Boundary disclosure present (`MKTd02_P1_Deliverable.md:70-72`); refresh deviation documented and explicitly "**not treated as an A2 defect**" (`:79-81`). Caveat: **verbatim** match to G's exact wording is **not independently verifiable** — no canonical G source text in-repo to diff against. |

## Build + test (run by me, fresh build)
- **Native unit tests** (`cargo test -p user_canister_impl mktd`): `2 passed` — `manifest_matches_the_ratified_13_field_projection`, `all_13_projection_fields_use_the_canonical_tombstone`.
- **User wasm**: rebuilt clean from CD's tree (`cargo build --release --target wasm32-unknown-unknown -p user_canister_impl`, Finished in 1m58s); confirmed `mktd_get_receipt_msgpack` + execute/finalize endpoints present in the binary.
- **PocketIC MKTd suite** (`cargo test -p integration_tests mktd_deletion_tests`, fresh wasm, pocket-ic 11.0.0): **`13 passed; 0 failed`** in 55.9s — includes `round_trip_a_simb_c`, `pending_receipt_is_not_exported_and_pin_is_destroyed`, `phase_a_leaves_direct_chat_stable_memory_untouched`, `post_tombstone_write_traps`, double-Phase-A, A2 cases.
- **Cleanliness**: `git diff --cached` empty (nothing staged); `git diff --check` clean; no tracked source file modified by me.

> Note on the green suite: it passes, but per the packet I did not treat green as sufficient — the two FLAGS (encoder-version not recorded; no real verifier round-trip) are **invisible to the current suite** because no test asserts version recording and the export test is a struct check.

## What CD got right (matches ratified design)
The core scope change is correctly implemented: 31→13 manifest reduction, full-content projections (the previous count/summary fields like `contacts_count`/`*_present` are gone), `pin_number` destroyed-not-attested, the 17 left untouched by Phase A (verified behaviorally), and the GC test correctly re-pointed to "Phase A leaves direct-chat content untouched."

## Recommended follow-ups (not fixed — reporting only)
1. Make `PII_ENCODER_VERSION` actually recorded: include it in the `PiiState` preimage (so the hash changes across schema versions) and/or in receipt meta; remove the dead `let _schema_version`. Fix the deliverable header `…PII_V1` → `V2`.
2. Either wire a real shared CVDR-Verify (V1–V3) round-trip into the export test, or — if no such verifier exists at engine v0.4.1 — escalate that the ratified Item-4 bar is **unmeetable at this engine pin** and record that explicitly (don't leave a struct check masquerading as verifier acceptance).
3. Obtain G's canonical boundary-disclosure text to confirm verbatim equivalence (currently asserted, not checkable in-repo).

**STOP — no changes made, nothing staged or committed.**
