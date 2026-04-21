use askama::Template;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    Form,
};
use log::{error, info, warn};

use crate::replication;
use crate::web::auth::AuthAdmin;
use crate::web::forms::ReplicaNodeForm;
use crate::web::AppState;

// ── Templates ──

#[derive(Template)]
#[template(path = "replicas/list.html")]
struct ListTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    nodes: Vec<crate::db::ReplicaNode>,
    stats: Vec<crate::db::ReplicationStat>,
    this_node_id: String,
}

#[derive(Template)]
#[template(path = "replicas/new.html")]
struct NewTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    this_node_id: String,
}

// ── Admin handlers ──

pub async fn list(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    info!("[web] GET /replicas — listing replica nodes");
    let (nodes, stats, this_node_id) = state
        .blocking_db(|db| {
            (
                db.list_replica_nodes(),
                db.get_replication_stats(),
                db.get_local_node_id(),
            )
        })
        .await;
    let tmpl = ListTemplate {
        nav_active: "Replication",
        flash: None,
        nodes,
        stats,
        this_node_id,
    };
    Html(tmpl.render().unwrap())
}

pub async fn new_form(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    let this_node_id = state.blocking_db(|db| db.get_local_node_id()).await;
    let tmpl = NewTemplate {
        nav_active: "Replication",
        flash: None,
        this_node_id,
    };
    Html(tmpl.render().unwrap())
}

pub async fn create(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<ReplicaNodeForm>,
) -> Response {
    let node_id = form.node_id.trim().to_string();
    let peer_url = form.peer_url.trim().to_string();
    let shared_secret = form.shared_secret.trim().to_string();

    if node_id.is_empty() || peer_url.is_empty() || shared_secret.is_empty() {
        let (nodes, stats, this_node_id) = state
            .blocking_db(|db| {
                (
                    db.list_replica_nodes(),
                    db.get_replication_stats(),
                    db.get_local_node_id(),
                )
            })
            .await;
        let flash = "Node ID, peer URL, and shared secret are required.";
        let tmpl = ListTemplate {
            nav_active: "Replication",
            flash: Some(flash),
            nodes,
            stats,
            this_node_id,
        };
        return Html(tmpl.render().unwrap()).into_response();
    }

    info!(
        "[web] POST /replicas — adding replica node_id={} url={}",
        node_id, peer_url
    );
    let result = state
        .blocking_db(move |db| db.add_replica_node(&node_id, &peer_url, &shared_secret))
        .await;

    match result {
        Ok(_) => axum::response::Redirect::to("/replicas").into_response(),
        Err(e) => {
            error!("[web] failed to add replica node: {}", e);
            let (nodes, stats, this_node_id) = state
                .blocking_db(|db| {
                    (
                        db.list_replica_nodes(),
                        db.get_replication_stats(),
                        db.get_local_node_id(),
                    )
                })
                .await;
            let flash = "Failed to add replica node. The node ID may already exist.";
            let tmpl = ListTemplate {
                nav_active: "Replication",
                flash: Some(flash),
                nodes,
                stats,
                this_node_id,
            };
            Html(tmpl.render().unwrap()).into_response()
        }
    }
}

pub async fn delete(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Response {
    info!("[web] POST /replicas/{}/delete", id);
    state.blocking_db(move |db| db.delete_replica_node(id)).await;
    axum::response::Redirect::to("/replicas").into_response()
}

pub async fn toggle(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> Response {
    info!("[web] POST /replicas/{}/toggle", id);
    let current = state.blocking_db(move |db| db.get_replica_node(id)).await;
    if let Some(node) = current {
        let new_active = !node.active;
        state
            .blocking_db(move |db| db.set_replica_node_active(id, new_active))
            .await;
    }
    axum::response::Redirect::to("/replicas").into_response()
}

// ── Public inbound endpoint ──

/// Receive a batch of replication changes from a peer node.
///
/// Security:
/// - The sender identifies itself via `X-Replication-From` header.
/// - The `X-Replication-Sig` header carries HMAC-SHA256(shared_secret, body).
/// - We look up the peer by node_id, verify the signature, then apply LWW changes.
pub async fn apply(State(state): State<AppState>, headers: HeaderMap, body: String) -> Response {
    let from_node = headers
        .get("X-Replication-From")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let received_sig = headers
        .get("X-Replication-Sig")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if from_node.is_empty() {
        warn!("[repl] /replication/apply: missing X-Replication-From header");
        return (StatusCode::BAD_REQUEST, "Missing X-Replication-From").into_response();
    }

    // Look up the peer and verify the signature
    let from_node_clone = from_node.clone();
    let peer = state
        .blocking_db(move |db| db.get_replica_node_by_node_id(&from_node_clone))
        .await;

    let peer = match peer {
        Some(p) => p,
        None => {
            warn!(
                "[repl] /replication/apply: unknown peer node '{}'",
                from_node
            );
            return (StatusCode::UNAUTHORIZED, "Unknown peer node").into_response();
        }
    };

    if !replication::verify_hmac_sha256(&peer.shared_secret, &body, &received_sig) {
        warn!(
            "[repl] /replication/apply: invalid HMAC from '{}'",
            peer.node_id
        );
        return (StatusCode::UNAUTHORIZED, "Invalid signature").into_response();
    }

    let entries = match replication::parse_entries(&body) {
        Ok(e) => e,
        Err(e) => {
            error!("[repl] /replication/apply: failed to parse body: {}", e);
            return (StatusCode::BAD_REQUEST, "Invalid payload").into_response();
        }
    };

    info!(
        "[repl] /replication/apply: received {} entries from '{}'",
        entries.len(),
        peer.node_id
    );

    let peer_id = peer.id;
    state
        .blocking_db(move |db| {
            db.apply_changelog_entries(&entries);
            db.touch_replica_node(peer_id);
        })
        .await;

    StatusCode::OK.into_response()
}
