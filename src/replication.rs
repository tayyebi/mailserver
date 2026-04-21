use data_encoding::HEXLOWER;
use hmac::{Hmac, Mac};
use log::{debug, error, info, warn};
use sha2::Sha256;
use std::time::Duration;

use crate::db::{ChangeLogEntry, Database, LogEntry};
use crate::hlc::Hlc;

// ── Legacy push-replication constants (kept for backward compatibility) ──

/// Interval between outbound push attempts per peer.
const PUSH_INTERVAL_SECS: u64 = 5;

/// Maximum changelog entries per push batch.
const BATCH_SIZE: i64 = 200;

/// HTTP request timeout for replication pushes.
const REQUEST_TIMEOUT_SECS: u64 = 10;

// ── HLC replication loop constants ──

/// How often the gossip-pull loop wakes up.
const GOSSIP_INTERVAL_SECS: u64 = 5;

/// How often the anti-entropy loop runs.
const ANTI_ENTROPY_INTERVAL_SECS: u64 = 60;

/// How often the log sweeper runs.
const SWEEP_INTERVAL_SECS: u64 = 3600;

/// How often the peer-health probes run.
const HEALTH_PROBE_INTERVAL_SECS: u64 = 60;

/// How often the HLC state is persisted to the DB.
const HLC_PERSIST_INTERVAL_SECS: u64 = 5;

/// Maximum log entries pulled per gossip batch.
const GOSSIP_BATCH_LIMIT: i64 = 500;

/// Log retention in days.
const LOG_RETENTION_DAYS: i64 = 7;

// ── Public API ──

/// Start the legacy outbound push-replication loop **and** all HLC-based
/// background loops (gossip pull, anti-entropy, log sweeper, peer health, HLC persist).
pub fn start(db: Database) {
    // Legacy outbound push loop (backward-compatible with static peer config)
    {
        let db2 = db.clone();
        std::thread::spawn(move || {
            info!("[repl] legacy outbound replication service started");
            loop {
                run_one_cycle(&db2);
                std::thread::sleep(Duration::from_secs(PUSH_INTERVAL_SECS));
            }
        });
    }

    // HLC-based loops
    start_hlc_loops(db);
}

/// Start only the HLC-based replication loops (gossip pull, anti-entropy, sweeper,
/// peer health, HLC persist). Called from `start()` and optionally from tests/main.
pub fn start_hlc_loops(db: Database) {
    let instance_id = db.get_local_node_id();
    let hlc = {
        let persisted = db.get_node_state("hlc_high_water").unwrap_or_default();
        if persisted.is_empty() {
            Hlc::new(&instance_id)
        } else {
            Hlc::restore(&persisted, &instance_id)
        }
    };

    // HLC-persist loop
    {
        let db2 = db.clone();
        let hlc2 = hlc.clone();
        std::thread::spawn(move || {
            info!("[hlc] persist loop started");
            loop {
                std::thread::sleep(Duration::from_secs(HLC_PERSIST_INTERVAL_SECS));
                let current = hlc2.peek();
                db2.set_node_state("hlc_high_water", &current);
                debug!("[hlc] persisted high-water={}", current);
            }
        });
    }

    // Gossip-pull loop
    {
        let db2 = db.clone();
        let hlc2 = hlc.clone();
        std::thread::spawn(move || {
            info!("[repl] gossip-pull loop started");
            loop {
                std::thread::sleep(Duration::from_secs(GOSSIP_INTERVAL_SECS));
                run_gossip_pull(&db2, &hlc2);
            }
        });
    }

    // Anti-entropy loop
    {
        let db2 = db.clone();
        let hlc2 = hlc.clone();
        std::thread::spawn(move || {
            info!("[repl] anti-entropy loop started");
            loop {
                std::thread::sleep(Duration::from_secs(ANTI_ENTROPY_INTERVAL_SECS));
                run_anti_entropy(&db2, &hlc2);
            }
        });
    }

    // Log-sweeper loop
    {
        let db2 = db.clone();
        std::thread::spawn(move || {
            info!("[repl] log-sweeper loop started");
            loop {
                std::thread::sleep(Duration::from_secs(SWEEP_INTERVAL_SECS));
                run_sweep(&db2);
            }
        });
    }

    // Peer-health probe loop
    {
        let db2 = db.clone();
        let hlc2 = hlc.clone();
        let iid = instance_id.clone();
        std::thread::spawn(move || {
            info!("[repl] peer-health probe loop started");
            loop {
                std::thread::sleep(Duration::from_secs(HEALTH_PROBE_INTERVAL_SECS));
                probe_peer_health(&db2, &hlc2, &iid);
            }
        });
    }
}

// ── HLC gossip pull ──

/// Pick one uniformly random online peer and pull any log entries newer than the
/// local cursor for that peer.
fn run_gossip_pull(db: &Database, hlc: &Hlc) {
    let peers = db.list_online_peers();
    if peers.is_empty() {
        return;
    }
    let peer = &peers[rand_index(peers.len())];
    gossip_pull_from(db, hlc, &peer.instance_id, &peer.url);
}

/// Pull log entries from a specific peer and apply them.
pub fn gossip_pull_from(db: &Database, hlc: &Hlc, peer_instance_id: &str, peer_url: &str) {
    let cursor = db.get_hlc_cursor(peer_instance_id);
    let url = format!(
        "{}/cluster/log?since={}&limit={}",
        peer_url.trim_end_matches('/'),
        urlencoding_simple(&cursor),
        GOSSIP_BATCH_LIMIT
    );

    debug!(
        "[repl] gossip pull from {} since='{}' url={}",
        peer_instance_id, cursor, url
    );

    let client = match build_http_client() {
        Some(c) => c,
        None => return,
    };

    let resp = match client.get(&url).send() {
        Ok(r) => r,
        Err(e) => {
            warn!("[repl] gossip pull from {} failed: {}", peer_instance_id, e);
            return;
        }
    };

    if !resp.status().is_success() {
        warn!(
            "[repl] gossip pull from {} returned HTTP {}",
            peer_instance_id,
            resp.status()
        );
        return;
    }

    let body = match resp.text() {
        Ok(b) => b,
        Err(e) => {
            error!("[repl] gossip pull from {}: failed to read body: {}", peer_instance_id, e);
            return;
        }
    };

    #[derive(serde::Deserialize)]
    struct LogResponse {
        entries: Vec<LogEntry>,
        high_water: String,
    }

    let parsed: LogResponse = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            error!("[repl] gossip pull from {}: failed to parse response: {}", peer_instance_id, e);
            return;
        }
    };

    if parsed.entries.is_empty() {
        return;
    }

    // Sort defensively by HLC before applying
    let mut entries = parsed.entries;
    entries.sort_by(|a, b| a.hlc.cmp(&b.hlc));

    let last_hlc = entries.last().map(|e| e.hlc.clone()).unwrap_or_default();
    let count = entries.len();

    // Apply in a single transaction-like batch (abort on first error)
    for entry in &entries {
        if let Err(e) = hlc.update(&entry.hlc) {
            warn!("[repl] gossip pull from {}: HLC update rejected: {}", peer_instance_id, e);
            // Don't abort — just skip advancing the HLC for this entry
        }
        db.apply_log_entry_hlc(entry);
    }

    // Advance cursor
    db.set_hlc_cursor(peer_instance_id, &last_hlc);
    info!(
        "[repl] gossip pull from {}: applied {} entries, cursor='{}'",
        peer_instance_id, count, last_hlc
    );
}

// ── Anti-entropy ──

fn run_anti_entropy(db: &Database, hlc: &Hlc) {
    let peers = db.list_online_peers();
    if peers.is_empty() {
        return;
    }
    let peer = &peers[rand_index(peers.len())];
    anti_entropy_with(db, hlc, &peer.instance_id, &peer.url);
}

/// Compare digests for the previous hour with a peer; pull missing entries on mismatch.
pub fn anti_entropy_with(db: &Database, hlc: &Hlc, peer_instance_id: &str, peer_url: &str) {
    use chrono::Timelike;
    // Use previous complete hour
    let now = chrono::Utc::now();
    // Truncate to the start of the current hour, then subtract one hour
    let current_hour_start = now
        .with_minute(0)
        .and_then(|t| t.with_second(0))
        .and_then(|t| t.with_nanosecond(0))
        .unwrap_or(now);
    let bucket_start = current_hour_start - chrono::Duration::hours(1);
    let bucket_end = current_hour_start;
    let bucket_label = bucket_start.format("%Y-%m-%dT%H").to_string();
    let bucket_start_str = bucket_start.to_rfc3339();
    let bucket_end_str = bucket_end.to_rfc3339();

    let local_digests = db.compute_digest(&bucket_start_str, &bucket_end_str);

    let url = format!(
        "{}/cluster/digest?bucket={}",
        peer_url.trim_end_matches('/'),
        urlencoding_simple(&bucket_label)
    );

    let client = match build_http_client() {
        Some(c) => c,
        None => return,
    };

    let resp = match client.get(&url).send() {
        Ok(r) => r,
        Err(e) => {
            warn!("[repl] anti-entropy with {}: digest request failed: {}", peer_instance_id, e);
            return;
        }
    };

    if !resp.status().is_success() {
        warn!(
            "[repl] anti-entropy with {}: digest returned HTTP {}",
            peer_instance_id,
            resp.status()
        );
        return;
    }

    #[derive(serde::Deserialize)]
    struct DigestResponse {
        digests: Vec<crate::db::DigestEntry>,
    }

    let body = match resp.text() {
        Ok(b) => b,
        Err(e) => {
            error!("[repl] anti-entropy with {}: failed to read body: {}", peer_instance_id, e);
            return;
        }
    };
    let peer_resp: DigestResponse = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            error!("[repl] anti-entropy with {}: parse error: {}", peer_instance_id, e);
            return;
        }
    };

    // Compare digests per entity_type
    for peer_d in &peer_resp.digests {
        let local_d = local_digests.iter().find(|d| d.entity_type == peer_d.entity_type);
        let mismatch = match local_d {
            None => true,
            Some(ld) => ld.count != peer_d.count || ld.xor_hash != peer_d.xor_hash,
        };
        if mismatch {
            warn!(
                "[repl] anti-entropy with {}: mismatch in entity_type='{}' — pulling bucket",
                peer_instance_id, peer_d.entity_type
            );
            // Pull entries for this bucket
            pull_bucket(db, hlc, peer_url, peer_instance_id, &bucket_start_str, &bucket_end_str);
            // Only pull once per anti-entropy cycle even if multiple entity types mismatch
            break;
        }
    }
}

fn pull_bucket(
    db: &Database,
    hlc: &Hlc,
    peer_url: &str,
    peer_instance_id: &str,
    bucket_start: &str,
    bucket_end: &str,
) {
    // Convert bucket_start to an HLC-prefix (physical-time component only)
    let since_ms: u64 = chrono::DateTime::parse_from_rfc3339(bucket_start)
        .map(|dt| dt.timestamp_millis() as u64)
        .unwrap_or(0);
    let since_hlc = format!("{:013}-000000-", since_ms);

    let url = format!(
        "{}/cluster/log?since={}&limit=2000",
        peer_url.trim_end_matches('/'),
        urlencoding_simple(&since_hlc)
    );

    let client = match build_http_client() {
        Some(c) => c,
        None => return,
    };

    let resp = match client.get(&url).send() {
        Ok(r) => r,
        Err(e) => {
            warn!("[repl] pull_bucket from {}: request failed: {}", peer_instance_id, e);
            return;
        }
    };

    if !resp.status().is_success() {
        warn!(
            "[repl] pull_bucket from {}: HTTP {}",
            peer_instance_id,
            resp.status()
        );
        return;
    }

    #[derive(serde::Deserialize)]
    struct LogResponse {
        entries: Vec<LogEntry>,
        #[allow(dead_code)]
        high_water: String,
    }

    let body = match resp.text() {
        Ok(b) => b,
        Err(e) => {
            error!("[repl] pull_bucket from {}: failed to read body: {}", peer_instance_id, e);
            return;
        }
    };
    let parsed: LogResponse = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            error!("[repl] pull_bucket from {}: parse error: {}", peer_instance_id, e);
            return;
        }
    };

    // Filter to only entries whose created_at falls in the bucket window
    let bucket_end_ms: u64 = chrono::DateTime::parse_from_rfc3339(bucket_end)
        .map(|dt| dt.timestamp_millis() as u64)
        .unwrap_or(u64::MAX);

    let mut reconciled = 0usize;
    for entry in &parsed.entries {
        let entry_ms = crate::hlc::physical_ms(&entry.hlc).unwrap_or(0);
        if entry_ms >= since_ms && entry_ms < bucket_end_ms {
            let _ = hlc.update(&entry.hlc);
            db.apply_log_entry_hlc(entry);
            reconciled += 1;
        }
    }

    info!(
        "[repl] anti-entropy with {}: reconciled {} entries from bucket {}",
        peer_instance_id, reconciled, bucket_start
    );
}

// ── Log sweeper ──

fn run_sweep(db: &Database) {
    let deleted = db.sweep_replication_log(LOG_RETENTION_DAYS);
    if deleted > 0 {
        info!("[repl] log sweeper: deleted {} old entries", deleted);
    } else {
        debug!("[repl] log sweeper: nothing to delete");
    }
}

// ── Peer health probes ──

fn probe_peer_health(db: &Database, _hlc: &Hlc, _self_instance_id: &str) {
    let peers = db.list_live_peers();
    for peer in peers {
        let url = format!("{}/cluster/health", peer.url.trim_end_matches('/'));
        let client = match build_http_client() {
            Some(c) => c,
            None => continue,
        };
        let ok = client.get(&url).send().map(|r| r.status().is_success()).unwrap_or(false);
        let prev_level = peer.suspicion_level;
        let new_level = if ok {
            (prev_level - 1).max(0)
        } else {
            (prev_level + 1).min(20)
        };
        let new_status = if new_level >= 5 {
            "offline"
        } else if new_level >= 3 {
            "suspect"
        } else {
            "online"
        };

        // Log transitions
        let prev_status = if prev_level >= 5 {
            "offline"
        } else if prev_level >= 3 {
            "suspect"
        } else {
            "online"
        };
        if new_status != prev_status {
            info!(
                "[repl] peer {} transitioned {} -> {}",
                peer.instance_id, prev_status, new_status
            );
        }

        db.update_peer_health(&peer.instance_id, new_level, new_status, ok);
    }
}

// ── Legacy push-replication ──

fn run_one_cycle(db: &Database) {
    let nodes = db.list_replica_nodes();
    let active: Vec<_> = nodes.into_iter().filter(|n| n.active).collect();
    if active.is_empty() {
        return;
    }

    let this_node = db.get_local_node_id();

    for node in active {
        push_to_peer(db, &node, &this_node);
    }
}

fn push_to_peer(db: &Database, node: &crate::db::ReplicaNode, this_node_id: &str) {
    let checkpoint = db.get_replication_checkpoint(&node.node_id);
    let entries = db.get_changelog_since(checkpoint, BATCH_SIZE);
    if entries.is_empty() {
        return;
    }

    let last_id = entries.last().map(|e| e.id).unwrap_or(checkpoint);
    let payload = match serde_json::to_string(&entries) {
        Ok(s) => s,
        Err(e) => {
            error!("[repl] failed to serialize changelog batch: {}", e);
            return;
        }
    };

    let sig = compute_hmac_sha256(&node.shared_secret, &payload);
    let url = format!("{}/replication/apply", node.peer_url.trim_end_matches('/'));

    debug!(
        "[repl] pushing {} entries to {} (checkpoint {} -> {})",
        entries.len(),
        node.node_id,
        checkpoint,
        last_id
    );

    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            error!("[repl] failed to build HTTP client: {}", e);
            return;
        }
    };

    match client
        .post(&url)
        .header("Content-Type", "application/json")
        .header("X-Replication-Sig", &sig)
        .header("X-Replication-From", this_node_id)
        .body(payload)
        .send()
    {
        Ok(resp) if resp.status().is_success() => {
            info!(
                "[repl] successfully pushed {} entries to {} (new checkpoint: {})",
                entries.len(),
                node.node_id,
                last_id
            );
            db.set_replication_checkpoint(&node.node_id, last_id);
            db.touch_replica_node(node.id);
        }
        Ok(resp) => {
            warn!(
                "[repl] peer {} returned HTTP {} — will retry",
                node.node_id,
                resp.status()
            );
        }
        Err(e) => {
            warn!(
                "[repl] failed to push to peer {} ({}): {}",
                node.node_id, url, e
            );
        }
    }
}

// ── Utilities ──

/// Compute HMAC-SHA256(key, data) and return the lowercase hex digest.
pub fn compute_hmac_sha256(key: &str, data: &str) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(key.as_bytes()).expect("HMAC accepts any key length");
    mac.update(data.as_bytes());
    let result = mac.finalize();
    HEXLOWER.encode(&result.into_bytes())
}

/// Verify an HMAC-SHA256 signature in constant time.
pub fn verify_hmac_sha256(key: &str, data: &str, expected_hex: &str) -> bool {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(key.as_bytes()).expect("HMAC accepts any key length");
    mac.update(data.as_bytes());
    let computed = HEXLOWER.encode(&mac.finalize().into_bytes());
    constant_time_eq(computed.as_bytes(), expected_hex.as_bytes())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Deserialize a JSON batch of [`ChangeLogEntry`] from a request body.
pub fn parse_entries(body: &str) -> Result<Vec<ChangeLogEntry>, String> {
    serde_json::from_str(body).map_err(|e| e.to_string())
}

fn build_http_client() -> Option<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
        .map_err(|e| {
            error!("[repl] failed to build HTTP client: {}", e);
        })
        .ok()
}

/// Minimal percent-encoding for use in query-string values.
/// Only encodes characters that would break a URL query parameter.
fn urlencoding_simple(s: &str) -> String {
    s.replace('%', "%25")
        .replace('&', "%26")
        .replace('+', "%2B")
        .replace(' ', "%20")
        .replace('#', "%23")
}

/// Pick a uniformly random index in [0, len) using a proper RNG.
fn rand_index(len: usize) -> usize {
    use rand::Rng;
    rand::thread_rng().gen_range(0..len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_compute_and_verify_match() {
        let sig = compute_hmac_sha256("secret", "hello world");
        assert!(verify_hmac_sha256("secret", "hello world", &sig));
    }

    #[test]
    fn hmac_verify_rejects_wrong_key() {
        let sig = compute_hmac_sha256("secret", "hello world");
        assert!(!verify_hmac_sha256("wrong", "hello world", &sig));
    }

    #[test]
    fn hmac_verify_rejects_wrong_data() {
        let sig = compute_hmac_sha256("secret", "hello world");
        assert!(!verify_hmac_sha256("secret", "hello world!", &sig));
    }

    #[test]
    fn hmac_verify_rejects_tampered_signature() {
        let mut sig = compute_hmac_sha256("secret", "data");
        sig.push('0'); // extend to different length
        assert!(!verify_hmac_sha256("secret", "data", &sig));
    }

    #[test]
    fn parse_entries_empty_array() {
        let entries = parse_entries("[]").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_entries_invalid_json_returns_err() {
        assert!(parse_entries("{bad json}").is_err());
    }

    #[test]
    fn urlencoding_simple_encodes_special_chars() {
        assert_eq!(urlencoding_simple("a b"), "a%20b");
        assert_eq!(urlencoding_simple("a&b"), "a%26b");
        assert_eq!(urlencoding_simple("a+b"), "a%2Bb");
        assert_eq!(urlencoding_simple(""), "");
    }

    #[test]
    fn rand_index_in_bounds() {
        for len in 1..=10 {
            let idx = rand_index(len);
            assert!(idx < len, "rand_index({}) = {} out of bounds", len, idx);
        }
    }
}

