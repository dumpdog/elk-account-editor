#!/bin/bash
# Updates the Rust editor from git and rebuilds it (the "plain git" way; the in-app "Check for updates" needs a public release instead).
#   rust/packaging/update-from-git.sh [branch] [--no-build] [--run]
# Needs git and the Rust toolchain (rustup). It never touches your accounts library: a build from git keeps its data in
# "<repo>/ElkEditor data" when started with --run (the folder next to a cargo build output would be deleted by `cargo clean`).
set -euo pipefail
BRANCH="master"; BUILD=1; RUN=0
for a in "$@"; do
  case "$a" in
    --no-build) BUILD=0 ;;
    --run) RUN=1 ;;
    *) BRANCH="$a" ;;
  esac
done
REPO="$(cd "$(dirname "$0")/../.." && pwd)"
export PATH="$HOME/.cargo/bin:$PATH"
cd "$REPO"
if [ -n "$(git status --porcelain --untracked-files=no)" ]; then
  echo "There are uncommitted changes in $REPO. Commit or stash them first (nothing was changed)." >&2
  exit 1
fi
git fetch origin
git checkout "$BRANCH"
git pull --ff-only origin "$BRANCH"
if [ "$BUILD" = 0 ]; then echo "Pulled. Skipped the build."; exit 0; fi
(cd rust/app && cargo build --release)
(cd rust/cli && cargo build --release)
BIN="$REPO/rust/app/target/release/elkaccounteditor"
echo "Built: $BIN"
if [ "$RUN" = 1 ]; then
  DATA="${ELKEDITOR_DATA:-$REPO/ElkEditor data}"
  "$BIN" --data-dir "$DATA" &
fi
