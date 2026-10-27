#!/usr/bin/env bash
# M3 benchmark: full suite vs impact-selected runs on a clean clone of the Qualyx repo.
set -u
S="${QUALYX_BENCH_DIR:-$(mktemp -d)}"
Q="${QUALYX_BIN:-$(pwd)/target/release/qualyx}"
B="$S/bench"
OUT="$S/bench-results.tsv"
rm -rf "$B" && git clone -q "${QUALYX_REPO:-$(pwd)}" "$B"
cd "$B" || exit 1
printf 'scenario\trep\tseconds\texit\tran\ttests\tlog_bytes\tsummary_bytes\tselection\n' > "$OUT"

measure() { # scenario rep [extra args]
  local name=$1 rep=$2; shift 2
  local t0=$(date +%s.%N)
  "$Q" test "$B" "$@" > /dev/null 2>&1
  local code=$?
  local t1=$(date +%s.%N)
  local secs=$(python -c "print(round($t1-$t0,1))")
  "$Q" report "$B" --format json > "$S/bench-last.json"
  python -I - "$S/bench-last.json" "$B" "$name" "$rep" "$secs" "$code" >> "$OUT" <<'PY'
import json, os, sys
m = json.load(open(sys.argv[1], encoding="utf-8"))
b, name, rep, secs, code = sys.argv[2:7]
run_dir = os.path.join(b, ".qualyx", "runs", m["run_id"])
log = sum(os.path.getsize(os.path.join(run_dir, a)) for r in m["runners"] for a in r["artifacts"] if os.path.exists(os.path.join(run_dir, a)))
summary = len(json.dumps({"status": m["status"], "counts": m["counts"], "failures": [t["name"] for r in m["runners"] for t in r["tests"] if t["status"] == "failed"]}))
ran = ",".join(r["ecosystem"] for r in m["runners"]) or "-"
print("\t".join([name, rep, secs, code, ran, str(m["counts"]["total"]), str(log), str(summary), (m.get("selection") or "full").replace("\t", " ")]))
PY
}

# Warm-up: compile everything once so timings compare test work, not cold builds.
"$Q" test "$B" > /dev/null 2>&1

for rep in 1 2; do measure full "$rep"; done

scenario() { # name file
  local name=$1 file=$2
  printf '\n// bench edit\n' >> "$file"
  "$Q" impact "$B" --base HEAD --json > "$S/bench-impact-$name.json"
  for rep in 1 2; do measure "$name" "$rep" --changed-since HEAD; done
  git checkout -q -- "$file"
}

scenario leaf-llm crates/qualyx-llm/src/lib.rs
scenario core-store crates/qualyx-store/src/lib.rs
scenario fixture-py fixtures/python/failing/tests/test_calculator.py
printf '\nnote\n' >> README.md
for rep in 1 2; do measure docs-only "$rep" --changed-since HEAD; done
git checkout -q -- README.md
echo DONE
