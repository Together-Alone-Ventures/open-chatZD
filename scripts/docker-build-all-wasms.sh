#!/bin/bash
# Canonical all-canister reproducible build (suite v5): the recipe a published module hash names.
# No secret or token: every git source in Cargo.lock is public since R-3. The wasms embed
# GIT_COMMIT_ID (`git rev-parse HEAD`), so a hash reproduces only at the commit it was built at.

SCRIPT=$(readlink -f "$0")
SCRIPT_DIR=$(dirname "$SCRIPT")
cd $SCRIPT_DIR/..

./scripts/check-docker-is-running.sh || exit 1

GIT_COMMIT_ID=$(git rev-parse HEAD)

echo "CommitId: $GIT_COMMIT_ID"

DOCKER_BUILDKIT=1 docker build -t openchat --build-arg git_commit_id=$GIT_COMMIT_ID --platform linux/amd64 . || exit 1

container_id=$(docker create openchat)
rm -rf wasms
docker cp $container_id:/build/wasms wasms
docker rm --volumes $container_id

cd wasms
for wasm in *; do
    sha256sum "$wasm"
done
cd ..
