//! `castle-pdf`: diagnostic command line over the CastlePDF engine.
//!
//! `castle-pdf inspect <file>` reports the file structure (version, revisions,
//! cross-reference entries, trailer keys) and the pages (ids, boxes, rotation).
//! It never prints document content: no strings, streams or resource values.
//! Exit codes: 0 success, 1 the document cannot be read, 2 usage error.

mod inspect;

use std::io::Write;
use std::process::ExitCode;

const USAGE: &str = "\
castle-pdf — diagnostic tool of the CastlePDF engine

Usage:
  castle-pdf inspect <file>   Report the file structure and the pages
  castle-pdf --help           Show this help
  castle-pdf --version        Show the version

The report lists structure only, never document content. Exit codes: 0 success,
1 the document cannot be read (the error says why), 2 usage error.
";

fn main() -> ExitCode {
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let code = run(&args, &mut stdout.lock(), &mut stderr.lock());
    ExitCode::from(code)
}

/// Runs one command; returns the process exit code.
fn run(args: &[std::ffi::OsString], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let args: Vec<&std::ffi::OsStr> = args.iter().map(|a| a.as_os_str()).collect();
    let result = match args.as_slice() {
        [flag] if *flag == "--help" || *flag == "-h" => out.write_all(USAGE.as_bytes()).map(|()| 0),
        [flag] if *flag == "--version" => {
            writeln!(out, "castle-pdf {}", env!("CARGO_PKG_VERSION")).map(|()| 0)
        }
        [command, path] if *command == "inspect" => inspect::run(path.as_ref(), out, err),
        _ => err.write_all(USAGE.as_bytes()).map(|()| 2),
    };
    // A closed output (such as a pipe to `head`) is not a document error.
    result.unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    fn call(args: &[&str]) -> (u8, String, String) {
        let args: Vec<OsString> = args.iter().map(OsString::from).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&args, &mut out, &mut err);
        (
            code,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        )
    }

    #[test]
    fn help_version_and_usage_errors() {
        let (code, out, _) = call(&["--help"]);
        assert_eq!(code, 0);
        assert!(out.contains("castle-pdf inspect <file>"));
        let (code, out, _) = call(&["--version"]);
        assert_eq!(
            (code, out.trim()),
            (0, concat!("castle-pdf ", env!("CARGO_PKG_VERSION")))
        );
        for args in [
            &[][..],
            &["inspect"],
            &["inspect", "a", "b"],
            &["open", "a"],
        ] {
            let (code, out, err) = call(args);
            assert_eq!(code, 2, "{args:?}");
            assert!(out.is_empty());
            assert!(err.contains("Usage:"));
        }
    }
}
