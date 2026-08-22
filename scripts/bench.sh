#!/usr/bin/env bash
# Reproducible benchmark: `se` vs the BSD/macOS POSIX tools on a large log file.
#
# Usage: scripts/bench.sh [SIZE_GIB]   (default 1)
#
# Generates a synthetic log under $TMPDIR, then times each tool 5x and reports
# the best (min) wall-clock time. `se` is built in release first.
set -euo pipefail

SIZE_GIB="${1:-1}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
SE="$TARGET_DIR/release/se"
DATA="${TMPDIR:-/tmp}/se_bench.log"

echo ">> building se (release)"
( cd "$ROOT" && env CARGO_TARGET_DIR="$TARGET_DIR" RUSTFLAGS="${RUSTFLAGS:--C target-cpu=native}" cargo build --release >/dev/null 2>&1 )

if [[ ! -f "$DATA" ]] || [[ "$(stat -f%z "$DATA" 2>/dev/null || echo 0)" -lt $((SIZE_GIB * 1024 * 1024 * 1024)) ]]; then
  echo ">> generating ${SIZE_GIB} GiB log at $DATA"
  python3 - "$DATA" <<'PY'
import sys, random
path = sys.argv[1]
random.seed(7)
lines = []
for i in range(20000):
    pid = random.randint(1000, 9999)
    if i % 19 == 0:
        lines.append(f"2026-06-13T10:00:{i%60:02d} [pid {pid}] ERROR connection reset by peer foo baz\n")
    else:
        lines.append(f"2026-06-13T10:00:{i%60:02d} [pid {pid}] INFO request from 10.0.0.{i%255} foo=bar took {i%99}ms\n")
seed = "".join(lines).encode()
with open(path, "wb") as f:
    f.write(seed)
PY
  # Double the file until it reaches the requested size (sequential I/O, fast).
  TARGET=$((SIZE_GIB * 1024 * 1024 * 1024))
  while [[ "$(stat -f%z "$DATA")" -lt "$TARGET" ]]; do
    cat "$DATA" "$DATA" > "$DATA.tmp" && mv "$DATA.tmp" "$DATA"
  done
fi

BYTES="$(stat -f%z "$DATA")"
echo ">> file size: $(echo "scale=2; $BYTES/1024/1024/1024" | bc) GiB ($BYTES bytes)"
echo

python3 - "$SE" "$DATA" <<'PY'
import subprocess, sys, time, statistics, shutil

se, data = sys.argv[1], sys.argv[2]

def have(p): return shutil.which(p) is not None

def bench(cmd, n=5):
    ts = []
    for _ in range(n):
        t = time.perf_counter()
        subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        ts.append(time.perf_counter() - t)
    return min(ts)

groups = [
    ("substitute foo->bar (global)", [
        ("se",  [se, "s/foo/bar/g", data]),
        ("sed", ["/usr/bin/sed", "s/foo/bar/g", data]),
    ]),
    ("filter lines containing ERROR", [
        ("se",   [se, r"x/.*\n/ g/ERROR/ p", data]),
        ("grep", ["/usr/bin/grep", "ERROR", data]),
        ("awk",  ["/usr/bin/awk", "/ERROR/", data]),
    ]),
    ("exclude lines containing ERROR", [
        ("se",   [se, r"x/.*\n/ v/ERROR/ p", data]),
        ("grep", ["/usr/bin/grep", "-v", "ERROR", data]),
    ]),
    ("literal scan: locate 'ERROR'", [
        ("se",   [se, "x/ERROR/ =", data]),
        ("grep", ["/usr/bin/grep", "-bo", "ERROR", data]),
    ]),
]

for title, tools in groups:
    print(f"== {title} ==")
    base = None
    rows = []
    for name, cmd in tools:
        if not have(cmd[0]):
            continue
        secs = bench(cmd)
        rows.append((name, secs))
        if name == "se":
            base = secs
    for name, secs in rows:
        rel = ""
        if base and name != "se":
            if secs > base:
                rel = f"  (se is {secs/base:.2f}x faster)"
            else:
                rel = f"  (se is {base/secs:.2f}x slower)"
        print(f"  {name:5} {secs*1000:8.1f} ms{rel}")
    print()
PY
