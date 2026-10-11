# syntax=docker/dockerfile:1.7
# To build run (canonical all-canister recipe — the one published module hashes name; omit
# canister_name, or pass it to build a single canister):
#   DOCKER_BUILDKIT=1 docker build --build-arg git_commit_id=$(git rev-parse HEAD) \
#     --platform linux/amd64 -t openchat .
# (Add `--build-arg canister_name=local_user_index` for a single canister.) No secret: every git
# source in Cargo.lock is public since R-3. Trees that still lock the private MKTd02 / zombie-core
# crates (the c744de1 baseline) need the gh_token-secret recipe of their own commit.
#
# M2: pass distinct build_nonce so only the post-toolchain layers rebuild.
FROM ubuntu:24.04 AS builder
SHELL ["bash", "-c"]

ARG git_commit_id
ARG rust_version=1.95.0
ARG canister_name

ENV GIT_COMMIT_ID=$git_commit_id
ENV TZ=UTC
ENV CARGO_NET_GIT_FETCH_WITH_CLI=true

RUN ln -snf /usr/share/zoneinfo/$TZ /etc/localtime && echo $TZ > /etc/timezone && \
    apt -yq update && \
    apt -yqq install --no-install-recommends curl ca-certificates build-essential git

ENV RUSTUP_HOME=/opt/rustup \
    CARGO_HOME=/opt/cargo \
    PATH=/cargo/bin:/opt/cargo/bin:$PATH

RUN curl --fail https://sh.rustup.rs -sSf \
        | sh -s -- -y --default-toolchain ${rust_version}-x86_64-unknown-linux-gnu --no-modify-path && \
    rustup default ${rust_version}-x86_64-unknown-linux-gnu && \
    rustup target add wasm32-unknown-unknown

RUN cargo install --version 0.9.11 ic-wasm

COPY . /build
WORKDIR /build

# Declare AFTER toolchain so changing nonce does not bust rustup/ic-wasm cache.
ARG build_nonce=0
RUN echo "m2 build_nonce=${build_nonce}" >/tmp/m2_build_nonce

RUN set -euo pipefail; \
    if [[ -z "${canister_name:-}" ]]; then \
      bash ./scripts/generate-all-canister-wasms.sh; \
    else \
      bash ./scripts/generate-wasm.sh "$canister_name"; \
    fi
