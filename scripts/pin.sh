#!/bin/sh
# Moves every pin on kumikit/kumikit (Cargo.toml and CI) to one commit.
set -eu
rev=${1:?usage: sh scripts/pin.sh <commit>}
case $rev in *[!0-9a-f]*) echo "not a commit: $rev" >&2; exit 1 ;; esac
[ ${#rev} -eq 40 ] || { echo "give the full 40-character commit" >&2; exit 1; }
here=$(cd "$(dirname "$0")/.." && pwd)
sed -i.bak -E "s/rev = \"[0-9a-f]{40}\"/rev = \"$rev\"/g" "$here/Cargo.toml"
sed -i.bak -E "s/KUMIKIT_REV: [0-9a-f]{40}/KUMIKIT_REV: $rev/g" "$here/.github/workflows/ci.yml"
rm -f "$here/Cargo.toml.bak" "$here/.github/workflows/ci.yml.bak"
echo "pinned to $rev"
