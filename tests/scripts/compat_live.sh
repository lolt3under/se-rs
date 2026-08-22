#!/usr/bin/env bash
# Live compatibility harness: run common grep / ripgrep invocations against the
# equivalent `se` program on the same input and diff the output byte-for-byte.
#
# Unlike tests/compatibility.rs (which pins expected values), this executes the
# REAL tools so drift in either direction is caught. grep = /usr/bin/grep,
# rg = a real ripgrep binary (installed via brew), bypassing any shell wrappers.
#
# Usage: tests/scripts/compat_live.sh
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
SE="$TARGET_DIR/release/se"
GREP="/usr/bin/grep"
RG="$(command -v rg || true)"

if [[ -z "$RG" ]]; then
  printf 'ripgrep is required for the live compatibility harness\n' >&2
  exit 2
fi
if [[ ! -x "$SE" ]]; then
  (cd "$ROOT" && env CARGO_TARGET_DIR="$TARGET_DIR" cargo build --release) || exit 2
fi

pass=0; fail=0
FAILURES=()
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# corpus: mixed case, numbers, IPs, blank lines, punctuation, unicode, tabs
CORPUS="$WORK/corpus"
printf '%s\n' \
'the quick brown fox' \
'The Quick Brown Fox' \
'ERROR: disk full' \
'error: retry' \
'warn: low memory' \
'192.168.1.1 gateway' \
'10.0.0.255 host' \
'foo=bar baz=qux' \
'' \
'cat scattered concatenate' \
'a1b2c3 d4e5f6' \
'UPPER lower MiXeD' \
'  leading spaces' \
'trailing spaces   ' \
'tab	separated	values' \
'café résumé naïve' \
'λ lambda μ mu' \
'end.' > "$CORPUS"

# Run one reference command and the equivalent se program, then compare bytes.
check() {
  local name="$1"; shift
  local se_prog="$1"; shift
  local ref_out="$WORK/ref.$pass.$fail"
  local se_out="$WORK/se.$pass.$fail"
  "$@" < "$CORPUS" > "$ref_out" 2>/dev/null
  "$SE" "$se_prog" < "$CORPUS" > "$se_out" 2>/dev/null
  if cmp -s "$ref_out" "$se_out"; then
    pass=$((pass+1))
  else
    fail=$((fail+1))
    FAILURES+=("$name")
    printf 'FAIL %s\n' "$name"
    diff -u "$ref_out" "$se_out" | sed -n '1,20p'
  fi
}

echo "=== se ($($SE --version 2>/dev/null || echo '?')) vs grep ($($GREP --version|head -1)) / rg ($($RG --version|head -1)) ==="

# ---- grep parity (literal + regex line matching) ----
check "grep literal"          'x/.*\n?/ g/error/ p'        "$GREP" error
check "grep -i"               'x/.*\n?/ g/error/i p'       "$GREP" -i error
check "grep -v"               'x/.*\n?/ v/error/ p'        "$GREP" -v error
check "grep -E alt"           'x/.*\n?/ g/foo|bar/ p'      "$GREP" -E 'foo|bar'
check "grep -E anchored"      'x/.*\n?/ g/^error/ p'       "$GREP" -E '^error'
check "grep -E end anchor"    'x/.*\n?/ g/full$/ p'        "$GREP" -E 'full$'
check "grep -E class"         'x/.*\n?/ g/[0-9]+/ p'       "$GREP" -E '[0-9]+'
check "grep -w word"          'x/.*\n?/ g/\bcat\b/ p'      "$GREP" -w cat
check "grep -E dot"           'x/.*\n?/ g/a.c/ p'          "$GREP" -E 'a.c'
check "grep -Eo only-match"   'x/[0-9]+/ p'                "$GREP" -Eo '[0-9]+'
check "grep -E ip"            'x/.*\n?/ g/[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+/ p' "$GREP" -E '[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+'

# ---- ripgrep parity ----
check "rg literal"            'x/.*\n?/ g/error/ p'        "$RG" -N error
check "rg -i"                 'x/.*\n?/ g/error/i p'       "$RG" -N -i error
check "rg -v"                 'x/.*\n?/ v/error/ p'        "$RG" -N -v error
check "rg alt"                'x/.*\n?/ g/foo|bar/ p'      "$RG" -N 'foo|bar'
check "rg -o only-match"      'x/[0-9]+/ p'                "$RG" -N -o '[0-9]+'
check "rg -w word"            'x/.*\n?/ g/\bcat\b/ p'      "$RG" -N -w cat
check "rg unicode \\w"        'x/\w+/ p'                   "$RG" -N -o '\w+'
check "rg anchored"           'x/.*\n?/ g/^error/ p'       "$RG" -N '^error'
check "rg digit class"        'x/.*\n?/ g/\d+/ p'          "$RG" -N '\d+'
check "rg case fold unicode"  'x/.*\n?/ g/CAFÉ/i p'        "$RG" -N -i 'CAFÉ'

echo
echo "=== $pass passed, $fail failed ==="
if [[ $fail -gt 0 ]]; then
  printf 'failing: %s\n' "${FAILURES[*]}"
  exit 1
fi
