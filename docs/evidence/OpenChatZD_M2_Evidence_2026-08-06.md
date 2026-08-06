# OpenChatZD — M2 Evidence (reproducible local_user_index)

**Milestone:** M2 — Reproducible build  
**Branch:** `antek`  
**Commit (artefact):** `5b2077c7ee77f8abacc74e1e663b4b45fbd19992`  
**Date:** 2026-08-06 (refresh after Stef A/B/C delta; prior pass 2026-08-01 @ `1dce89a6…`)  
**Status:** **PASS** — same-window Docker dual-build byte-identical (`linux/amd64`, BuildKit `gh_token`).

## Claim scope (restated)

M2 shows **same-window dual-build identity**: two Docker builds of the same recipe on the same
machine/day produced identical `local_user_index.wasm.gz` bytes.

This is **not** a full network-hermetic / bit-for-bit forever claim. The Dockerfile still pulls
mutable `ubuntu:24.04`, apt packages, a rustup installer, `cargo install ic-wasm`, and git
dependencies. Two builds at different times could legitimately diverge if those inputs move.

## Matched artefact

| Field | Value |
|-------|--------|
| File | `wasms/m2-repro/build{1,2}/local_user_index.wasm.gz` |
| SHA-256 | `4d4a4128b0e4719dbcfcb4d635c0cd30e6468fc85a6be8b8bb5907d3dc6cce4b` |
| Method | Two Docker builds (`build_nonce` 1 then 2); compare gzip SHA-256 |
| Log | `/tmp/m2-dual-2026-08-06.log` |

## Docker dual-build (2026-08-06)

- Tip: `5b2077c7ee77f8abacc74e1e663b4b45fbd19992`
- `build1` == `build2` → **PASS**
- Provenance: rust 1.95.0, ic-wasm 0.9.11, docker linux/amd64, `--locked`, BuildKit `gh_token`

## Exit gate

Same-window Docker dual-build byte-identical: **met** (refreshed post Stef delta).  
Network-hermetic / calendar-stable Module Hash: **not claimed**.
