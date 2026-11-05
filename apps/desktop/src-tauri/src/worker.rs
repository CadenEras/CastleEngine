//! Owns the `pdf-worker` process for the shell. Tauri commands only queue
//! validated requests; a manager thread forwards them, polls responses and
//! publishes job events, so no command ever waits on the worker.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use pdf_worker::client::{ClientError, WorkerProcess};
use pdf_worker::protocol::{ErrorCode, Request, Response};
use serde::Serialize;

pub const MAX_UNITS: u64 = 10_000_000;
pub const MAX_UNIT_DELAY_MS: u8 = 100;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);
/// Upper bound on the delay before a worker response reaches the UI.
const POLL_INTERVAL: Duration = Duration::from_millis(20);
/// Jobs awaiting a final response before new ones are refused as `Busy`.
const MAX_PENDING: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum JobOutcome {
    Done { units: u64 },
    Cancelled,
    LimitExceeded,
    Busy,
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobEvent {
    pub job_id: u64,
    pub outcome: JobOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "code", rename_all = "camelCase")]
pub enum CommandError {
    InvalidArgument { field: &'static str },
    WorkerUnavailable,
}

enum Command {
    Run {
        id: u64,
        units: u64,
        unit_delay_ms: u8,
    },
    Cancel {
        id: u64,
    },
    Stop,
}

pub struct WorkerHandle {
    /// Last issued job id; the lock also keeps ids and sends in the same order.
    commands: Mutex<(u64, Sender<Command>)>,
    manager: Mutex<Option<JoinHandle<()>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The worker is expected next to the shell executable.
pub fn bundled_worker_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.with_file_name(format!("pdf-worker{}", std::env::consts::EXE_SUFFIX)))
}

pub fn validate_run(units: u64, unit_delay_ms: u8) -> Result<(), CommandError> {
    if units == 0 || units > MAX_UNITS {
        return Err(CommandError::InvalidArgument { field: "units" });
    }
    if unit_delay_ms > MAX_UNIT_DELAY_MS {
        return Err(CommandError::InvalidArgument {
            field: "unitDelayMs",
        });
    }
    Ok(())
}

impl WorkerHandle {
    pub fn start<F>(worker: Option<PathBuf>, publish: F) -> Self
    where
        F: Fn(JobEvent) + Send + 'static,
    {
        let (tx, rx) = channel();
        let manager = thread::Builder::new()
            .name("pdf-worker-manager".into())
            .spawn(move || manage(worker.as_deref(), &rx, &publish));
        if let Err(error) = &manager {
            eprintln!("castle-engine: cannot start worker manager: {error}");
        }
        Self {
            commands: Mutex::new((0, tx)),
            manager: Mutex::new(manager.ok()),
        }
    }

    pub fn run(&self, units: u64, unit_delay_ms: u8) -> Result<u64, CommandError> {
        validate_run(units, unit_delay_ms)?;
        let mut commands = lock(&self.commands);
        let id = commands
            .0
            .checked_add(1)
            .ok_or(CommandError::WorkerUnavailable)?;
        commands
            .1
            .send(Command::Run {
                id,
                units,
                unit_delay_ms,
            })
            .map_err(|_| CommandError::WorkerUnavailable)?;
        commands.0 = id;
        Ok(id)
    }

    pub fn cancel(&self, id: u64) -> Result<(), CommandError> {
        lock(&self.commands)
            .1
            .send(Command::Cancel { id })
            .map_err(|_| CommandError::WorkerUnavailable)
    }

    /// Stops the worker; returns after at most the shutdown grace period
    /// plus one poll interval.
    pub fn stop(&self) {
        let _ = lock(&self.commands).1.send(Command::Stop);
        if let Some(manager) = lock(&self.manager).take() {
            let _ = manager.join();
        }
    }
}

fn outcome(code: ErrorCode) -> JobOutcome {
    match code {
        ErrorCode::Cancelled => JobOutcome::Cancelled,
        ErrorCode::LimitExceeded => JobOutcome::LimitExceeded,
        ErrorCode::Busy => JobOutcome::Busy,
        other => JobOutcome::Failed {
            reason: format!("{other:?}"),
        },
    }
}

fn failed(job_id: u64, reason: &str) -> JobEvent {
    JobEvent {
        job_id,
        outcome: JobOutcome::Failed {
            reason: reason.to_owned(),
        },
    }
}

/// Jobs sent without a final response yet, reported as failed if the worker
/// dies. Bounded: once full, new jobs are answered `Busy` without being sent.
#[derive(Debug, Default)]
struct PendingJobs(BTreeSet<u64>);

impl PendingJobs {
    fn is_full(&self) -> bool {
        self.0.len() >= MAX_PENDING
    }

    fn track(&mut self, id: u64) {
        self.0.insert(id);
    }

    fn settle(&mut self, id: u64) {
        self.0.remove(&id);
    }

    fn drain(&mut self) -> impl Iterator<Item = u64> + use<> {
        std::mem::take(&mut self.0).into_iter()
    }
}

fn manage(path: Option<&Path>, commands: &Receiver<Command>, publish: &dyn Fn(JobEvent)) {
    let mut worker = match path.map(|p| WorkerProcess::spawn(p, HANDSHAKE_TIMEOUT)) {
        Some(Ok(worker)) => Some(worker),
        Some(Err(error)) => {
            eprintln!("castle-engine: worker unavailable: {error}");
            None
        }
        None => {
            eprintln!("castle-engine: worker path unknown");
            None
        }
    };
    let mut pending = PendingJobs::default();

    loop {
        match commands.recv_timeout(POLL_INTERVAL) {
            Ok(Command::Stop) | Err(RecvTimeoutError::Disconnected) => break,
            Ok(Command::Run {
                id,
                units,
                unit_delay_ms,
            }) => {
                let request = Request::RunSynthetic {
                    id,
                    units,
                    unit_delay_ms,
                };
                let sent = if pending.is_full() {
                    Some(Err(ClientError::Backpressure))
                } else {
                    worker.as_ref().map(|w| w.send(&request))
                };
                match sent {
                    Some(Ok(())) => {
                        pending.track(id);
                    }
                    Some(Err(ClientError::Backpressure)) => publish(JobEvent {
                        job_id: id,
                        outcome: JobOutcome::Busy,
                    }),
                    _ => publish(failed(id, "worker unavailable")),
                }
            }
            Ok(Command::Cancel { id }) => {
                if let Some(worker) = &worker {
                    let _ = worker.send(&Request::Cancel { id });
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
        }

        let Some(current) = &worker else { continue };
        loop {
            let (job_id, outcome) = match current.recv_timeout(Duration::ZERO) {
                Ok(Response::JobDone { id, units }) => (id, JobOutcome::Done { units }),
                Ok(Response::JobFailed { id, code }) => (id, outcome(code)),
                Err(ClientError::Timeout) => break,
                Ok(other) => {
                    eprintln!("castle-engine: unexpected worker response: {other:?}");
                    worker = None;
                    break;
                }
                Err(error) => {
                    eprintln!("castle-engine: worker lost: {error}");
                    worker = None;
                    break;
                }
            };
            pending.settle(job_id);
            publish(JobEvent { job_id, outcome });
        }
        if worker.is_none() {
            for job_id in pending.drain() {
                publish(failed(job_id, "worker exited"));
            }
        }
    }

    if let Some(worker) = worker {
        let _ = worker.shutdown(SHUTDOWN_GRACE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Instant;

    const TIMEOUT: Duration = Duration::from_secs(10);

    /// `cargo test --workspace` builds `pdf-worker` into the same target dir.
    fn test_worker_path() -> PathBuf {
        let exe = std::env::current_exe().unwrap();
        let dir = exe.parent().and_then(Path::parent).unwrap();
        let path = dir.join(format!("pdf-worker{}", std::env::consts::EXE_SUFFIX));
        assert!(path.exists(), "build pdf-worker first: {}", path.display());
        path
    }

    fn start() -> (WorkerHandle, mpsc::Receiver<JobEvent>) {
        let (tx, rx) = mpsc::channel();
        let handle = WorkerHandle::start(Some(test_worker_path()), move |event| {
            let _ = tx.send(event);
        });
        (handle, rx)
    }

    #[test]
    fn arguments_are_validated_before_reaching_the_worker() {
        assert_eq!(validate_run(1, 0), Ok(()));
        assert_eq!(validate_run(MAX_UNITS, MAX_UNIT_DELAY_MS), Ok(()));
        for (units, delay, field) in [
            (0, 0, "units"),
            (MAX_UNITS + 1, 0, "units"),
            (1, MAX_UNIT_DELAY_MS + 1, "unitDelayMs"),
        ] {
            assert_eq!(
                validate_run(units, delay),
                Err(CommandError::InvalidArgument { field })
            );
        }
    }

    #[test]
    fn job_runs_and_reports_through_the_real_worker() {
        let (handle, events) = start();
        let id = handle.run(5, 0).unwrap();
        assert_eq!(
            events.recv_timeout(TIMEOUT),
            Ok(JobEvent {
                job_id: id,
                outcome: JobOutcome::Done { units: 5 }
            })
        );
        handle.stop();
    }

    #[test]
    fn job_is_cancelled_and_stop_is_bounded() {
        let (handle, events) = start();
        let id = handle.run(10_000, 10).unwrap();
        handle.cancel(id).unwrap();
        assert_eq!(
            events.recv_timeout(TIMEOUT),
            Ok(JobEvent {
                job_id: id,
                outcome: JobOutcome::Cancelled
            })
        );
        let second = handle.run(10_000, 10).unwrap();
        assert!(second > id);
        let start = Instant::now();
        handle.stop();
        assert!(start.elapsed() < SHUTDOWN_GRACE + Duration::from_secs(1));
    }

    #[test]
    fn pending_jobs_track_every_job_until_settled() {
        let mut pending = PendingJobs::default();
        pending.track(1);
        pending.track(2);
        pending.settle(1);
        pending.track(3);
        assert_eq!(pending.drain().collect::<Vec<_>>(), vec![2, 3]);
        assert_eq!(pending.drain().count(), 0);
        for id in 0..u64::try_from(MAX_PENDING).unwrap() {
            assert!(!pending.is_full());
            pending.track(id);
        }
        assert!(pending.is_full());
    }

    #[test]
    fn overlapping_jobs_each_get_one_final_event() {
        let (handle, events) = start();
        let first = handle.run(10_000, 10).unwrap();
        let second = handle.run(1, 0).unwrap();
        handle.cancel(first).unwrap();
        let mut seen = vec![
            events.recv_timeout(TIMEOUT).unwrap(),
            events.recv_timeout(TIMEOUT).unwrap(),
        ];
        seen.sort_by_key(|e| e.job_id);
        assert_eq!(
            seen,
            vec![
                JobEvent {
                    job_id: first,
                    outcome: JobOutcome::Cancelled
                },
                JobEvent {
                    job_id: second,
                    outcome: JobOutcome::Busy
                },
            ]
        );
        handle.stop();
    }

    #[test]
    fn missing_worker_reports_failure_instead_of_hanging() {
        let (tx, events) = mpsc::channel();
        let handle = WorkerHandle::start(Some(PathBuf::from("does-not-exist")), move |e| {
            let _ = tx.send(e);
        });
        let id = handle.run(1, 0).unwrap();
        assert_eq!(
            events.recv_timeout(TIMEOUT),
            Ok(failed(id, "worker unavailable"))
        );
        handle.stop();
    }

    #[test]
    fn events_serialize_to_the_frontend_contract() {
        let event = JobEvent {
            job_id: 3,
            outcome: JobOutcome::Done { units: 9 },
        };
        assert_eq!(
            serde_json::to_string(&event).unwrap(),
            r#"{"jobId":3,"outcome":{"kind":"done","units":9}}"#
        );
        assert_eq!(
            serde_json::to_string(&CommandError::InvalidArgument { field: "units" }).unwrap(),
            r#"{"code":"invalidArgument","field":"units"}"#
        );
    }
}
