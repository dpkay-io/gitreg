#!/usr/bin/env bash
set -euo pipefail

GITREG="${GITREG:-gitreg}"

TMPDIR_ROOT=$(mktemp -d)
trap 'rm -rf "$TMPDIR_ROOT"' EXIT

export HOME="$TMPDIR_ROOT/home"
export XDG_CONFIG_HOME="$TMPDIR_ROOT/home/.config"
mkdir -p "$HOME"

REPO_A="$TMPDIR_ROOT/repo_a"
REPO_B="$TMPDIR_ROOT/repo_b"

fail() { echo "FAIL: $*" >&2; exit 1; }
ok()   { echo "  OK: $*"; }

# Create two bare git repos
git init "$REPO_A" -q
git init "$REPO_B" -q

# Hook both repos
"$GITREG" hook --path "$REPO_A"
"$GITREG" hook --path "$REPO_B"

# Verify markers exist
[[ -f "$REPO_A/.git/gitreg_tracked" ]] || fail "marker missing for repo_a"
[[ -f "$REPO_B/.git/gitreg_tracked" ]] || fail "marker missing for repo_b"
ok "markers created"

# Verify ls shows both
LS_OUT=$("$GITREG" ls)
echo "$LS_OUT" | grep -q "$REPO_A" || fail "repo_a not in ls output"
echo "$LS_OUT" | grep -q "$REPO_B" || fail "repo_b not in ls output"
ok "ls shows both repos"

# Remove repo_b from disk
rm -rf "$REPO_B"

# Prune should remove repo_b
PRUNE_OUT=$("$GITREG" prune)
echo "$PRUNE_OUT" | grep -q "$REPO_B" || fail "prune did not report repo_b"
ok "prune removed missing repo"

# Verify ls no longer shows repo_b
LS_OUT2=$("$GITREG" ls)
echo "$LS_OUT2" | grep -q "$REPO_B" && fail "repo_b still in ls after prune"
echo "$LS_OUT2" | grep -q "$REPO_A" || fail "repo_a missing after prune"
ok "ls correct after prune"

# Remove repo_a via rm command
"$GITREG" rm "$REPO_A"

# Marker must NOT be deleted — its presence prevents the shell shim from
# ever calling the hook again (shim checks: if marker content == git_root,
# needs_reg=false).  Without the marker, the next `git status` would
# re-register the repo and undo the rm.
[[ -f "$REPO_A/.git/gitreg_tracked" ]] || fail "marker missing after rm (would allow re-registration)"
ok "marker preserved by rm (prevents re-registration via shell shim)"

# ls should be empty (or show no repos)
LS_OUT3=$("$GITREG" ls)
echo "$LS_OUT3" | grep -q "$REPO_A" && fail "repo_a still in ls after rm"
ok "ls empty after rm"

echo ""
echo "ALL TESTS PASSED"
