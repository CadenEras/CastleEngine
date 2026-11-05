//! `pdf-worker` process. stdout carries protocol frames only; diagnostics go
//! to stderr and never include document content.

use std::process::ExitCode;

fn main() -> ExitCode {
    let input = std::io::stdin().lock();
    let output = std::io::stdout();
    match pdf_worker::server::serve(input, output) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("pdf-worker: {error}");
            ExitCode::from(2)
        }
    }
}
