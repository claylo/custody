use std::{
    fmt,
    io::{self, Write},
};

use anyhow::{Context, Result};
use serde::Serialize;

/// Print any serializable report as stable pretty JSON.
pub fn print_json(value: &impl Serialize) -> Result<()> {
    let json = serde_json::to_string_pretty(value)?;
    print_line(format_args!("{json}"))
}

/// Write one report line to standard output and preserve I/O failures.
pub fn print_line(arguments: fmt::Arguments<'_>) -> Result<()> {
    let stdout = io::stdout();
    write_line(&mut stdout.lock(), arguments)
}

fn write_line(writer: &mut impl Write, arguments: fmt::Arguments<'_>) -> Result<()> {
    writeln!(writer, "{arguments}").context("failed to write report to stdout")
}

/// Whether an error chain was caused by a reader closing stdout early.
#[must_use]
pub fn is_broken_pipe(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<io::Error>()
            .is_some_and(|error| error.kind() == io::ErrorKind::BrokenPipe)
    })
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};

    use super::*;

    struct ClosedPipe;

    impl Write for ClosedPipe {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn preserves_broken_pipe_through_the_error_chain() {
        let error = write_line(&mut ClosedPipe, format_args!("report")).unwrap_err();

        assert!(is_broken_pipe(&error));
    }
}
