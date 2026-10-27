import { test } from "node:test";
import assert from "node:assert/strict";
import { filterRuns, parseRuns, quotePath, summarize } from "../src/lib/runs.ts";

const local = {
  in_flight: [],
  runs: [
    { run_id: "20261027T1-a", status: "passed", passed: 3, failed: 0, total: 3, started_at: "2026-10-27T18:00:00Z" },
    { run_id: "20261027T2-b", status: "failed", passed: 1, failed: 1, total: 2, started_at: null },
    { run_id: "20261027T3-c", status: "error", passed: null, failed: null, total: null },
  ],
};

test("parses native history as returned by the API", () => {
  const runs = parseRuns(local);
  assert.equal(runs.length, 3);
  assert.equal(runs[1].failed, 1);
  assert.equal(runs[2].total, null);
});

test("maps team jobs without inventing counts", () => {
  const runs = parseRuns({ jobs: [
    { run_id: "j1", state: "queued", manifest: null },
    { run_id: "j2", state: "finished", manifest: { status: "passed", counts: { passed: 4, failed: 0, total: 4 }, started_at: "2026-10-27T18:00:00Z" } },
  ] });
  assert.deepEqual(runs[0], { run_id: "j1", status: "queued", passed: undefined, failed: undefined, total: undefined, started_at: undefined });
  assert.equal(runs[1].status, "passed");
  assert.equal(runs[1].total, 4);
});

test("rejects incompatible payloads instead of rendering garbage", () => {
  assert.throws(() => parseRuns({ error: "missing or invalid bearer token" }));
  assert.throws(() => parseRuns({ runs: [{ run_id: 1, status: "passed" }] }));
  assert.throws(() => parseRuns({ runs: [{ run_id: "x", status: "passed", total: -1 }] }));
});

test("filters by id and status, and summarizes by outcome", () => {
  const runs = parseRuns(local);
  assert.deepEqual(filterRuns(runs, " 2-B ", "all").map((r) => r.run_id), ["20261027T2-b"]);
  assert.deepEqual(filterRuns(runs, "", "passed").map((r) => r.run_id), ["20261027T1-a"]);
  assert.equal(filterRuns(runs, "nope", "all").length, 0);
  assert.deepEqual(summarize(runs), { total: 3, passed: 1, failed: 2, active: 0 });
});

test("quotes paths for POSIX shells and PowerShell", () => {
  assert.equal(quotePath("/work/it's here", false), `'/work/it'"'"'s here'`);
  assert.equal(quotePath("C:/it's here", true), "'C:/it''s here'");
  assert.equal(quotePath("$(rm -rf ~)", false), "'$(rm -rf ~)'");
});
