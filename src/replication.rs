use data_encoding::HEXLOWER;
use hmac::{Hmac, Mac};
use log::{debug, error, info, warn};
use sha2::Sha256;
use std::time::Duration;

use crate::db::{ChangeLogEntry, Database};

/// Interval between outbound push attempts per peer.
const PUSH_INTERVAL_SECS: u64 = 5;

/// Maximum changelog entries per push batch.
const BATCH_SIZE: i64 = 200;

/// HTTP request timeout for replication pushes.
const REQUEST_TIMEOUT_SECS: u64 = 10;

/// Spawn the outbound replication loop in a background thread.
///
/// The loop wakes up every [`PUSH_INTERVAL_SECS`] seconds, inspects all
/// active peer nodes, and pushes any unsent changelog entries.
pub fn start(db: Database) {
    std::thread::spawn(move || {
        info!("[repl] outbound replication service started");
        loop {
            run_one_cycle(&db);
            std::thread::sleep(Duration::from_secs(PUSH_INTERVAL_SECS));
        }
    });
}

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
    // Constant-time comparison
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
}
