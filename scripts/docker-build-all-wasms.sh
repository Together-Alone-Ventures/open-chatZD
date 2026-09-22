#!/bin/bash
# Canonical all-canister reproducible build (suite v5): the recipe a published module hash names.
# The Dockerfile hard-fails on a missing/empty gh_token secret (private git deps), so the secret is
# always passed — by file path (GH_TOKEN_FILE, default ~/.config/tav/gh_token, mode 600) or, when
# that file is absent and GH_TOKEN is set, by environment. The token is read inside BuildKit only
# and is never printed.

SCRIPT=$(readlink -f "$0")
SCRIPT_DIR=$(dirname "$SCRIPT")
cd $SCRIPT_DIR/..

./scripts/check-docker-is-running.sh || exit 1

GIT_COMMIT_ID=$(git rev-parse HEAD)

echo "CommitId: $GIT_COMMIT_ID"

GH_TOKEN_FILE="${GH_TOKEN_FILE:-$HOME/.config/tav/gh_token}"
if [ -s "$GH_TOKEN_FILE" ]; then
    SECRET="id=gh_token,src=$GH_TOKEN_FILE"
elif [ -n "${GH_TOKEN:-}" ]; then
    SECRET="id=gh_token,env=GH_TOKEN"
else
    echo "gh_token secret required: put a GitHub read token in $GH_TOKEN_FILE (mode 600) or export GH_TOKEN" >&2
    exit 1
fi

DOCKER_BUILDKIT=1 docker build -t openchat --secret "$SECRET" --build-arg git_commit_id=$GIT_COMMIT_ID --platform linux/amd64 . || exit 1

container_id=$(docker create openchat)
rm -rf wasms
docker cp $container_id:/build/wasms wasms
docker rm --volumes $container_id

cd wasms
for wasm in *; do
    sha256sum "$wasm"
done
cd ..
