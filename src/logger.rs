//! Minimal `@actions/core`-style workflow command emitter.
//!
//! GitHub Actions interprets specially formatted lines on stdout as workflow
//! commands. This module covers the small subset we need:
//! `info`, `warning`, `error` and `set_failed`.
//!
//! Reference: <https://docs.github.com/actions/using-workflows/workflow-commands-for-github-actions>

use std::io::Write;

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

/// Default logger that writes workflow commands to stdout.
pub struct StdoutLogger;

impl Logger for StdoutLogger {
    fn info(&mut self, msg: &str) {
        println!("{}", msg);
    }

    fn warning(&mut self, msg: &str) {
        println!("::warning::{}", escape_data(msg));
    }

    fn error(&mut self, msg: &str) {
        println!("::error::{}", escape_data(msg));
    }

    fn set_failed(&mut self, msg: &str) {
        // `setFailed` in @actions/core both prints an error and exits 1.
        // We only print here; main() is responsible for the exit code.
        self.error(msg);
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
}
