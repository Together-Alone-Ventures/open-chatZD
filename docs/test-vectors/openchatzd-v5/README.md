# OpenChatZD suite-v5 test-vector corpus

Generated, hash-gated vectors for the suite-v5 CVDR receipt path (Brief B1 §2 step 6; plan Step 9).
Every file is regenerated from the **live** formulas and wire encoders by
`backend/canisters/local_user_index/impl/src/model/cvdr_vectors.rs`
(`corpus_matches_committed_files_byte_for_byte`) and must match the committed bytes;
`manifest.json` pins each file's SHA-256. A diff is a formula or encoding change and must be
ruled, never absorbed. Rewrite with `OPENCHATZD_WRITE_VECTORS=1 cargo test -p local_user_index_canister_impl cvdr_vectors`.

Fixed inputs (documented inside each vector): `record_salt = C3×32`, `nonce = 02×32`,
`targets_salt = 04×32`, `module_hash_pre = 09×32`, `user = Principal(02×10)`,
`index = Principal(03×10)`, `deletion_seq = 5`, targets `Principal(08×10), Principal(06×10)`,
`uninstall_completed_at = 111 ns`, `receipt_committed_at = 222 ns`.

| Vector | Kind | Pins |
|---|---|---|
| `pv5-001-receipt_id` | positive | `receipt_id = SHA256("OPENCHATZD_CVDR_RECEIPT_V1" ‖ record_id ‖ seq(u64 BE) ‖ nonce)` |
| `pv5-002-record_id_v2` | positive | `record_id = SHA256("OPENCHATZD_RECORD_ID_USER_V2" ‖ record_salt ‖ UserId bytes)` (R-1) and `≠` the retired V1 derivation |
| `pv5-003-receipt_body_v2` | positive | `RECEIPT_BODY_V2` exact layout (236 bytes for 10-byte principals) + `OPENCHATZD_RECEIPT_LEAF_V1` leaf |
| `pv5-004-canonical_package` | positive | canonical FrozenWire / PortablePackageV3 / RevealWire v2 JSON bytes and their SHA-256 (encoding only — placeholder certificate) |
| `nv5-001-old_tag_and_prefix_dispatch` | negative | V1 tag under a V3 package (`v1:version-tag-mismatch`), V2 tag over the V1 layout and a future tag (`v1:body-malformed`), prefix-matching version/schema (malformed) |
| `nv5-002-altered_receipt_id` | negative | receipt_id / nonce byte flipped → `v1:receipt-id` |
| `nv5-003-identifying_record_id` | negative | V2 body carrying the retired UserId-only `record_id` → `V1 reveal: record_id_v2` FAIL |
| `nv5-004-module_hash_in_preimage_or_mismatched_evidence` | negative | h_index/commitment in the body preimage under V3 → `v1:version-tag-mismatch`; displayed `index_module_hash` ≠ certified → `INDEX_HASH_MISMATCH` (the certificate case runs over the PocketIC fixture in CVDR-Verify `tests/openchatzd_v3_e2e.rs`) |
| `nv5-005-altered_body_field` | negative | `h_user_pre` byte flipped (receipt_id unaffected) → `V1 leaf == receipt_hash` FAIL |

Consumer: CVDR-Verify `mktd02/mktd02-verify/tests/openchatzd_v5_corpus.rs` drives a byte-identical
mirror (`tests/fixtures/v5-openchatzd/corpus/`) through the real CLI; the LUI test
`corpus_mirror_in_cvdr_verify_is_byte_identical` fails when a sibling checkout's mirror drifts.
