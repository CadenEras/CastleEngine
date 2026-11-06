// No console window for release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod worker;

use std::process::ExitCode;

use tauri::{Emitter, Manager, RunEvent, State};

use worker::{CommandError, JobEvent, WorkerHandle};

const JOB_EVENT: &str = "worker-job";

#[tauri::command]
fn worker_run(
    worker: State<'_, WorkerHandle>,
    units: u64,
    unit_delay_ms: u8,
) -> Result<u64, CommandError> {
    worker.run(units, unit_delay_ms)
}

#[tauri::command]
fn worker_cancel(worker: State<'_, WorkerHandle>, job_id: u64) -> Result<(), CommandError> {
    worker.cancel(job_id)
}

fn main() -> ExitCode {
    let app = tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            let worker =
                WorkerHandle::start(worker::bundled_worker_path(), move |event: JobEvent| {
                    if let Err(error) = handle.emit(JOB_EVENT, event) {
                        eprintln!("castle-pdf: cannot publish job event: {error}");
                    }
                });
            app.manage(worker);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![worker_run, worker_cancel])
        .build(tauri::generate_context!());

    match app {
        Ok(app) => {
            app.run(|app, event| {
                if let RunEvent::Exit = event {
                    app.state::<WorkerHandle>().stop();
                }
            });
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("castle-pdf: cannot start: {error}");
            ExitCode::FAILURE
        }
    }
}
