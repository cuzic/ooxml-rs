#!/usr/bin/env bash
# Dry run of the proposed Phase1 WML allowlist through the real generator (ADR-0007 §8).
#
# Usage: allowlist_dryrun.sh <base tree> <scratch dir> [--no-detach]
#   <base tree>   a checkout to copy (e.g. the impl/docx-phase0 worktree, or one with the
#                 detach/insert generator of PR #18 for the [detach] section)
#   <scratch dir> where the copy is made; nothing outside it is modified
#   --no-detach   strip the [detach] section (for generators without PR #18)
#
# Prints the generator's lowering line for every wml entry and every diagnostic that
# mentions a wml type, then whether `cargo check -p ooxml-schema --features wml` compiles.
set -euo pipefail
base=$1; scratch=$2; nodetach=${3:-}
here=$(cd "$(dirname "$0")" && pwd)
rm -rf "$scratch/tree"; mkdir -p "$scratch/tree"
(cd "$base" && tar --exclude=./target --exclude=./.git -cf - .) | (cd "$scratch/tree" && tar -xf -)
if [ "$nodetach" = "--no-detach" ]; then
  sed '/^\[detach\]/,$d' "$here/wml-phase1.toml" > "$scratch/tree/codegen/allowlist/wml.toml"
else
  cp "$here/wml-phase1.toml" "$scratch/tree/codegen/allowlist/wml.toml"
fi
cd "$scratch/tree"
export CARGO_TARGET_DIR="$scratch/target"
cargo xtask codegen 2>&1 | tail -n 3
echo "--- wml entries"
grep -E '^\s+\[wml\]' codegen/report.txt || true
echo "--- diagnostics mentioning w:"
sed -n '/^## Diagnostics/,$p' codegen/report.txt | grep -E 'w:|wml' || echo "(none)"
echo "--- cargo check"
cargo check -q -p ooxml-schema --features wml 2>&1 | tail -n 5 && echo "ooxml-schema (wml) compiles"
