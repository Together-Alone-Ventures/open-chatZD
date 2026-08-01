#!/usr/bin/env bash
# M2: two clean Docker builds of local_user_index; compare gzip wasm SHA-256.
set -euo pipefail

SCRIPT=$(readlink -f "$0")
SCRIPT_DIR=$(dirname "$SCRIPT")
cd "$SCRIPT_DIR/.."

./scripts/check-docker-is-running.sh || exit 1

GIT_COMMIT_ID=$(git rev-parse HEAD)
OUT_DIR="wasms/m2-repro"
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR/build1" "$OUT_DIR/build2"

build_once() {
  local dest=$1
  docker build \
    -t openchat-m2-lui \
    --build-arg git_commit_id="$GIT_COMMIT_ID" \
    --build-arg canister_name=local_user_index \
    --platform linux/amd64 \
    . || exit 1
  local cid
  cid=$(docker create openchat-m2-lui)
  docker cp "$cid:/build/wasms/local_user_index.wasm.gz" "$dest/local_user_index.wasm.gz"
  docker rm --volumes "$cid" >/dev/null
  sha256sum "$dest/local_user_index.wasm.gz" | tee "$dest/SHA256"
}

echo "M2 dual-build commit=$GIT_COMMIT_ID"
build_once "$OUT_DIR/build1"
build_once "$OUT_DIR/build2"

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
echo "provenance: rust 1.95.0, ic-wasm 0.9.11, docker linux/amd64, generate-wasm.sh + RUSTFLAGS remap, --locked, commit $GIT_COMMIT_ID"
