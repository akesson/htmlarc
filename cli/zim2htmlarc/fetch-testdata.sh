#!/usr/bin/env sh
# Download a tiny (~41 KB) new-namespace-scheme ZIM for the (ignored) e2e test.
# The file is NOT committed: the openzim test suite is unlicensed, so it must be
# fetched locally. `testdata/*.zim` is gitignored.
#
# Usage:  cli/zim2htmlarc/fetch-testdata.sh
# Then:   cargo nextest run -p zim2htmlarc --run-ignored all
set -eu

# Directory this script lives in (CDPATH cleared so a relative `cd` can't wander).
SCRIPT_DIR="$(unset CDPATH; cd -- "$(dirname -- "$0")" && pwd)"
DIR="$SCRIPT_DIR/testdata"
URL="https://raw.githubusercontent.com/openzim/zim-testing-suite/main/data/nons/small.zim"

mkdir -p "$DIR"
echo "Downloading test ZIM -> $DIR/test.zim"
curl -fsSL -o "$DIR/test.zim" "$URL"
echo "Done. Run: cargo nextest run -p zim2htmlarc --run-ignored all"
