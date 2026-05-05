//! Minimal `@actions/core`-style workflow command emitter.
//!
//! GitHub Actions interprets specially formatted lines on stdout as workflow
//! commands. This module covers the small subset we need:
//! `info`, `warning`, `error` and `set_failed`.
//!
//! Reference: <https://docs.github.com/actions/using-workflows/workflow-commands-for-github-actions>

use std::io::{self, Write};

/// Escape characters that have special meaning inside workflow command
/// arguments / data sections.
fn escape_data(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// A logger abstraction so tests can capture output instead of writing to
/// the process stdout.
pub trait Logger {
    fn info(&mut self, msg: &str);
    fn warning(&mut self, msg: &str);
    fn error(&mut self, msg: &str);
    fn set_failed(&mut self, msg: &str);
}

/// Logger that emits workflow commands to any `Write` sink. Generic so that
/// production uses [`io::Stdout`] and tests can pin a `Vec<u8>` to assert
/// the exact bytes that would land on the runner's log.
pub struct WriteLogger<W: Write> {
    pub sink: W,
}

impl<W: Write> WriteLogger<W> {
    pub fn new(sink: W) -> Self {
        Self { sink }
    }
}

impl<W: Write> Logger for WriteLogger<W> {
    fn info(&mut self, msg: &str) {
        let _ = writeln!(self.sink, "{}", msg);
    }

    fn warning(&mut self, msg: &str) {
        let _ = writeln!(self.sink, "::warning::{}", escape_data(msg));
    }

    fn error(&mut self, msg: &str) {
        let _ = writeln!(self.sink, "::error::{}", escape_data(msg));
    }

    fn set_failed(&mut self, msg: &str) {
        // `setFailed` in @actions/core both prints an error and exits 1.
        // We only print here; main() is responsible for the exit code.
        self.error(msg);
    }
}

/// Default logger that writes workflow commands to the process stdout.
pub type StdoutLogger = WriteLogger<io::Stdout>;

impl Default for StdoutLogger {
    fn default() -> Self {
        Self::new(io::stdout())
    }
}

/// In-memory logger used in tests.
#[derive(Default, Debug, Clone)]
pub struct CaptureLogger {
    pub lines: Vec<String>,
}

impl CaptureLogger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn contains(&self, needle: &str) -> bool {
        self.lines.iter().any(|l| l.contains(needle))
    }
}

impl Logger for CaptureLogger {
    fn info(&mut self, msg: &str) {
        self.lines.push(format!("info: {}", msg));
    }
    fn warning(&mut self, msg: &str) {
        self.lines.push(format!("warning: {}", msg));
    }
    fn error(&mut self, msg: &str) {
        self.lines.push(format!("error: {}", msg));
    }
    fn set_failed(&mut self, msg: &str) {
        self.lines.push(format!("failed: {}", msg));
    }
}

/// Helper used from `main` to flush stdout before exiting.
pub fn flush() {
    let _ = std::io::stdout().flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_handles_special_chars() {
        assert_eq!(escape_data("a\nb"), "a%0Ab");
        assert_eq!(escape_data("a\rb"), "a%0Db");
        assert_eq!(escape_data("50%"), "50%25");
        assert_eq!(escape_data("plain"), "plain");
    }

    #[test]
    fn capture_logger_records_messages() {
        let mut log = CaptureLogger::new();
        log.info("hi");
        log.warning("careful");
        log.error("bad");
        log.set_failed("dead");
        assert!(log.contains("hi"));
        assert!(log.contains("careful"));
        assert!(log.contains("bad"));
        assert!(log.contains("dead"));
    }

    fn writer_lines(buf: &[u8]) -> Vec<&str> {
        std::str::from_utf8(buf)
            .unwrap()
            .lines()
            .collect::<Vec<_>>()
    }

    #[test]
    fn write_logger_emits_workflow_command_prefixes() {
        // Pins down the exact bytes the runner sees: bare info lines, and
        // `::warning::` / `::error::` prefixes for the other levels. A
        // regression that drops a prefix would silently demote a failure
        // to a normal log line.
        let mut log = WriteLogger::new(Vec::<u8>::new());
        log.info("hello");
        log.warning("careful");
        log.error("bad");
        log.set_failed("dead");
        assert_eq!(
            writer_lines(&log.sink),
            vec![
                "hello",
                "::warning::careful",
                "::error::bad",
                "::error::dead",
            ],
        );
    }

    #[test]
    fn write_logger_escapes_data_in_warnings_and_errors() {
        // Multi-line / `%`-containing messages must be escaped so the
        // runner doesn't interpret embedded newlines as the start of a
        // new workflow command.
        let mut log = WriteLogger::new(Vec::<u8>::new());
        log.warning("line1\nline2");
        log.error("50% done\rback");
        assert_eq!(
            writer_lines(&log.sink),
            vec!["::warning::line1%0Aline2", "::error::50%25 done%0Dback"],
        );
    }

    #[test]
    fn write_logger_does_not_escape_info_payload() {
        // Info lines are plain prints — the runner only parses
        // `::command::` prefixes, so escaping would just garble the
        // human-readable text.
        let mut log = WriteLogger::new(Vec::<u8>::new());
        log.info("50% done");
        assert_eq!(writer_lines(&log.sink), vec!["50% done"]);
    }
}
