//! In-memory application log buffer.
//!
//! Wraps the env_logger sink so that every record which passes the configured
//! `RUST_LOG` filter is still written to stderr (unchanged behaviour), and is
//! also kept in a bounded ring buffer that the admin UI shows at `/logs/app`.
//!
//! The buffer is per process: it contains the records emitted by the `serve`
//! process (the admin web server and the background fail2ban watcher). Short
//! CLI commands and the Postfix pipe filter run in their own processes and
//! keep logging to stderr only.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

use log::{Log, Metadata, Record};

/// Maximum number of records kept in memory. Oldest records are dropped first.
pub const CAPACITY: usize = 2000;

#[derive(Clone, Debug)]
pub struct AppLogLine {
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
}

static BUFFER: OnceLock<Mutex<VecDeque<AppLogLine>>> = OnceLock::new();

fn buffer() -> &'static Mutex<VecDeque<AppLogLine>> {
    BUFFER.get_or_init(|| Mutex::new(VecDeque::with_capacity(CAPACITY)))
}

/// Returns up to `limit` of the most recent records, newest first.
pub fn recent(limit: usize) -> Vec<AppLogLine> {
    let guard = match buffer().lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.iter().rev().take(limit).cloned().collect()
}

fn push(line: AppLogLine) {
    let mut guard = match buffer().lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    if guard.len() >= CAPACITY {
        guard.pop_front();
    }
    guard.push_back(line);
}

/// Logger that forwards to env_logger (stderr) and records to the ring buffer.
struct TeeLogger {
    inner: env_logger::Logger,
}

impl Log for TeeLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.inner.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        if !self.inner.matches(record) {
            return;
        }
        self.inner.log(record);
        push(AppLogLine {
            timestamp: chrono::Local::now()
                .format("%Y-%m-%d %H:%M:%S%.3f")
                .to_string(),
            level: record.level().to_string(),
            target: record.target().to_string(),
            message: record.args().to_string(),
        });
    }

    fn flush(&self) {
        self.inner.flush();
    }
}

/// Installs the global logger. Replaces `builder.init()` in `main`.
///
/// Returns an error if a logger was already installed.
pub fn init(builder: &mut env_logger::Builder) -> Result<(), log::SetLoggerError> {
    let logger = builder.build();
    log::set_max_level(logger.filter());
    log::set_boxed_logger(Box::new(TeeLogger { inner: logger }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_returns_newest_first_and_respects_limit() {
        // The buffer is process-global, so only assert on the records we push here.
        for i in 0..3 {
            push(AppLogLine {
                timestamp: "t".into(),
                level: "INFO".into(),
                target: "applog::test".into(),
                message: format!("applog-test-{}", i),
            });
        }
        let lines: Vec<String> = recent(usize::MAX)
            .into_iter()
            .map(|l| l.message)
            .filter(|m| m.starts_with("applog-test-"))
            .collect();
        assert_eq!(lines.first().map(String::as_str), Some("applog-test-2"));
        assert!(lines.iter().position(|m| m == "applog-test-0").unwrap()
            > lines.iter().position(|m| m == "applog-test-2").unwrap());
        assert!(recent(1).len() <= 1);
    }
}
