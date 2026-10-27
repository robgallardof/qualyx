import { z } from "zod";

const count = z.number().int().nonnegative().nullable().optional();
const localRun = z.object({ run_id: z.string(), status: z.string(), passed: count, failed: count, total: count, started_at: z.string().nullable().optional() });
const job = z.object({ run_id: z.string(), state: z.string(), manifest: z.object({ status: z.string(), counts: z.object({ passed: count, failed: count, total: count }).optional(), started_at: z.string().optional() }).nullable().optional() });
const response = z.union([z.object({ runs: z.array(localRun) }), z.object({ jobs: z.array(job) })]);
export type Run = z.infer<typeof localRun>;

/** Validate both native history and durable-team responses without inventing test counts. */
export function parseRuns(value: unknown): Run[] {
  const data = response.parse(value);
  if ("runs" in data) return data.runs;
  return data.jobs.map((job) => ({ run_id: job.run_id, status: job.manifest?.status ?? job.state,
    passed: job.manifest?.counts?.passed, failed: job.manifest?.counts?.failed, total: job.manifest?.counts?.total, started_at: job.manifest?.started_at }));
}

export function filterRuns(runs: Run[], query: string, status: string) {
  const term = query.trim().toLowerCase();
  return runs.filter((run) => (!term || run.run_id.toLowerCase().includes(term)) && (status === "all" || run.status === status));
}

export function summarize(runs: Run[]) {
  return { total: runs.length, passed: runs.filter((r) => r.status === "passed").length,
    failed: runs.filter((r) => r.status === "failed" || r.status === "error").length,
    active: runs.filter((r) => r.status === "running" || r.status === "queued").length };
}

/** Quote user-provided paths for POSIX shells and PowerShell without evaluating input. */
export function quotePath(path: string, windows: boolean) {
  return "'" + path.replaceAll("'", windows ? "''" : "'\"'\"'") + "'";
}
