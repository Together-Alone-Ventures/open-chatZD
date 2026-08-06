#!/usr/bin/env bash
# M2: two clean Docker builds of local_user_index; compare gzip wasm SHA-256.
#
# Requires Docker + BuildKit and a GitHub token with read access to private
# Together-Alone-Ventures/ICP-Delete-Leaf:
#   export GH_TOKEN=...          # or GITHUB_TOKEN, or `gh auth login`
#   ./scripts/m2-repro-local-user-index.sh
#
# Toolchain/ic-wasm layers are cached; only source+wasm is forced twice via
# distinct --build-arg build_nonce (no full --no-cache).
set -euo pipefail

SCRIPT=$(readlink -f "$0")
SCRIPT_DIR=$(dirname "$SCRIPT")
cd "$SCRIPT_DIR/.."

./scripts/check-docker-is-running.sh || exit 1

resolve_gh_token() {
  if [[ -n "${GH_TOKEN:-}" ]]; then
    printf '%s' "$GH_TOKEN"
    return 0
  fi
  if [[ -n "${GITHUB_TOKEN:-}" ]]; then
    printf '%s' "$GITHUB_TOKEN"
    return 0
  fi
  if command -v gh >/dev/null 2>&1; then
    if token=$(gh auth token 2>/dev/null) && [[ -n "$token" ]]; then
      printf '%s' "$token"
      return 0
    fi
  fi
  return 1
}

if ! GH_TOKEN="$(resolve_gh_token)"; then
  echo "M2 Docker build needs a GitHub token for private ICP-Delete-Leaf." >&2
  echo "Set GH_TOKEN / GITHUB_TOKEN, or run: gh auth login" >&2
  exit 1
fi
export GH_TOKEN

GIT_COMMIT_ID=$(git rev-parse HEAD)
OUT_DIR="wasms/m2-repro"
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR/build1" "$OUT_DIR/build2"

build_once() {
  local dest=$1
  local nonce=$2
  echo "=== M2 build nonce=$nonce → $dest ==="
  DOCKER_BUILDKIT=1 docker build \
    -t openchat-m2-lui \
    --secret id=gh_token,env=GH_TOKEN \
    --build-arg git_commit_id="$GIT_COMMIT_ID" \
    --build-arg canister_name=local_user_index \
    --build-arg build_nonce="$nonce" \
    --platform linux/amd64 \
    . || exit 1
  local cid
  cid=$(docker create openchat-m2-lui)
  docker cp "$cid:/build/wasms/local_user_index.wasm.gz" "$dest/local_user_index.wasm.gz"
  docker rm --volumes "$cid" >/dev/null
  sha256sum "$dest/local_user_index.wasm.gz" | tee "$dest/SHA256"
}

echo "M2 dual-build commit=$GIT_COMMIT_ID"
build_once "$OUT_DIR/build1" "1"
build_once "$OUT_DIR/build2" "2"

HASH1=$(awk '{print $1}' "$OUT_DIR/build1/SHA256")
HASH2=$(awk '{print $1}' "$OUT_DIR/build2/SHA256")

echo "build1=$HASH1"
echo "build2=$HASH2"

if [[ "$HASH1" != "$HASH2" ]]; then
  echo "FAIL: local_user_index.wasm.gz hashes differ"
  exit 1
fi

echo "$HASH1" > "$OUT_DIR/MATCHED_SHA256"
echo "PASS: byte-identical local_user_index.wasm.gz"
echo "provenance: rust 1.95.0, ic-wasm 0.9.11, docker linux/amd64, generate-wasm.sh + RUSTFLAGS remap, --locked, BuildKit gh_token secret, build_nonce bust, commit $GIT_COMMIT_ID"
