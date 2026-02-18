use log::{debug, error, info, warn};
use regex::Regex;
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::db::Database;

const MAIL_LOG_PATH: &str = "/var/log/mail.log";
const POLL_INTERVAL: Duration = Duration::from_secs(5);
const ENABLED_CACHE_TTL: Duration = Duration::from_secs(30);

/// A parsed authentication failure from a mail service log line.
#[derive(Debug, Clone, PartialEq)]
pub struct AuthFailure {
    pub ip: String,
    pub service: String,
    pub detail: String,
}

// Lazily compiled regex patterns for Postfix and Dovecot log parsing.

static POSTFIX_SASL: OnceLock<Regex> = OnceLock::new();
static DOVECOT_AUTH: OnceLock<Regex> = OnceLock::new();
static DOVECOT_INVALID: OnceLock<Regex> = OnceLock::new();
static POSTFIX_ERRORS: OnceLock<Regex> = OnceLock::new();

fn postfix_sasl_re() -> &'static Regex {
    POSTFIX_SASL.get_or_init(|| {
        Regex::new(r"postfix/smtpd\[\d+\]: warning: [^\[]*\[([0-9a-fA-F.:]+)\]: SASL \S+ authentication failed")
            .expect("Invalid regex")
    })
}

fn dovecot_auth_re() -> &'static Regex {
    DOVECOT_AUTH.get_or_init(|| {
        Regex::new(r"dovecot: (imap|pop3)-login: .+(?:auth failed|Auth process broken).+rip=([0-9a-fA-F.:]+)")
            .expect("Invalid regex")
    })
}

fn dovecot_invalid_re() -> &'static Regex {
    DOVECOT_INVALID.get_or_init(|| {
        Regex::new(r"dovecot: (imap|pop3)-login: .+Disconnected.+too many invalid.+rip=([0-9a-fA-F.:]+)")
            .expect("Invalid regex")
    })
}

fn postfix_errors_re() -> &'static Regex {
    POSTFIX_ERRORS.get_or_init(|| {
        Regex::new(r"postfix/smtpd\[\d+\]: warning: [^\[]*\[([0-9a-fA-F.:]+)\]: too many errors")
            .expect("Invalid regex")
    })
}

/// Parse a single log line for authentication failures from Postfix or Dovecot.
///
/// Postfix SASL auth failures look like:
///   `... postfix/smtpd[...]: warning: ...[1.2.3.4]: SASL LOGIN authentication failed: ...`
///   `... postfix/smtpd[...]: warning: ...[1.2.3.4]: SASL PLAIN authentication failed: ...`
///
/// Dovecot auth failures look like:
///   `... dovecot: imap-login: Disconnected: ... (auth failed, ...): ... rip=1.2.3.4, ...`
///   `... dovecot: pop3-login: Aborted login: ... (auth failed, ...): ... rip=1.2.3.4, ...`
///   `... dovecot: imap-login: Disconnected (auth failed, ...): ... rip=1.2.3.4, ...`
pub fn parse_log_line(line: &str) -> Option<AuthFailure> {
    // Postfix SASL authentication failure
    if let Some(caps) = postfix_sasl_re().captures(line) {
        return Some(AuthFailure {
            ip: caps[1].to_string(),
            service: "smtp".to_string(),
            detail: line.to_string(),
        });
    }

    // Dovecot IMAP/POP3 auth failure
    if let Some(caps) = dovecot_auth_re().captures(line) {
        let proto = &caps[1];
        let service = if proto == "pop3" { "pop3" } else { "imap" };
        return Some(AuthFailure {
            ip: caps[2].to_string(),
            service: service.to_string(),
            detail: line.to_string(),
        });
    }

    // Dovecot: too many invalid commands
    if let Some(caps) = dovecot_invalid_re().captures(line) {
        let proto = &caps[1];
        let service = if proto == "pop3" { "pop3" } else { "imap" };
        return Some(AuthFailure {
            ip: caps[2].to_string(),
            service: service.to_string(),
            detail: line.to_string(),
        });
    }

    // Postfix: too many connection errors
    if let Some(caps) = postfix_errors_re().captures(line) {
        return Some(AuthFailure {
            ip: caps[1].to_string(),
            service: "smtp".to_string(),
            detail: line.to_string(),
        });
    }

    None
}

/// Process a detected auth failure: record, count, and potentially ban the IP.
fn handle_auth_failure(db: &Database, failure: &AuthFailure) {
    // Check whitelist first
    if db.is_ip_whitelisted(&failure.ip) {
        debug!(
            "[fail2ban] skipping whitelisted IP {} for service {}",
            failure.ip, failure.service
        );
        return;
    }

    // Check if already banned
    if db.is_ip_banned(&failure.ip) {
        debug!(
            "[fail2ban] IP {} already banned, skipping",
            failure.ip
        );
        return;
    }

    // Record the attempt
    db.record_fail2ban_attempt(&failure.ip, &failure.service, &failure.detail);

    // Get settings for this service
    let setting = match db.get_fail2ban_setting_by_service(&failure.service) {
        Some(s) => s,
        None => {
            debug!(
                "[fail2ban] no settings configured for service {}, skipping",
                failure.service
            );
            return;
        }
    };

    if !setting.enabled {
        debug!(
            "[fail2ban] service {} is disabled, skipping ban check",
            failure.service
        );
        return;
    }

    // Count recent attempts within the find_time window
    let recent_count = db.count_recent_attempts(&failure.ip, &failure.service, setting.find_time_minutes);

    info!(
        "[fail2ban] IP {} service {} has {} attempts in last {} min (threshold: {})",
        failure.ip, failure.service, recent_count, setting.find_time_minutes, setting.max_attempts
    );

    if recent_count >= setting.max_attempts as i64 {
        let reason = format!(
            "Auto-banned: {}: {} failed attempts in {} min",
            failure.service, recent_count, setting.find_time_minutes
        );
        match db.ban_ip(
            &failure.ip,
            &failure.service,
            &reason,
            setting.ban_duration_minutes,
            false,
        ) {
            Ok(_) => {
                warn!(
                    "[fail2ban] BANNED IP {} for service {} — {} attempts exceeded threshold of {} (ban duration: {} min)",
                    failure.ip, failure.service, recent_count, setting.max_attempts, setting.ban_duration_minutes
                );
            }
            Err(e) => {
                error!(
                    "[fail2ban] failed to ban IP {}: {}",
                    failure.ip, e
                );
            }
        }
    }
}

/// Start the fail2ban log watcher daemon. This runs in a background thread
/// and continuously tails the mail log file for authentication failures.
pub fn start_watcher(db: Database) {
    info!("[fail2ban] starting log watcher for {}", MAIL_LOG_PATH);

    std::thread::spawn(move || {
        // Wait for the log file to be created (syslog may start after us)
        loop {
            if Path::new(MAIL_LOG_PATH).exists() {
                break;
            }
            debug!("[fail2ban] waiting for {} to appear...", MAIL_LOG_PATH);
            std::thread::sleep(Duration::from_secs(2));
        }

        info!("[fail2ban] log file found, starting to monitor {}", MAIL_LOG_PATH);

        loop {
            match tail_log_file(&db) {
                Ok(()) => {
                    warn!("[fail2ban] log watcher loop exited, restarting in 5s");
                }
                Err(e) => {
                    error!("[fail2ban] log watcher error: {}, restarting in 5s", e);
                }
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    });
}

/// Tail the mail log file, seeking to the end and then processing new lines.
fn tail_log_file(db: &Database) -> Result<(), std::io::Error> {
    let mut file = File::open(MAIL_LOG_PATH)?;
    // Seek to end — we only process new log lines
    file.seek(SeekFrom::End(0))?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();

    // Cache the global enabled state to avoid querying the DB on every log line
    let mut enabled_cache = db.is_fail2ban_enabled();
    let mut cache_refreshed = Instant::now();

    info!("[fail2ban] tailing {} from end of file", MAIL_LOG_PATH);

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => {
                // No new data, check if the file was rotated
                if !Path::new(MAIL_LOG_PATH).exists() {
                    warn!("[fail2ban] log file disappeared, will re-open");
                    return Ok(());
                }
                // Check file size — if it shrank, the file was rotated
                let meta = std::fs::metadata(MAIL_LOG_PATH)?;
                let current_pos = reader.get_ref().stream_position()?;
                if meta.len() < current_pos {
                    info!("[fail2ban] log file was rotated, re-opening");
                    return Ok(());
                }
                // Refresh cache during idle periods
                if cache_refreshed.elapsed() >= ENABLED_CACHE_TTL {
                    enabled_cache = db.is_fail2ban_enabled();
                    cache_refreshed = Instant::now();
                }
                std::thread::sleep(POLL_INTERVAL);
            }
            Ok(_) => {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    if let Some(failure) = parse_log_line(trimmed) {
                        // Refresh the cached enabled state periodically
                        if cache_refreshed.elapsed() >= ENABLED_CACHE_TTL {
                            enabled_cache = db.is_fail2ban_enabled();
                            cache_refreshed = Instant::now();
                        }
                        if !enabled_cache {
                            debug!("[fail2ban] system disabled globally, skipping");
                            continue;
                        }
                        info!(
                            "[fail2ban] detected auth failure: ip={} service={}",
                            failure.ip, failure.service
                        );
                        handle_auth_failure(db, &failure);
                    }
                }
            }
            Err(e) => {
                error!("[fail2ban] error reading log line: {}", e);
                return Err(e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_postfix_sasl_login_failure() {
        let line = "Feb 18 10:15:23 mail postfix/smtpd[1234]: warning: unknown[192.168.1.100]: SASL LOGIN authentication failed: UGFzc3dvcmQ6";
        let result = parse_log_line(line);
        assert!(result.is_some());
        let f = result.unwrap();
        assert_eq!(f.ip, "192.168.1.100");
        assert_eq!(f.service, "smtp");
    }

    #[test]
    fn parse_postfix_sasl_plain_failure() {
        let line = "Feb 18 10:15:23 mail postfix/smtpd[5678]: warning: mail.example.com[10.0.0.5]: SASL PLAIN authentication failed: generic failure";
        let result = parse_log_line(line);
        assert!(result.is_some());
        let f = result.unwrap();
        assert_eq!(f.ip, "10.0.0.5");
        assert_eq!(f.service, "smtp");
    }

    #[test]
    fn parse_dovecot_imap_auth_failure() {
        let line = "Feb 18 10:15:23 mail dovecot: imap-login: Disconnected: Too many invalid IMAP commands (auth failed, 3 attempts in 5 secs): user=<attacker>, method=PLAIN, rip=203.0.113.42, lip=192.168.1.1";
        let result = parse_log_line(line);
        assert!(result.is_some());
        let f = result.unwrap();
        assert_eq!(f.ip, "203.0.113.42");
        assert_eq!(f.service, "imap");
    }

    #[test]
    fn parse_dovecot_pop3_auth_failure() {
        let line = "Feb 18 10:15:23 mail dovecot: pop3-login: Aborted login (auth failed, 1 attempts in 2 secs): user=<user@example.com>, method=PLAIN, rip=172.16.0.10, lip=10.0.0.1";
        let result = parse_log_line(line);
        assert!(result.is_some());
        let f = result.unwrap();
        assert_eq!(f.ip, "172.16.0.10");
        assert_eq!(f.service, "pop3");
    }

    #[test]
    fn parse_postfix_too_many_errors() {
        let line = "Feb 18 10:15:23 mail postfix/smtpd[9012]: warning: 192.168.1.50[192.168.1.50]: too many errors after AUTH";
        let result = parse_log_line(line);
        assert!(result.is_some());
        let f = result.unwrap();
        assert_eq!(f.ip, "192.168.1.50");
        assert_eq!(f.service, "smtp");
    }

    #[test]
    fn parse_dovecot_ipv6_auth_failure() {
        let line = "Feb 18 10:15:23 mail dovecot: imap-login: Disconnected (auth failed, 1 attempts in 3 secs): user=<test>, method=PLAIN, rip=2001:db8::1, lip=::1";
        let result = parse_log_line(line);
        assert!(result.is_some());
        let f = result.unwrap();
        assert_eq!(f.ip, "2001:db8::1");
        assert_eq!(f.service, "imap");
    }

    #[test]
    fn parse_normal_log_line_returns_none() {
        let line = "Feb 18 10:15:23 mail postfix/smtpd[1234]: connect from mail.example.com[1.2.3.4]";
        assert!(parse_log_line(line).is_none());
    }

    #[test]
    fn parse_dovecot_successful_login_returns_none() {
        let line = "Feb 18 10:15:23 mail dovecot: imap-login: Login: user=<user@example.com>, method=PLAIN, rip=1.2.3.4, lip=10.0.0.1";
        assert!(parse_log_line(line).is_none());
    }

    #[test]
    fn parse_empty_line_returns_none() {
        assert!(parse_log_line("").is_none());
    }

    #[test]
    fn parse_postfix_sasl_with_hostname_bracket() {
        let line = "Feb 18 10:15:23 mail postfix/smtpd[3456]: warning: host.example.com[192.0.2.1]: SASL CRAM-MD5 authentication failed: ";
        let result = parse_log_line(line);
        assert!(result.is_some());
        let f = result.unwrap();
        assert_eq!(f.ip, "192.0.2.1");
        assert_eq!(f.service, "smtp");
    }
}
