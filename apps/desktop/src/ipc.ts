// Typed boundary with the Rust shell. Payloads are `unknown` until validated:
// a well-typed call here does not make the data trustworthy.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type JobOutcome =
  | { kind: "done"; units: number }
  | { kind: "cancelled" }
  | { kind: "limitExceeded" }
  | { kind: "busy" }
  | { kind: "failed"; reason: string };

export interface JobEvent {
  jobId: number;
  outcome: JobOutcome;
}

export const JOB_EVENT = "worker-job";

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isCount(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

export function parseJobId(value: unknown): number {
  if (!isCount(value)) {
    throw new Error("invalid job id from shell");
  }
  return value;
}

function parseOutcome(value: unknown): JobOutcome | null {
  if (!isRecord(value)) {
    return null;
  }
  switch (value.kind) {
    case "done":
      return isCount(value.units) ? { kind: "done", units: value.units } : null;
    case "cancelled":
    case "limitExceeded":
    case "busy":
      return { kind: value.kind };
    case "failed":
      return typeof value.reason === "string"
        ? { kind: "failed", reason: value.reason }
        : null;
    default:
      return null;
  }
}

export function parseJobEvent(value: unknown): JobEvent | null {
  if (!isRecord(value) || !isCount(value.jobId)) {
    return null;
  }
  const outcome = parseOutcome(value.outcome);
  return outcome === null ? null : { jobId: value.jobId, outcome };
}

/** Turns a rejected command (a serialized Rust `CommandError`) into a message. */
export function describeCommandError(error: unknown): string {
  if (isRecord(error)) {
    if (error.code === "workerUnavailable") {
      return "worker indisponible";
    }
    if (error.code === "invalidArgument" && typeof error.field === "string") {
      return `argument invalide (${error.field})`;
    }
  }
  if (error instanceof Error) {
    return error.message;
  }
  return "erreur inattendue du shell";
}

async function command(name: string, args: Record<string, number>): Promise<unknown> {
  try {
    return await invoke<unknown>(name, args);
  } catch (error: unknown) {
    throw new Error(describeCommandError(error), { cause: error });
  }
}

export async function runSyntheticJob(
  units: number,
  unitDelayMs: number,
): Promise<number> {
  return parseJobId(await command("worker_run", { units, unitDelayMs }));
}

export async function cancelJob(jobId: number): Promise<void> {
  await command("worker_cancel", { jobId });
}

/** Malformed payloads are dropped rather than shown. */
export function onJobEvent(handler: (event: JobEvent) => void): Promise<UnlistenFn> {
  return listen<unknown>(JOB_EVENT, ({ payload }) => {
    const event = parseJobEvent(payload);
    if (event !== null) {
      handler(event);
    }
  });
}
