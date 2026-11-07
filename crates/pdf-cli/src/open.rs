//! Reading the input file of a command.

use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

use pdf_syntax::{MemorySource, ResourceLimits};

/// Reads the regular file at `path` within `limits.max_input_bytes`. On
/// failure, writes the reason to `err` and returns `None`. The path is not
/// echoed: it may hold control characters or private names. The type is
/// checked on the path first (Windows refuses to open a directory as a file)
/// and again on the opened handle.
pub fn source(
    path: &Path,
    limits: &ResourceLimits,
    err: &mut dyn Write,
) -> io::Result<Option<MemorySource>> {
    let opened = std::fs::metadata(path).and_then(|m| {
        if m.is_file() {
            File::open(path)
                .and_then(|f| f.metadata().map(|m| (f, m)))
                .map(|(f, m)| m.is_file().then_some((f, m.len())))
        } else {
            Ok(None)
        }
    });
    let (file, size) = match opened {
        Ok(Some(opened)) => opened,
        Ok(None) => {
            writeln!(err, "error: not a regular file")?;
            return Ok(None);
        }
        Err(e) => {
            writeln!(err, "error: cannot open the file: {}", e.kind())?;
            return Ok(None);
        }
    };
    match MemorySource::from_reader(file, Some(size), limits) {
        Ok(source) => Ok(Some(source)),
        Err(e) => {
            writeln!(err, "error: {e}")?;
            Ok(None)
        }
    }
}
