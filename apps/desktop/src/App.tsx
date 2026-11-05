import { useEffect, useRef, useState } from "react";

import { cancelJob, onJobEvent, runSyntheticJob, type JobOutcome } from "./ipc";

// Diagnostic job: about 3 s, long enough to exercise cancellation by hand.
const DIAGNOSTIC_UNITS = 300;
const DIAGNOSTIC_DELAY_MS = 10;

type JobState =
  | { kind: "idle" }
  | { kind: "starting" }
  | { kind: "running"; jobId: number; cancelling: boolean }
  | { kind: "finished"; outcome: JobOutcome }
  | { kind: "error"; message: string };

function describe(state: JobState): string {
  switch (state.kind) {
    case "idle":
      return "Worker prêt.";
    case "starting":
      return "Démarrage de la tâche…";
    case "running":
      return state.cancelling ? "Annulation en cours…" : "Tâche en cours…";
    case "error":
      return `Erreur : ${state.message}`;
    case "finished":
      switch (state.outcome.kind) {
        case "done":
          return `Tâche terminée (${String(state.outcome.units)} unités).`;
        case "cancelled":
          return "Tâche annulée.";
        case "limitExceeded":
          return "Tâche arrêtée : limite de ressources atteinte.";
        case "busy":
          return "Worker occupé : tâche refusée.";
        case "failed":
          return `Échec du worker : ${state.outcome.reason}`;
      }
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

// Results that arrive before `worker_run` returns their job id.
const MAX_EARLY_EVENTS = 8;

export function App() {
  const [job, setJob] = useState<JobState>({ kind: "idle" });
  const [listening, setListening] = useState(false);
  // Latest job id: results of older jobs never overwrite a newer state.
  const latestJob = useRef<number | null>(null);
  // The event and the command reply travel separately: keep a bounded set of
  // results for ids not known yet instead of dropping them.
  const earlyEvents = useRef(new Map<number, JobOutcome>());

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    onJobEvent((event) => {
      const latest = latestJob.current;
      if (event.jobId === latest) {
        setJob({ kind: "finished", outcome: event.outcome });
      } else if (latest === null || event.jobId > latest) {
        const early = earlyEvents.current;
        early.set(event.jobId, event.outcome);
        for (const id of early.keys()) {
          if (early.size <= MAX_EARLY_EVENTS) {
            break;
          }
          early.delete(id);
        }
      }
    }).then(
      (stop) => {
        if (disposed) {
          stop();
        } else {
          unlisten = stop;
          setListening(true);
        }
      },
      (error: unknown) => {
        setJob({ kind: "error", message: errorMessage(error) });
      },
    );
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const start = () => {
    setJob({ kind: "starting" });
    runSyntheticJob(DIAGNOSTIC_UNITS, DIAGNOSTIC_DELAY_MS).then(
      (jobId) => {
        latestJob.current = jobId;
        const early = earlyEvents.current;
        const outcome = early.get(jobId);
        for (const id of early.keys()) {
          if (id <= jobId) {
            early.delete(id);
          }
        }
        setJob((current) => {
          if (current.kind !== "starting") {
            return current;
          }
          return outcome === undefined
            ? { kind: "running", jobId, cancelling: false }
            : { kind: "finished", outcome };
        });
      },
      (error: unknown) => {
        setJob({ kind: "error", message: errorMessage(error) });
      },
    );
  };

  const cancel = () => {
    if (job.kind !== "running") {
      return;
    }
    setJob({ ...job, cancelling: true });
    cancelJob(job.jobId).catch((error: unknown) => {
      setJob({ kind: "error", message: errorMessage(error) });
    });
  };

  const busy = job.kind === "starting" || job.kind === "running";

  return (
    <main className="app">
      <header>
        <h1>CastleEngine</h1>
      </header>
      <section aria-labelledby="document-title" className="document">
        <h2 id="document-title">Document</h2>
        <p>Aucun document ouvert.</p>
      </section>
      <section aria-labelledby="worker-title" className="worker">
        <h2 id="worker-title">Diagnostic du worker</h2>
        <p role="status" aria-live="polite">
          {describe(job)}
        </p>
        <div className="actions">
          <button type="button" onClick={start} disabled={busy || !listening}>
            Lancer une tâche de test
          </button>
          <button
            type="button"
            onClick={cancel}
            disabled={job.kind !== "running" || job.cancelling}
          >
            Annuler
          </button>
        </div>
      </section>
    </main>
  );
}
