# P1 — OpenChatZD User-Canister MKTd02 Integration — DELIVERABLE (inspect-and-propose)

**Mode:** inspect-and-propose only. This is a **proposed diff (uncommitted)** + artifacts.
No commit/push/branch. Claude reviews → CD independent review → human-in-the-loop commit.
**Engine pin:** `ICP-Delete-Leaf` @ `mktd02-v0.4.0` (zombie-core `zombie-core-v0.3.1`).
**Target HEAD:** `open-chatZD` @ `39b8d34af` (re-confirmed; E5 citations valid).

> ⛔ **GATING PREREQUISITE (Divergence #1 — see NOTES):** the proposed diff does **not build**
> against `mktd02-v0.4.0` as-pinned. The engine pins `ic-cdk = 0.17` / `ic-stable-structures = 0.6`;
> this workspace uses `0.18` / `0.7`. Two hard problems: (a) **dependency resolution fails** — both
> `ic-cdk` lines pull `ic-cdk-executor` with the same `links` value, which Cargo forbids in one graph;
> (b) even if it resolved, `MemoryManager<DefaultMemoryImpl>` from is-0.6 ≠ is-0.7 crosses the
> `mktd02::init` boundary. **Both are fixed only by re-pinning the engine to ic-cdk 0.18 +
> ic-stable-structures 0.7 and re-releasing (e.g. `mktd02-v0.4.1`).** The code below is correct
> *modulo that re-pin*; the dep tag must then be bumped. **No crate was compiled** (resolution aborts).

---

## 1. Proposed diff — file inventory

| File | Change | Work item |
|---|---|---|
| `Cargo.toml` (root) | add `mktd02`, `zombie-core` to `[workspace.dependencies]` | W1 |
| `backend/canisters/user/impl/Cargo.toml` | add `mktd02`, `zombie-core` deps | W1 |
| `backend/canisters/user/impl/src/memory.rs` | `with_memory_manager()` accessor (share the manager) | W6 |
| `backend/canisters/user/impl/src/mktd.rs` | **NEW** — adapter, PiiState, record_id, config | W2, W3, W6 |
| `backend/canisters/user/impl/src/lib.rs` | `mod mktd;`; `Data.pii_tombstoned`; D8 block in `execute_update`/`_async` | W2, W7 |
| `backend/canisters/user/impl/src/lifecycle/{init,post_upgrade}.rs` | `mktd02::init` / `on_post_upgrade` wiring | W1A |
| `backend/canisters/user/api/src/lifecycle/{init,post_upgrade}.rs` | additive `mktd_module_hash: Option<[u8;32]>` | W1A |
| `…/api/src/updates/mktd_execute_deletion.rs` (+impl) | **NEW** Phase A (S2) | W4 |
| `…/api/src/queries/mktd_pending_certificate.rs` (+impl) | **NEW** Phase B (S3) | W5 |
| `…/api/src/updates/mktd_finalize_deletion.rs` (+impl) | **NEW** Phase C (S4) | — |
| `…/api/src/queries/mktd_pending_deletion_state.rs` (+impl) | **NEW** pending-state (S8) | W8 |
| `…/api/src/{updates,queries}/mod.rs`, `api/src/main.rs` | module + TS-binding registration | — |

---

## 2. State Encoding Spec — `OPENCHATZD_USER_PII_V2`

**Encoder:** `zombie_core::serialisation::encode_pii_state` (ciborium + serde, struct fields in
declaration order; no maps; no floats). **Deterministic, not RFC-8949-canonical** (engine docs §11).
**Versioned:** any change to the preimage below requires a `PII_ENCODER_VERSION` bump and re-publication
this spec (historical receipts reproduce against the version recorded at issue time).

The current schema is `OPENCHATZD_USER_PII_V2`.

**Preimage struct `PiiState` (field_order = declaration order):**

The fixed leading field is `encoder_version = "OPENCHATZD_USER_PII_V2"`. It is present unchanged in
both active and tombstoned snapshots, binding both `pre_state_hash` and `post_state_hash` to this
encoding specification.

| # | Field | Deterministic active-state projection |
|---|---|---|
| 0 | `bio` | Complete string |
| 1 | `username` | Complete string |
| 2 | `display_name` | Complete optional string |
| 3 | `avatar` | Optional document ID, MIME type, and complete bytes |
| 4 | `profile_background` | Optional document ID, MIME type, and complete bytes |
| 5 | `unique_person_proof` | Optional provider and timestamp |
| 6 | `contacts` | Sorted `(user_id bytes, nickname)` tuples |
| 7 | `blocked_users` | Sorted user-ID bytes |
| 8 | `achievements` | Sorted stable enum IDs |
| 9 | `external_achievements` | Sorted complete strings |
| 10 | `message_activity_events` | Every event in stored order, plus read/update timestamps |
| 11 | `phone_is_verified` | Boolean |
| 12 | `referred_by` | Optional user-ID bytes |

Each projection field is encoded as either its complete active value or
`Tombstone(TOMBSTONE_CONSTANT)`. The post-deletion projection contains the same
32-byte tombstone constant in all 13 positions. The PIN credential is also
destroyed as a security measure, but it is not part of the attested projection.

The engine `DeletionReceipt` has no application metadata extension field, so OpenChatZD does not add
`encoder_version` to the receipt wire type. Verifiers recover it through
`receipt.module_hash -> published OpenChatZD release/build record -> this state-encoding specification`.
Publishing that release/provenance mapping is a G commit-gate carry item.

**Boundary disclosure:** CVDR covers the defined profile/identity PII projection in this user canister.
It does not attest deletion of message content, direct-chat bodies, or data held on counterparty
canisters. Native uninstall may destroy this canister's local state, but message-content deletion is
outside this receipt's scope.

Operational, financial, membership, entitlement, and message-container state is excluded from both
the manifest and adapter erase path. The adapter leaves it unchanged for OpenChat's native deletion
pipeline.

**Refresh deviation:** OpenChatZD computes fresh `get_state_bytes()` snapshots immediately before and
after tombstoning during deletion rather than continuously calling `refresh_state_hash()` after every
PII write. This is accepted for the finalized-receipt export model and is not treated as an A2 defect.

---

## 3. ADR — `record_id` derivation (S5 / D7)

**Decision:** `record_id = SHA-256( DomainTag("OPENCHATZD_RECORD_ID_USER_V1") ‖ canonical_user_id_bytes )`,
via `zombie_core::hashing::hash_with_tag(DomainTag(b"OPENCHATZD_RECORD_ID_USER_V1"), &[bytes])`,
returned as the 32-byte `Vec<u8>` passed to `execute_deletion_with_record_id`.

**Exact canonical encoding (cited):** the subject is the **OpenChat `UserId`** —
`pub struct UserId(CanisterId)` where `CanisterId = Principal`
(`backend/libraries/types/src/user.rs:10-11`). The canister obtains its own UserId via
`self.env.canister_id().into()` (`backend/canisters/user/impl/src/lib.rs:179,199`).
`canonical_user_id_bytes = Principal::as_slice(&principal)` — the IC's canonical principal byte
representation (≤29 bytes, length-determined by the principal itself). `UserId: Deref<Target=Principal>`
and `From<UserId> for CanisterId` (`user.rs:52-58, 25-29`) make this lossless and stable.

**Why this and not the alternatives:**
- **Never `caller()`** — `caller()` is the owner's *identity* principal, mutable via identity-linking;
  the UserId (the user canister's own principal) is permanent.
- **Never a raw identifier as `record_id`** — the stored value is the 32-byte hash, not the principal.
- Deterministic and collision-resistant; distinct subjects ⇒ distinct `record_id` ⇒ distinct
  `receipt_id` (engine `compute_receipt_id` test `receipt_id_is_sensitive_to_record_id`).

**Trust boundary (E4-corrected):** `record_id` **is** in the v3 `receipt_id` preimage
(length-delimited `canister_id ‖ record_id ‖ deletion_seq`), so it binds *this receipt's id*. It does
**not** reach the BLS-certified `certified_commitment` (computed from `post_state_hash ‖
deletion_event_hash` before receipt construction). A misbehaving host can mislabel *its own* receipt's
subject; it cannot forge another canister's certified state. (Do not call `record_id` "metadata only".)

---

## 4. ADR — lifecycle & module-hash pipeline (W1A)

**Engine lifecycle (confirmed against v0.4.0):** `mktd02::init(&adapter, &mm, MktdConfig{base:100},
module_hash)` from `#[init]` (after all initial PII writes); `mktd02::on_post_upgrade(&adapter, &mm,
config, module_hash)` from `#[post_upgrade]` (after state restore). Both share the host's single
`MemoryManager` (slots 100..=107). `on_post_upgrade` recomputes the state hash, re-publishes the
certified commitment, and updates `module_hash` unconditionally; **it traps if the finalization lock is
held** (finalize before upgrading) — intended.

**Module-hash decision:** a canister cannot read its own wasm hash
(`docs/sections/module-hash-pipeline.md`); it must be supplied by the installer. P1 adds an **additive
`mktd_module_hash: Option<[u8;32]>`** to the user `init`/`post_upgrade` `Args` (serde-default `None`).
- `local_user_index` already receives and retains the gzip-compressed user WASM it installs. When it
  accepts a new user WASM, it caches the SHA-256 of the gzip **upload** bytes — the exact bytes
  installed (the gzip itself, not the expanded module) — matching the IC's on-chain `module_hash`
  and what CVDR-Verify V3 reads.
- New-user install args carry that cached hash. Every user upgrade re-supplies the hash in
  `post_upgrade::Args`, so `mktd02::on_post_upgrade` records the new build's hash unconditionally.
- `None` remains accepted only for backward decoding of older init/upgrade arguments; current
  OpenChat installers pass `Some(real_module_hash)`.

---

## 5. Final MemoryId map (S6 / W6)

User canister `MEMORY_MANAGER` (single manager, shared with the engine):

| MemoryId | Owner | Content |
|---|---|---|
| 0 | host | UPGRADES (serialized `Data` at pre_upgrade) |
| 1, 2 | — | free (gaps) |
| 3 | host | STABLE_MEMORY_MAP (`chat_events` / unified keyed store) |
| 4–99 | — | free (buffer for future host growth) |
| **100** | engine | meta (schema, base, init_at, module_hash, pending_receipt_id) |
| **101** | engine | state_hash `[u8;32]` |
| **102** | engine | deletion_seq `u64` |
| **103** | engine | certified_commitment `[u8;32]` |
| **104** | engine | deletion_event_hash `[u8;32]` |
| **105** | engine | finalization_lock `bool` |
| **106** | engine | receipts `StableBTreeMap<[u8;32],Vec<u8>>` |
| **107** | engine | tombstoned_at `Option<u64>` |

`{0,3} ∩ {100..107} = ∅`. Engine slot layout confirmed in `mktd02/src/storage.rs` (base+0..base+7).
`base+7 = 107 ≤ 255` (engine range check passes). E5 verified no library allocates a MemoryId from
this manager outside `memory.rs` (`stable_memory_map` is *handed* id 3's memory; it does not allocate).

---

## 6. NOTES

### Q1 — `get_state_bytes()` feasibility (full message-body hash within one call?)
**No — and the design deliberately avoids it.** Hashing every direct-chat message body at Phase A is
unbounded in a user's message count and would risk the single-update instruction/cycle limit (the
canister's *own* GC already self-limits at `instruction_counter() < 2e9` and batches 100 keys/run —
`stable_memory_map/src/lib.rs:137-162`). Message content and direct-chat containers are therefore
explicitly outside the G-ratified CVDR boundary. The adapter neither hashes nor erases them.

### Q2 — engine confirms pre/post hashes + commitment derivation (no continuous refresh)
**Confirmed in code** (`mktd02/src/engine.rs:498-608`): `execute_deletion_with_record_id` calls
`adapter.get_state_bytes()` at line 515 (`pre_state_hash`), `adapter.tombstone_state()` at 518,
asserts `adapter.is_tombstoned()` at 521 (traps if false), `adapter.get_state_bytes()` again at 526
(`post_state_hash`); `certified_commitment = publish_certified_commitment(post_state_hash,
deletion_event_hash)` at 569, then acquires the lock (573) and persists the pending receipt id (577).
So the commitment derives from `post_state_hash + deletion_event_hash`; **no `refresh_state_hash()`
loop is needed** (V4 N/A by D3). The adapter must therefore be a *complete, deterministic, on-demand*
snapshot — which it is.

### W2 — stable-map boundary
The direct-chat stable map and its message bodies are outside the attested projection. Phase A does not
enqueue or drain message GC. Any later destruction of that state is OpenChat native-uninstall behavior,
not a property claimed by this receipt.

### W7 — D8 guard point (where post-Phase-A mutation is blocked)
**Central chokepoint, complete.** E5 + this pass verified all **51 `caller_is_owner` PII writers funnel
through `execute_update`** (0 bypass), and c2c writers through `execute_update`/`execute_update_async`
(77 call-sites). P1 adds one check — `assert_not_pending_deletion()` — at the head of **both**
helpers: if `data.pii_tombstoned`, it traps. Since Phase A sets `pii_tombstoned` synchronously, every
subsequent mutating ingress/c2c call is blocked → **no restoration of normal use (D8)**. The Phase A
wrapper drives the engine directly (not via `execute_update`), and the Phase C wrapper + S8/B queries +
P3 recovery use `mutate_state`/`read_state` directly, so none self-block. **Trade-off:** this blocks
*all* updates post-Phase-A (PII and non-PII), which is the intended D8 posture; if selective blocking
is ever wanted, the alternative is the 51-writer enumeration (more invasive, not recommended).
**On failure (B/C/storage):** the lock + `pii_tombstoned` persist → resumable/idempotent pending state;
P1 triggers no pipeline, no uninstall, no `deleteCurrentUser`; `local_user_index/.../delete_users.rs`
untouched (P3-owned).

### Double-finalize semantics (test guidance, confirmed)
A **natural** second Phase-C call returns **`NoPendingReceipt`**, not `AlreadyFinalized`: the first
success calls `release_finalization_lock()` which clears `pending_receipt_id`
(`storage.rs:1082-1096`), so `read_pending_receipt_id()` is then `None`. `AlreadyFinalized` only
arises if the lock is artificially re-acquired pointing at a finalized receipt (engine unit test
`host_auth_double_finalize_returns_already_finalized`). Tests assert the **natural** shape.

### W5 — Phase B / S8 owner-guard call
**Left unguarded (open queries).** `mktd_pending_certificate` returns only the certified commitment,
the IC-supplied BLS certificate (verification material intended for export/off-chain V2), and the
receipt id (a hash) — **no PII**. `mktd_pending_deletion_state` returns a boolean + the receipt hash
for P3 recovery. Neither leaks raw principal/record_id. If product prefers, both can take
`guard = "caller_is_owner"` trivially — but the certificate is not secret, so an open query is correct
and lets a recovery tool read state without owner credentials.

### Divergences from packet assumptions at HEAD
1. **⛔ Engine dep skew is a hard, build-stopping blocker** (resolution-level `ic-cdk-executor` `links`
   collision + `ic-stable-structures` type skew). Headline prerequisite: re-pin engine to ic-cdk 0.18 /
   ic-stable-structures 0.7 and re-release. **Until then nothing in this diff compiles.** (Packet E1
   assumed a clean pin; it is not clean for *this* workspace.)
2. **Module-hash V3:** installer wiring is complete. `local_user_index` derives the **upload** hash
   (SHA-256 of the gzip module bytes as installed) and supplies it on initial install and every
   upgrade, so `receipt.module_hash` equals the on-chain `module_hash`.
3. All other E5 citations (set_bio guard, MemoryId {0,3}, execute_update chokepoint, UserId type,
   certified_data absence, delete path) **held at HEAD** and are re-confirmed above.

---

## 7. Proposed PocketIC tests (unrun — blocked by Divergence #1)

Wire into `backend/integration_tests` (mirrors existing user-canister suites). Cases:

- **round_trip_A_simB_C** — owner calls `mktd_execute_deletion` → `Success(receipt_id)`;
  `mktd_pending_deletion_state` → `{pending:true, tombstoned:true, receipt_id:Some}`;
  `mktd_pending_certificate` (query) → `Success{receipt_id, certificate}`;
  `mktd_finalize_deletion{receipt_id, certificate}` → `Success`; pending-state then `{pending:false}`;
  engine `get_receipt(receipt_id).bls_certificate.is_some()`.
- **non_owner_rejected_A_and_C** — non-owner principal → both trap on `caller_is_owner`.
- **lock_blocks_mutation_after_A** — after Phase A, `set_bio` (any owner PII writer) traps
  (`assert_not_pending_deletion`).
- **A2_receipt_id_mismatch** — Phase C with wrong `receipt_id` → `Error("…ReceiptIdMismatch…")`,
  lock still held.
- **A2_missing_lock** — Phase C before any Phase A → `Error("…NoPendingReceipt…")`.
- **A2_natural_double_finalize** — finalize twice → second returns
  `Error("…no receipt pending finalization…")` (**NoPendingReceipt**, per semantics above).
- **record_id_shape** — derived value is stable across calls for the same subject, 32 bytes,
  ≠ `caller().as_slice()`, ≠ raw `canister_id` bytes (it's the domain-tagged hash).
- **base_memory_id_no_collision** — engine init at base 100 succeeds; host slots 0/3 intact;
  `memory_sizes()` unchanged shape.
- **pending_state_query_lifecycle** — `{pending:false}` before A; `{pending:true,id}` after A;
  `{pending:false}` after finalize.
- **D8_failure_resumable** — simulate B/C failure (bad cert id) → canister stays pending, normal use
  still blocked, no uninstall/pipeline triggered; a subsequent correct Phase C still finalizes.
