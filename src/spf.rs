//! SPF policy walker for the admin toolbox.
//!
//! Follows an SPF policy the way a receiving server does (RFC 7208): every
//! `include:`, `redirect=`, `a`, and `mx` term is evaluated, and the DNS-lookup
//! budget is counted. The result is a flat list of lines annotated with their
//! depth, which the page renders as a tree.
//!
//! DNS access goes through the `SpfResolver` trait so the walk can be tested
//! with fixed data.

use std::net::IpAddr;

use crate::dns;

/// RFC 7208 §4.6.4: at most 10 DNS-querying terms per check.
pub const MAX_DNS_LOOKUPS: usize = 10;
/// Deepest include/redirect nesting followed.
const MAX_DEPTH: usize = 10;
/// Upper bound on TXT queries for one trace, as a safety net.
const MAX_RECORD_QUERIES: usize = 100;
/// RFC 7208 §4.6.4: at most 10 MX names are followed for one `mx` term.
const MAX_MX_NAMES: usize = 10;

pub trait SpfResolver {
    fn txt(&self, name: &str) -> Result<Vec<String>, String>;
    /// A and AAAA addresses combined.
    fn ip_addrs(&self, name: &str) -> Result<Vec<IpAddr>, String>;
    fn mx_hosts(&self, name: &str) -> Result<Vec<String>, String>;
}

/// Resolver backed by the server's configured nameservers.
pub struct SystemResolver;

impl SpfResolver for SystemResolver {
    fn txt(&self, name: &str) -> Result<Vec<String>, String> {
        dns::lookup_txt(name).map_err(|e| e.to_string())
    }

    fn ip_addrs(&self, name: &str) -> Result<Vec<IpAddr>, String> {
        match (dns::lookup_a(name), dns::lookup_aaaa(name)) {
            (Err(e), Err(_)) => Err(e.to_string()),
            (v4, v6) => {
                let mut out: Vec<IpAddr> = Vec::new();
                if let Ok(addrs) = v4 {
                    out.extend(addrs.into_iter().map(IpAddr::V4));
                }
                if let Ok(addrs) = v6 {
                    out.extend(addrs.into_iter().map(IpAddr::V6));
                }
                Ok(out)
            }
        }
    }

    fn mx_hosts(&self, name: &str) -> Result<Vec<String>, String> {
        dns::lookup_mx(name).map_err(|e| e.to_string())
    }
}

/// One line of the output tree. `class` selects how the page styles it:
/// "record", "mechanism", "resolved", "info", "warning", or "error".
#[derive(Debug, Clone, PartialEq)]
pub struct SpfLine {
    pub depth: usize,
    pub class: &'static str,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpfTrace {
    pub lines: Vec<SpfLine>,
    pub lookups: usize,
    pub limit: usize,
    pub over_limit: bool,
}

pub fn trace(resolver: &dyn SpfResolver, domain: &str) -> SpfTrace {
    let mut walk = Walk {
        resolver,
        lines: Vec::new(),
        lookups: 0,
        record_queries: 0,
    };
    walk.record("", domain, 0, &mut Vec::new());
    SpfTrace {
        lines: walk.lines,
        lookups: walk.lookups,
        limit: MAX_DNS_LOOKUPS,
        over_limit: walk.lookups > MAX_DNS_LOOKUPS,
    }
}

struct Walk<'a> {
    resolver: &'a dyn SpfResolver,
    lines: Vec<SpfLine>,
    lookups: usize,
    record_queries: usize,
}

impl<'a> Walk<'a> {
    fn push(&mut self, depth: usize, class: &'static str, text: impl Into<String>) {
        self.lines.push(SpfLine {
            depth,
            class,
            text: text.into(),
        });
    }

    /// Evaluates the SPF record published at `domain`. `label` is the prefix
    /// shown before the domain ("include:", "redirect=", or "" for the root).
    /// `path` holds the domains currently being evaluated, to detect loops.
    fn record(&mut self, label: &str, domain: &str, depth: usize, path: &mut Vec<String>) {
        let key = domain.to_ascii_lowercase();
        if depth > MAX_DEPTH {
            self.push(
                depth,
                "error",
                format!("{}{}: not followed, nesting limit {} reached", label, domain, MAX_DEPTH),
            );
            return;
        }
        if path.contains(&key) {
            self.push(
                depth,
                "warning",
                format!("{}{}: loop, already evaluated above", label, domain),
            );
            return;
        }
        if self.record_queries >= MAX_RECORD_QUERIES {
            self.push(
                depth,
                "error",
                format!("{}{}: not followed, query budget exhausted", label, domain),
            );
            return;
        }
        self.record_queries += 1;

        let txts = match self.resolver.txt(domain) {
            Ok(txts) => txts,
            Err(e) => {
                self.push(
                    depth,
                    "error",
                    format!("{}{}: DNS lookup failed: {}", label, domain, e),
                );
                return;
            }
        };
        let spf: Vec<String> = txts.into_iter().filter(|t| is_spf(t)).collect();
        let raw = match spf.first() {
            Some(raw) => raw.clone(),
            None => {
                self.push(
                    depth,
                    "error",
                    format!("{}{}: no SPF record (v=spf1 TXT)", label, domain),
                );
                return;
            }
        };

        self.push(depth, "record", format!("{}{}", label, domain));
        self.push(depth + 1, "info", raw.clone());
        if spf.len() > 1 {
            self.push(
                depth + 1,
                "warning",
                format!(
                    "{} SPF records published; receivers return permerror",
                    spf.len()
                ),
            );
        }

        path.push(key);
        for term in raw.split_whitespace().skip(1) {
            self.term(term, domain, depth, path);
        }
        path.pop();
    }

    /// Evaluates one term of a record published at `domain` (at `depth`).
    fn term(&mut self, term: &str, domain: &str, depth: usize, path: &mut Vec<String>) {
        // Strip a single qualifier (+ - ~ ?). Qualifiers are ASCII, so slicing is safe.
        let body = match term.as_bytes().first() {
            Some(b'+') | Some(b'-') | Some(b'~') | Some(b'?') => &term[1..],
            _ => term,
        };
        let lower = body.to_ascii_lowercase();
        let child = depth + 1;

        if lower.starts_with("ip4:") || lower.starts_with("ip6:") || lower == "all" {
            self.push(child, "mechanism", term);
        } else if lower == "a" || lower.starts_with("a:") || lower.starts_with("a/") {
            self.lookups += 1;
            self.push(child, "mechanism", term);
            let host = mechanism_host(body, 1, domain);
            self.addresses(&host, child + 1);
        } else if lower == "mx" || lower.starts_with("mx:") || lower.starts_with("mx/") {
            self.lookups += 1;
            self.push(child, "mechanism", term);
            let host = mechanism_host(body, 2, domain);
            self.mail_exchangers(&host, child + 1);
        } else if lower.starts_with("include:") {
            self.lookups += 1;
            let target = body.get(8..).unwrap_or("").trim_end_matches('.');
            self.record("include:", target, child, path);
        } else if lower.starts_with("redirect=") {
            self.lookups += 1;
            let target = body.get(9..).unwrap_or("").trim_end_matches('.');
            self.record("redirect=", target, child, path);
        } else if lower == "ptr"
            || lower.starts_with("ptr:")
            || lower.starts_with("ptr/")
            || lower.starts_with("exists:")
        {
            // Counted against the limit but not evaluated: PTR results depend on the
            // connecting client's address, and exists: depends on the sender.
            self.lookups += 1;
            self.push(child, "info", format!("{} (counted, not evaluated)", term));
        } else {
            // Modifiers such as exp= and unknown mechanisms.
            self.push(child, "info", format!("{} (not evaluated)", term));
        }
    }

    fn addresses(&mut self, host: &str, depth: usize) {
        if host.contains('%') {
            self.push(depth, "info", format!("{}: contains macros, not expanded", host));
            return;
        }
        match self.resolver.ip_addrs(host) {
            Ok(addrs) if addrs.is_empty() => {
                self.push(depth, "info", format!("{}: no address records", host));
            }
            Ok(addrs) => {
                for ip in addrs {
                    self.push(depth, "resolved", ip.to_string());
                }
            }
            Err(e) => {
                self.push(depth, "error", format!("{}: DNS lookup failed: {}", host, e));
            }
        }
    }

    fn mail_exchangers(&mut self, host: &str, depth: usize) {
        if host.contains('%') {
            self.push(depth, "info", format!("{}: contains macros, not expanded", host));
            return;
        }
        match self.resolver.mx_hosts(host) {
            Ok(hosts) if hosts.is_empty() => {
                self.push(depth, "info", format!("{}: no MX records", host));
            }
            Ok(hosts) => {
                for mx in hosts.iter().take(MAX_MX_NAMES) {
                    // Each MX name needs its own address lookup.
                    self.lookups += 1;
                    self.push(depth, "resolved", format!("MX {}", mx));
                    self.addresses(mx, depth + 1);
                }
                if hosts.len() > MAX_MX_NAMES {
                    self.push(
                        depth,
                        "warning",
                        format!(
                            "{} MX records; only the first {} are followed",
                            hosts.len(),
                            MAX_MX_NAMES
                        ),
                    );
                }
            }
            Err(e) => {
                self.push(depth, "error", format!("{}: DNS lookup failed: {}", host, e));
            }
        }
    }
}

/// True for a TXT string that starts an SPF record: "v=spf1" followed by a space or the end.
fn is_spf(txt: &str) -> bool {
    let bytes = txt.as_bytes();
    bytes.len() >= 6
        && bytes[..6].eq_ignore_ascii_case(b"v=spf1")
        && (bytes.len() == 6 || bytes[6] == b' ')
}

/// Host named by an `a` or `mx` term. `name_len` is the length of the mechanism
/// name ("a" = 1, "mx" = 2). With no explicit host, the record's own domain is used.
fn mechanism_host(body: &str, name_len: usize, domain: &str) -> String {
    let rest = body.get(name_len..).unwrap_or("");
    let host = rest
        .strip_prefix(':')
        .unwrap_or("")
        .split('/')
        .next()
        .unwrap_or("");
    if host.is_empty() {
        domain.to_string()
    } else {
        host.trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[derive(Default)]
    struct FakeResolver {
        txt: HashMap<String, Vec<String>>,
        ips: HashMap<String, Vec<IpAddr>>,
        mx: HashMap<String, Vec<String>>,
    }

    impl FakeResolver {
        fn with_txt(mut self, name: &str, records: &[&str]) -> Self {
            self.txt.insert(
                name.to_string(),
                records.iter().map(|r| r.to_string()).collect(),
            );
            self
        }
        fn with_ip(mut self, name: &str, ip: &str) -> Self {
            self.ips
                .entry(name.to_string())
                .or_default()
                .push(ip.parse().unwrap());
            self
        }
        fn with_mx(mut self, name: &str, hosts: &[&str]) -> Self {
            self.mx.insert(
                name.to_string(),
                hosts.iter().map(|h| h.to_string()).collect(),
            );
            self
        }
    }

    impl SpfResolver for FakeResolver {
        fn txt(&self, name: &str) -> Result<Vec<String>, String> {
            Ok(self.txt.get(name).cloned().unwrap_or_default())
        }
        fn ip_addrs(&self, name: &str) -> Result<Vec<IpAddr>, String> {
            Ok(self.ips.get(name).cloned().unwrap_or_default())
        }
        fn mx_hosts(&self, name: &str) -> Result<Vec<String>, String> {
            Ok(self.mx.get(name).cloned().unwrap_or_default())
        }
    }

    fn texts(trace: &SpfTrace) -> Vec<&str> {
        trace.lines.iter().map(|l| l.text.as_str()).collect()
    }

    #[test]
    fn simple_policy_has_no_lookups() {
        let r = FakeResolver::default().with_txt("example.com", &["v=spf1 ip4:192.0.2.1 -all"]);
        let t = trace(&r, "example.com");
        assert_eq!(t.lookups, 0);
        assert!(!t.over_limit);
        assert!(texts(&t).contains(&"ip4:192.0.2.1"));
        assert!(texts(&t).contains(&"-all"));
    }

    #[test]
    fn include_a_and_mx_are_followed_and_counted() {
        let r = FakeResolver::default()
            .with_txt("example.com", &["v=spf1 include:_spf.a.com a mx ~all"])
            .with_txt("_spf.a.com", &["v=spf1 ip4:10.0.0.0/8 -all"])
            .with_ip("example.com", "192.0.2.10")
            .with_mx("example.com", &["mx1.example.com"])
            .with_ip("mx1.example.com", "192.0.2.25");
        let t = trace(&r, "example.com");
        // include (1) + a (1) + mx (1) + one MX host address lookup (1)
        assert_eq!(t.lookups, 4);
        assert!(texts(&t).contains(&"include:_spf.a.com"));
        assert!(texts(&t).contains(&"192.0.2.10"));
        assert!(texts(&t).contains(&"MX mx1.example.com"));
        assert!(texts(&t).contains(&"192.0.2.25"));
        let include_line = t
            .lines
            .iter()
            .find(|l| l.text == "include:_spf.a.com")
            .unwrap();
        assert_eq!(include_line.class, "record");
        assert_eq!(include_line.depth, 1);
    }

    #[test]
    fn include_loop_is_reported_not_followed() {
        let r = FakeResolver::default()
            .with_txt("a.example", &["v=spf1 include:b.example -all"])
            .with_txt("b.example", &["v=spf1 include:a.example -all"]);
        let t = trace(&r, "a.example");
        assert!(t.lines.iter().any(|l| l.class == "warning" && l.text.contains("loop")));
    }

    #[test]
    fn missing_record_is_an_error_line() {
        let r = FakeResolver::default().with_txt("example.com", &["v=spf1 include:gone.example -all"]);
        let t = trace(&r, "example.com");
        assert!(t
            .lines
            .iter()
            .any(|l| l.class == "error" && l.text == "gone.example: no SPF record (v=spf1 TXT)"));
    }

    #[test]
    fn non_spf_txt_records_are_ignored() {
        let r = FakeResolver::default().with_txt(
            "example.com",
            &["google-site-verification=abc", "v=spf1 -all"],
        );
        let t = trace(&r, "example.com");
        assert!(t.lines.iter().all(|l| l.class != "error"));
        assert!(texts(&t).contains(&"-all"));
    }

    #[test]
    fn multiple_spf_records_are_warned() {
        let r = FakeResolver::default().with_txt("example.com", &["v=spf1 -all", "v=spf1 +all"]);
        let t = trace(&r, "example.com");
        assert!(t
            .lines
            .iter()
            .any(|l| l.class == "warning" && l.text.contains("2 SPF records")));
    }

    #[test]
    fn more_than_ten_lookups_is_over_limit() {
        let includes: Vec<String> = (0..11).map(|i| format!("include:leaf{}.example", i)).collect();
        let root = format!("v=spf1 {} -all", includes.join(" "));
        let mut r = FakeResolver::default().with_txt("example.com", &[root.as_str()]);
        for i in 0..11 {
            r = r.with_txt(&format!("leaf{}.example", i), &["v=spf1 -all"]);
        }
        let t = trace(&r, "example.com");
        assert_eq!(t.lookups, 11);
        assert!(t.over_limit);
        assert_eq!(t.limit, MAX_DNS_LOOKUPS);
    }

    #[test]
    fn explicit_a_host_and_cidr_are_parsed() {
        assert_eq!(mechanism_host("a", 1, "example.com"), "example.com");
        assert_eq!(mechanism_host("a/24", 1, "example.com"), "example.com");
        assert_eq!(mechanism_host("a:mail.example.net/24", 1, "example.com"), "mail.example.net");
        assert_eq!(mechanism_host("mx:mx.example.net.", 2, "example.com"), "mx.example.net");
        assert_eq!(mechanism_host("mx", 2, "example.com"), "example.com");
    }

    #[test]
    fn spf_detection_requires_exact_version_tag() {
        assert!(is_spf("v=spf1 -all"));
        assert!(is_spf("V=SPF1"));
        assert!(!is_spf("v=spf10 -all"));
        assert!(!is_spf("v=spf"));
    }
}
