import { describe, expect, it } from "vitest";

import { parseJobEvent, parseJobId } from "./ipc";

describe("IPC payload validation", () => {
  it("accepts well-formed job events", () => {
    expect(parseJobEvent({ jobId: 2, outcome: { kind: "done", units: 5 } })).toEqual({
      jobId: 2,
      outcome: { kind: "done", units: 5 },
    });
    expect(parseJobEvent({ jobId: 3, outcome: { kind: "failed", reason: "x" } })).toEqual({
      jobId: 3,
      outcome: { kind: "failed", reason: "x" },
    });
  });

  it("rejects malformed job events", () => {
    for (const payload of [
      null,
      "done",
      { jobId: -1, outcome: { kind: "cancelled" } },
      { jobId: 1.5, outcome: { kind: "cancelled" } },
      { jobId: 1, outcome: { kind: "done" } },
      { jobId: 1, outcome: { kind: "done", units: Number.MAX_SAFE_INTEGER + 1 } },
      { jobId: 1, outcome: { kind: "failed", reason: 4 } },
      { jobId: 1, outcome: { kind: "unknown" } },
    ]) {
      expect(parseJobEvent(payload)).toBeNull();
    }
  });

  it("validates job ids returned by the shell", () => {
    expect(parseJobId(7)).toBe(7);
    expect(() => parseJobId("7")).toThrow();
    expect(() => parseJobId(-1)).toThrow();
  });
});
