#!/bin/sh
# Lists or packs kumikit.schedules.
# Usage:
#   sh scripts/pack.sh --list
#   sh scripts/pack.sh <out-dir> [plugin-id ...]
set -eu

KUMIKIT_BIN=${KUMIKIT_BIN:-kumikit}

read_field() {
  python3 -c 'import sys, tomllib; print(tomllib.load(open(sys.argv[1], "rb"))[sys.argv[2]])' "$1" "$2"
}

id=$(read_field "schedules/plugin.toml" id)
ver=$(read_field "schedules/plugin.toml" version)
short=${id#kumikit.}

if [ "${1:-}" = "--list" ]; then
  printf '%s %s %s-v%s\n' "$id" "$ver" "$short" "$ver"
  exit 0
fi

OUT=${1:?usage: scripts/pack.sh <out-dir> [plugin-id ...]}
shift
WANT=" $* "

if [ "$WANT" != "  " ]; then
  case "$WANT" in
    *" $id "*) ;;
    *) exit 0 ;;
  esac
fi

wasm="target/wasm32-wasip2/release/kumikit_schedules.wasm"
if [ ! -f "$wasm" ]; then
  echo "missing $wasm — run: cargo build --target wasm32-wasip2 --release -p kumikit-schedules" >&2
  exit 1
fi

mkdir -p "$OUT"
STAGE=$(mktemp -d "${TMPDIR:-/tmp}/kk-pack.XXXXXX")
trap 'rm -rf "$STAGE"' EXIT

d="$STAGE/schedules"
mkdir -p "$d"
cp schedules/plugin.toml schedules/README.md "$d/"
cp "$wasm" "$d/plugin.wasm"
"$KUMIKIT_BIN" plugin pack "$d" -o "$OUT/$id-$ver.kkp"
