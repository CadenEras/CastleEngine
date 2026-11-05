import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { App } from "./App";
import type { JobEvent } from "./ipc";

// Mocks the IPC layer only: this does not prove the Tauri binary works.
const ipc = vi.hoisted(() => ({
  handler: null as ((event: JobEvent) => void) | null,
  runSyntheticJob: vi.fn<(units: number, delay: number) => Promise<number>>(),
  cancelJob: vi.fn<(jobId: number) => Promise<void>>(),
  unlisten: vi.fn(),
}));

vi.mock("./ipc", () => ({
  runSyntheticJob: ipc.runSyntheticJob,
  cancelJob: ipc.cancelJob,
  onJobEvent: (handler: (event: JobEvent) => void) => {
    ipc.handler = handler;
    return Promise.resolve(ipc.unlisten);
  },
}));

function status(): string {
  return screen.getByRole("status").textContent;
}

function emit(event: JobEvent) {
  act(() => {
    ipc.handler?.(event);
  });
}

async function startJob(jobId: number) {
  ipc.runSyntheticJob.mockResolvedValueOnce(jobId);
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Lancer une tâche de test" }));
    await Promise.resolve();
  });
}

beforeEach(() => {
  ipc.handler = null;
  vi.clearAllMocks();
  ipc.cancelJob.mockResolvedValue(undefined);
});

afterEach(() => {
  cleanup();
});

describe("App", () => {
  it("runs a job and shows its result", async () => {
    render(<App />);
    expect(status()).toBe("Worker prêt.");
    await startJob(1);
    expect(status()).toBe("Tâche en cours…");
    emit({ jobId: 1, outcome: { kind: "done", units: 300 } });
    expect(status()).toBe("Tâche terminée (300 unités).");
  });

  it("cancels the running job", async () => {
    render(<App />);
    await startJob(4);
    fireEvent.click(screen.getByRole("button", { name: "Annuler" }));
    expect(ipc.cancelJob).toHaveBeenCalledWith(4);
    expect(status()).toBe("Annulation en cours…");
    emit({ jobId: 4, outcome: { kind: "cancelled" } });
    expect(status()).toBe("Tâche annulée.");
  });

  it("ignores results of older jobs", async () => {
    render(<App />);
    await startJob(1);
    emit({ jobId: 1, outcome: { kind: "cancelled" } });
    await startJob(2);
    emit({ jobId: 1, outcome: { kind: "done", units: 3 } });
    expect(status()).toBe("Tâche en cours…");
  });

  it("reports a shell error", async () => {
    render(<App />);
    ipc.runSyntheticJob.mockRejectedValueOnce(new Error("worker unavailable"));
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Lancer une tâche de test" }));
      await Promise.resolve();
    });
    expect(status()).toBe("Erreur : worker unavailable");
  });

  it("stops listening on unmount", async () => {
    const { unmount } = render(<App />);
    await act(async () => {
      await Promise.resolve();
    });
    unmount();
    expect(ipc.unlisten).toHaveBeenCalledOnce();
  });
});
