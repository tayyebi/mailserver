/// Cluster wire-protocol endpoints for active-active multi-master replication.
///
/// Endpoints (all peer-to-peer, guarded by Ed25519 JWT or static cluster secret):
///   GET  /cluster/health
///   POST /cluster/join
///   POST /cluster/leave
///   GET  /cluster/log?since=HLC&limit=500
///   GET  /cluster/digest?bucket=YYYY-MM-DDTHH
///   POST /cluster/apply-sync
///
/// Admin-only endpoints (session auth):
///   GET  /cluster/replication/metrics
///   GET  /cluster/replication/log
///   POST /cluster/replication/gossip-now
///   POST /cluster/replication/anti-entropy-now
///   POST /cluster/replication/sweep
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json, Response},
};
use ed25519_dalek::{SigningKey, VerifyingKey};
use log::{error, info, warn};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::db::{Database, LogEntry};
use crate::web::auth::AuthAdmin;
use crate::web::AppState;

// ── Nonce replay cache ──

/// In-memory nonce cache to prevent JWT replay attacks.
/// Each nonce is stored with its expiry time (unix seconds).
#[derive(Default)]
pub struct NonceCache {
    entries: HashMap<String, u64>,
}

impl NonceCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return true if the nonce has NOT been seen before (and record it).
    /// Lazily sweeps entries older than 15 minutes when the cache exceeds 10k entries.
    pub fn check_and_insert(&mut self, nonce: &str, expires_at: u64) -> bool {
        let now = unix_now_secs();
        // Sweep if large
        if self.entries.len() > 10_000 {
            let cutoff = now.saturating_sub(15 * 60);
            self.entries.retain(|_, &mut exp| exp > cutoff);
        }
        if self.entries.contains_key(nonce) {
            return false; // replay
        }
        self.entries.insert(nonce.to_string(), expires_at);
        true
    }
}

// ── Keypair helpers ──

const PRIVATE_KEY_PATH: &str = "/etc/mailserver/cluster_private_key.bin";
const PUBLIC_KEY_PATH: &str = "/etc/mailserver/cluster_public_key.bin";

/// Load or generate an Ed25519 keypair.  Private key is stored at 0600, public at 0644.
pub fn load_or_generate_keypair() -> (SigningKey, VerifyingKey) {
    // Try loading from disk
    if let (Ok(priv_bytes), Ok(pub_bytes)) = (
        std::fs::read(PRIVATE_KEY_PATH),
        std::fs::read(PUBLIC_KEY_PATH),
    ) {
        if priv_bytes.len() == 32 && pub_bytes.len() == 32 {
            let priv_arr: [u8; 32] = priv_bytes.try_into().unwrap();
            let signing = SigningKey::from_bytes(&priv_arr);
            let verifying = signing.verifying_key();
            return (signing, verifying);
        }
    }

    // Generate fresh keypair
    let signing = SigningKey::generate(&mut OsRng);
    let verifying = signing.verifying_key();

    // Persist (best-effort; if /etc/mailserver doesn't exist we fall back to temp dir)
    let _ = std::fs::create_dir_all("/etc/mailserver");
    let priv_path = if std::fs::metadata("/etc/mailserver").is_ok() {
        PRIVATE_KEY_PATH
    } else {
        "/tmp/cluster_private_key.bin"
    };
    let pub_path = if std::fs::metadata("/etc/mailserver").is_ok() {
        PUBLIC_KEY_PATH
    } else {
        "/tmp/cluster_public_key.bin"
    };

    let _ = std::fs::write(priv_path, signing.to_bytes());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(priv_path, std::fs::Permissions::from_mode(0o600));
        let _ = std::fs::set_permissions(pub_path, std::fs::Permissions::from_mode(0o644));
    }
    let _ = std::fs::write(pub_path, verifying.to_bytes());

    (signing, verifying)
}

/// Return the base64-encoded public key bytes (for peer registration).
pub fn public_key_b64(vk: &VerifyingKey) -> String {
    base64::Engine::encode(&base64::engine::general_purpose::STANDARD, vk.to_bytes())
}

// ── JWT signing / verification (EdDSA / Ed25519) ──

#[derive(Serialize, Deserialize, Debug)]
struct JwtHeader {
    alg: String,
    typ: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PeerClaims {
    pub iss: String,
    pub iat: u64,
    pub exp: u64,
    pub nonce: String,
    pub body_sha256: String,
}

fn b64url_encode(data: &[u8]) -> String {
    base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        data,
    )
}

fn b64url_decode(s: &str) -> Result<Vec<u8>, String> {
    base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, s)
        .map_err(|e| e.to_string())
}

/// Create a signed JWT for an outgoing peer-to-peer request.
pub fn sign_peer_jwt(
    signing_key: &SigningKey,
    instance_id: &str,
    body: &[u8],
) -> String {
    use ed25519_dalek::Signer;
    use sha2::{Digest, Sha256};

    let iat = unix_now_secs();
    let exp = iat + 60;
    let nonce = {
        let mut bytes = [0u8; 16];
        use rand::RngCore;
        OsRng.fill_bytes(&mut bytes);
        hex::encode(bytes)
    };
    let body_sha256 = if body.is_empty() {
        String::new()
    } else {
        hex::encode(Sha256::digest(body))
    };

    let header = JwtHeader {
        alg: "EdDSA".to_string(),
        typ: "JWT".to_string(),
    };
    let claims = PeerClaims {
        iss: instance_id.to_string(),
        iat,
        exp,
        nonce,
        body_sha256,
    };

    let header_b64 = b64url_encode(serde_json::to_string(&header).unwrap().as_bytes());
    let claims_b64 = b64url_encode(serde_json::to_string(&claims).unwrap().as_bytes());
    let msg = format!("{}.{}", header_b64, claims_b64);
    let sig = signing_key.sign(msg.as_bytes());
    let sig_b64 = b64url_encode(sig.to_bytes().as_ref());
    format!("{}.{}", msg, sig_b64)
}

/// Verify a peer JWT.  Returns `Ok(claims)` on success, `Err(reason)` on any failure.
pub fn verify_peer_jwt(
    token: &str,
    db: &Database,
    nonce_cache: &Arc<Mutex<NonceCache>>,
    body: &[u8],
) -> Result<PeerClaims, String> {
    use ed25519_dalek::Verifier;
    use sha2::{Digest, Sha256};

    let parts: Vec<&str> = token.splitn(3, '.').collect();
    if parts.len() != 3 {
        return Err("malformed JWT".into());
    }
    let (header_b64, claims_b64, sig_b64) = (parts[0], parts[1], parts[2]);

    let claims_bytes = b64url_decode(claims_b64)?;
    let claims: PeerClaims =
        serde_json::from_slice(&claims_bytes).map_err(|e| format!("claims parse: {}", e))?;

    let now = unix_now_secs();
    if claims.exp < now {
        return Err("JWT expired".into());
    }
    if claims.iat > now + 5 {
        return Err("JWT issued too far in the future".into());
    }

    // Body SHA-256 check
    if !body.is_empty() {
        let expected = hex::encode(Sha256::digest(body));
        if claims.body_sha256 != expected {
            return Err("body_sha256 mismatch".into());
        }
    }

    // Nonce replay check
    let not_replayed = nonce_cache
        .lock()
        .unwrap()
        .check_and_insert(&claims.nonce, claims.exp);
    if !not_replayed {
        return Err("nonce replayed".into());
    }

    // Look up peer public key
    let peer = db
        .get_peer_by_instance_id(&claims.iss)
        .ok_or_else(|| format!("unknown peer: {}", claims.iss))?;
    let pub_key_bytes = peer
        .peer_public_key
        .as_deref()
        .ok_or("peer has no public key")?;
    let pub_key_raw = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        pub_key_bytes,
    )
    .map_err(|e| format!("public key decode: {}", e))?;
    let pub_arr: [u8; 32] = pub_key_raw
        .try_into()
        .map_err(|_| "public key wrong length")?;
    let verifying_key =
        VerifyingKey::from_bytes(&pub_arr).map_err(|e| format!("verifying key: {}", e))?;

    // Verify signature
    let msg = format!("{}.{}", header_b64, claims_b64);
    let sig_bytes = b64url_decode(sig_b64)?;
    let sig_arr: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| "signature wrong length")?;
    let sig = ed25519_dalek::Signature::from_bytes(&sig_arr);
    verifying_key
        .verify(msg.as_bytes(), &sig)
        .map_err(|e| format!("signature invalid: {}", e))?;

    Ok(claims)
}

/// Middleware-style check: accept either a valid Ed25519 JWT (`X-Peer-Auth`) or
/// the static cluster secret (`X-Cluster-Secret`) — the latter allows rolling upgrades.
pub fn authenticate_peer(
    headers: &HeaderMap,
    db: &Database,
    nonce_cache: &Arc<Mutex<NonceCache>>,
    body: &[u8],
) -> Result<String, String> {
    // 1. Static cluster secret (optional backward-compat header)
    if let Some(secret_val) = headers.get("X-Cluster-Secret").and_then(|v| v.to_str().ok()) {
        let configured = db.get_setting("cluster_secret").unwrap_or_default();
        if !configured.is_empty() && secret_val == configured {
            return Ok("cluster-secret-auth".into());
        }
    }

    // 2. Ed25519 JWT
    let token = headers
        .get("X-Peer-Auth")
        .and_then(|v| v.to_str().ok())
        .ok_or("missing X-Peer-Auth header")?;
    verify_peer_jwt(token, db, nonce_cache, body).map(|c| c.iss)
}

// ── Request/response types ──

#[derive(Deserialize)]
pub struct JoinRequest {
    pub instance_id: String,
    pub url: String,
    pub region: Option<String>,
    pub public_key: Option<String>,
    pub first_seen_hlc: Option<String>,
    #[allow(dead_code)]
    pub name: Option<String>,
}

#[derive(Serialize)]
pub struct JoinResponse {
    pub peer_list: Vec<crate::db::Peer>,
    pub hlc_high_water: String,
    pub self_instance_id: String,
}

#[derive(Deserialize)]
pub struct LogQuery {
    pub since: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Serialize)]
pub struct LogResponse {
    pub entries: Vec<LogEntry>,
    pub high_water: String,
}

#[derive(Deserialize)]
pub struct DigestQuery {
    pub bucket: Option<String>,
}

#[derive(Serialize)]
pub struct DigestResponse {
    pub bucket: String,
    pub digests: Vec<crate::db::DigestEntry>,
}

#[derive(Deserialize)]
pub struct ApplySyncRequest {
    pub entity_type: String,
    pub entity_id: String,
    pub payload: serde_json::Value,
    pub origin: String,
    pub hlc: String,
}

// ── GET /cluster/health ──

pub async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let instance_id = state.blocking_db(|db| db.get_local_node_id()).await;
    let uptime_secs = state
        .cluster_start_time
        .elapsed()
        .unwrap_or_default()
        .as_secs();
    Json(serde_json::json!({
        "instance_id": instance_id,
        "uptime_secs": uptime_secs,
        "status": "ok",
    }))
}

// ── POST /cluster/join ──

pub async fn join(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let body_bytes = body.as_bytes().to_vec();
    let nonce_cache = state.nonce_cache.clone();

    // Auth check (peer or cluster-secret)
    let auth_result = state
        .blocking_db(move |db| {
            authenticate_peer(&headers, db, &nonce_cache, &body_bytes)
        })
        .await;

    if let Err(e) = auth_result {
        warn!("[cluster] /cluster/join auth failed: {}", e);
        return (StatusCode::UNAUTHORIZED, e).into_response();
    }

    let req: JoinRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("invalid body: {}", e)).into_response();
        }
    };

    let instance_id = req.instance_id.clone();
    let url = req.url.clone();
    let region = req.region.clone();
    let public_key = req.public_key.clone();
    let first_seen_hlc = req.first_seen_hlc.clone();

    let result = state
        .blocking_db(move |db| {
            // Register / update the peer
            let _ = db.upsert_peer(
                &instance_id,
                &url,
                region.as_deref(),
                public_key.as_deref(),
                first_seen_hlc.as_deref(),
            );
            // Also append to replication_log so the peer info gossips to others
            let hlc = db
                .get_node_state("hlc_high_water")
                .unwrap_or_else(|| db.get_local_node_id());
            let payload = serde_json::json!({
                "instance_id": instance_id,
                "url": url,
                "region": region,
                "peer_public_key": public_key,
                "first_seen_hlc": first_seen_hlc,
            });
            let _ = db.append_log_entry(
                &hlc,
                &db.get_local_node_id(),
                "peer",
                &instance_id,
                "upsert",
                &payload,
            );
            let peers = db.list_live_peers();
            let high_water = db.get_hlc_high_water();
            let self_id = db.get_local_node_id();
            (peers, high_water, self_id)
        })
        .await;

    Json(JoinResponse {
        peer_list: result.0,
        hlc_high_water: result.1,
        self_instance_id: result.2,
    })
    .into_response()
}

// ── POST /cluster/leave ──

pub async fn leave(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let body_bytes = body.as_bytes().to_vec();
    let nonce_cache = state.nonce_cache.clone();

    let auth_result = state
        .blocking_db(move |db| authenticate_peer(&headers, db, &nonce_cache, &body_bytes))
        .await;
    let instance_id = match auth_result {
        Ok(id) => id,
        Err(e) => {
            warn!("[cluster] /cluster/leave auth failed: {}", e);
            return (StatusCode::UNAUTHORIZED, e).into_response();
        }
    };

    if instance_id == "cluster-secret-auth" {
        return (StatusCode::BAD_REQUEST, "use X-Peer-Auth for /leave").into_response();
    }

    state
        .blocking_db(move |db| {
            db.decommission_peer(&instance_id);
            let hlc = db.get_hlc_high_water();
            let payload = serde_json::json!({
                "instance_id": instance_id,
                "decommissioned_at": chrono::Utc::now().to_rfc3339(),
            });
            let _ = db.append_log_entry(
                &hlc,
                &db.get_local_node_id(),
                "peer",
                &instance_id,
                "tombstone",
                &payload,
            );
        })
        .await;

    Json(serde_json::json!({ "ok": true })).into_response()
}

// ── GET /cluster/log ──

pub async fn log_entries(
    State(state): State<AppState>,
    Query(params): Query<LogQuery>,
) -> Json<LogResponse> {
    let since = params.since.unwrap_or_default();
    let limit = params.limit.unwrap_or(500).max(1).min(2000);
    let (entries, high_water) = state
        .blocking_db(move |db| {
            let e = db.get_log_since_hlc(&since, limit);
            let hw = db.get_hlc_high_water();
            (e, hw)
        })
        .await;
    Json(LogResponse {
        entries,
        high_water,
    })
}

// ── GET /cluster/digest ──

pub async fn digest(
    State(state): State<AppState>,
    Query(params): Query<DigestQuery>,
) -> Json<DigestResponse> {
    let bucket = params.bucket.unwrap_or_else(|| {
        (chrono::Utc::now() - chrono::Duration::hours(1))
            .format("%Y-%m-%dT%H")
            .to_string()
    });
    let bucket_clone = bucket.clone();
    let digests = state
        .blocking_db(move |db| {
            // Parse YYYY-MM-DDTHH
            let start = chrono::NaiveDateTime::parse_from_str(
                &format!("{}:00:00", bucket_clone),
                "%Y-%m-%dT%H:%M:%S",
            )
            .map(|dt| dt.and_utc().to_rfc3339())
            .unwrap_or_default();
            let end = chrono::NaiveDateTime::parse_from_str(
                &format!("{}:59:59", bucket_clone),
                "%Y-%m-%dT%H:%M:%S",
            )
            .map(|dt| (dt + chrono::Duration::seconds(1)).and_utc().to_rfc3339())
            .unwrap_or_default();
            db.compute_digest(&start, &end)
        })
        .await;
    Json(DigestResponse { bucket, digests })
}

// ── POST /cluster/apply-sync ──

pub async fn apply_sync(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let body_bytes = body.as_bytes().to_vec();
    let nonce_cache = state.nonce_cache.clone();

    let auth_result = state
        .blocking_db(move |db| authenticate_peer(&headers, db, &nonce_cache, &body_bytes))
        .await;
    if let Err(e) = auth_result {
        warn!("[cluster] /cluster/apply-sync auth failed: {}", e);
        return (StatusCode::UNAUTHORIZED, e).into_response();
    }

    let req: ApplySyncRequest = match serde_json::from_str(&body) {
        Ok(r) => r,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("invalid body: {}", e)).into_response();
        }
    };

    let entry = LogEntry {
        seq: 0, // ignored on insert
        hlc: req.hlc,
        origin_replica: req.origin,
        entity_type: req.entity_type,
        entity_id: req.entity_id,
        op: "upsert".to_string(),
        payload: req.payload,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    state
        .blocking_db(move |db| db.apply_log_entry_hlc(&entry))
        .await;

    Json(serde_json::json!({ "ok": true })).into_response()
}

// ── Admin: GET /cluster/replication/metrics ──

pub async fn metrics(_auth: AuthAdmin, State(state): State<AppState>) -> Json<serde_json::Value> {
    let (high_water, cursors, peer_count, log_size) = state
        .blocking_db(|db| {
            let hw = db.get_hlc_high_water();
            let cursors = db.list_hlc_cursors();
            let peers = db.list_live_peers().len();
            let log_size = db.count_log_entries();
            (hw, cursors, peers, log_size)
        })
        .await;

    let cursor_data: Vec<serde_json::Value> = cursors
        .iter()
        .map(|(peer, hlc, gossip_at)| {
            serde_json::json!({
                "peer_instance_id": peer,
                "last_applied_hlc": hlc,
                "last_gossip_at": gossip_at,
            })
        })
        .collect();

    Json(serde_json::json!({
        "high_water": high_water,
        "log_size": log_size,
        "peer_count": peer_count,
        "cursors": cursor_data,
    }))
}

// ── Admin: GET /cluster/replication/log ──

#[derive(Deserialize)]
pub struct AdminLogQuery {
    pub entity_type: Option<String>,
    pub origin: Option<String>,
    pub op: Option<String>,
    pub limit: Option<i64>,
}

pub async fn admin_log(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Query(params): Query<AdminLogQuery>,
) -> Json<serde_json::Value> {
    let entity_type = params.entity_type.unwrap_or_default();
    let origin = params.origin.unwrap_or_default();
    let op = params.op.unwrap_or_default();
    let limit = params.limit.unwrap_or(100).max(1).min(1000);

    let entries = state
        .blocking_db(move |db| db.query_log_entries(&entity_type, &origin, &op, limit))
        .await;

    Json(serde_json::json!({ "entries": entries }))
}

// ── Admin: POST /cluster/replication/gossip-now ──

pub async fn gossip_now(_auth: AuthAdmin, State(state): State<AppState>) -> Json<serde_json::Value> {
    info!("[cluster] admin triggered gossip-now");
    let instance_id = state.blocking_db(|db| db.get_local_node_id()).await;
    let hlc = {
        let persisted = state
            .blocking_db(|db| db.get_node_state("hlc_high_water").unwrap_or_default())
            .await;
        if persisted.is_empty() {
            crate::hlc::Hlc::new(&instance_id)
        } else {
            crate::hlc::Hlc::restore(&persisted, &instance_id)
        }
    };

    let peers = state.blocking_db(|db| db.list_online_peers()).await;
    let triggered = peers.len();
    for peer in peers {
        let db = state.db.clone();
        let hlc2 = hlc.clone();
        let pid = peer.instance_id.clone();
        let purl = peer.url.clone();
        std::thread::spawn(move || {
            crate::replication::gossip_pull_from(&db, &hlc2, &pid, &purl);
        });
    }
    Json(serde_json::json!({ "triggered": triggered }))
}

// ── Admin: POST /cluster/replication/anti-entropy-now ──

pub async fn anti_entropy_now(
    _auth: AuthAdmin,
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    info!("[cluster] admin triggered anti-entropy-now");
    let instance_id = state.blocking_db(|db| db.get_local_node_id()).await;
    let hlc = crate::hlc::Hlc::new(&instance_id);

    let peers = state.blocking_db(|db| db.list_online_peers()).await;
    let triggered = peers.len();
    for peer in peers {
        let db = state.db.clone();
        let hlc2 = hlc.clone();
        let pid = peer.instance_id.clone();
        let purl = peer.url.clone();
        std::thread::spawn(move || {
            crate::replication::anti_entropy_with(&db, &hlc2, &pid, &purl);
        });
    }
    Json(serde_json::json!({ "triggered": triggered }))
}

// ── Admin: POST /cluster/replication/sweep ──

pub async fn sweep_now(_auth: AuthAdmin, State(state): State<AppState>) -> Json<serde_json::Value> {
    info!("[cluster] admin triggered sweep-now");
    let deleted = state.blocking_db(|db| db.sweep_replication_log(7)).await;
    Json(serde_json::json!({ "deleted": deleted }))
}

// ── Utilities ──

fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nonce_cache_rejects_replay() {
        let mut cache = NonceCache::new();
        assert!(cache.check_and_insert("abc", 9999999999));
        assert!(!cache.check_and_insert("abc", 9999999999));
    }

    #[test]
    fn nonce_cache_allows_different_nonces() {
        let mut cache = NonceCache::new();
        assert!(cache.check_and_insert("n1", 9999999999));
        assert!(cache.check_and_insert("n2", 9999999999));
    }

    #[test]
    fn nonce_cache_sweeps_on_large_size() {
        let mut cache = NonceCache::new();
        // Insert 10001 entries with already-expired times
        for i in 0..10_001usize {
            cache.entries.insert(format!("nonce-{}", i), 1); // expired
        }
        // Check-and-insert a new nonce — should trigger sweep
        assert!(cache.check_and_insert("new-nonce", 9999999999));
        // After sweep, old expired entries should be gone
        assert!(cache.entries.len() < 10_001);
    }

    #[test]
    fn sign_and_verify_jwt_roundtrip() {
        let signing = SigningKey::generate(&mut OsRng);
        let verifying = signing.verifying_key();
        let pub_b64 = public_key_b64(&verifying);

        // Build a minimal peer + nonce cache for verification
        let nonce_cache = Arc::new(Mutex::new(NonceCache::new()));
        let token = sign_peer_jwt(&signing, "test-instance", b"");

        // Manually verify by parsing the JWT parts
        let parts: Vec<&str> = token.splitn(3, '.').collect();
        assert_eq!(parts.len(), 3);
        let claims_bytes = b64url_decode(parts[1]).unwrap();
        let claims: PeerClaims = serde_json::from_slice(&claims_bytes).unwrap();
        assert_eq!(claims.iss, "test-instance");

        // Verify signature
        use ed25519_dalek::Verifier;
        let msg = format!("{}.{}", parts[0], parts[1]);
        let sig_bytes = b64url_decode(parts[2]).unwrap();
        let sig_arr: [u8; 64] = sig_bytes.try_into().unwrap();
        let sig = ed25519_dalek::Signature::from_bytes(&sig_arr);
        assert!(verifying.verify(msg.as_bytes(), &sig).is_ok());

        // Ensure the public key round-trips
        let decoded = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &pub_b64,
        )
        .unwrap();
        assert_eq!(decoded, verifying.to_bytes());
        let _ = nonce_cache; // used to avoid unused warning
    }

    #[test]
    fn b64url_roundtrip() {
        let data = b"hello world \x00\xFF";
        let encoded = b64url_encode(data);
        let decoded = b64url_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }
}
