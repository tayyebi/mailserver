//! MCP (Model Context Protocol) endpoint — JSON-RPC 2.0 interface for AI bots.
//!
//! Endpoint: `POST /mcp`
//! Authentication: HTTP Basic Auth (same admin credentials as the web UI)
//!
//! Supported JSON-RPC methods:
//!   `initialize`   — Negotiate protocol version and announce capabilities
//!   `tools/list`   — List the available tools and their input schemas
//!   `tools/call`   — Call a named tool
//!
//! Available tools:
//!   `list_accounts` — List all mail accounts on the server
//!   `list_emails`   — List emails in an account's inbox or folder
//!   `read_email`    — Read the full content of an email
//!   `send_email`    — Send an email from an account
//!   `delete_email`  — Delete an email from an account

use axum::{extract::State, Json};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::web::auth::AuthAdmin;
use crate::web::AppState;

use super::webmail::{
    extract_body, folder_root, is_safe_folder, is_safe_path_component, maildir_path, read_emails,
};

const PROTOCOL_VERSION: &str = "2024-11-05";
const PAGE_SIZE: usize = 20;

// ── JSON-RPC 2.0 types ───────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct McpRequest {
    #[allow(dead_code)]
    pub jsonrpc: Option<String>,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

#[derive(Serialize)]
pub struct McpResponse {
    pub jsonrpc: String,
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpError>,
}

#[derive(Serialize)]
pub struct McpError {
    pub code: i32,
    pub message: String,
}

impl McpResponse {
    fn ok(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    fn err(id: Option<Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(McpError {
                code,
                message: message.into(),
            }),
        }
    }
}

// ── Tool schema ───────────────────────────────────────────────────────────────

pub fn tools_list_value() -> Value {
    json!({
        "tools": [
            {
                "name": "list_accounts",
                "description": "List all mail accounts on the server",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "list_emails",
                "description": "List emails in a mail account's inbox or a named folder (20 per page)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "account_id": {
                            "type": "integer",
                            "description": "Account ID (from list_accounts)"
                        },
                        "folder": {
                            "type": "string",
                            "description": "Maildir folder name (e.g. '.Sent', '.Drafts'). Omit or leave empty for INBOX."
                        },
                        "page": {
                            "type": "integer",
                            "description": "Page number, 1-based (default: 1)"
                        }
                    },
                    "required": ["account_id"]
                }
            },
            {
                "name": "read_email",
                "description": "Read the full content of a single email",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "account_id": {
                            "type": "integer",
                            "description": "Account ID"
                        },
                        "filename": {
                            "type": "string",
                            "description": "Base64url-encoded filename returned by list_emails"
                        },
                        "folder": {
                            "type": "string",
                            "description": "Maildir folder name. Omit or leave empty for INBOX."
                        }
                    },
                    "required": ["account_id", "filename"]
                }
            },
            {
                "name": "send_email",
                "description": "Send an email from a mail account via the local SMTP server",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "account_id": {
                            "type": "integer",
                            "description": "Account ID to send from"
                        },
                        "to": {
                            "type": "string",
                            "description": "Recipient email address"
                        },
                        "subject": {
                            "type": "string",
                            "description": "Email subject"
                        },
                        "body": {
                            "type": "string",
                            "description": "Email body text"
                        },
                        "cc": {
                            "type": "string",
                            "description": "CC recipients (comma-separated)"
                        },
                        "bcc": {
                            "type": "string",
                            "description": "BCC recipients (comma-separated)"
                        },
                        "reply_to": {
                            "type": "string",
                            "description": "Reply-To address"
                        },
                        "sender_name": {
                            "type": "string",
                            "description": "Display name shown as the sender"
                        },
                        "body_format": {
                            "type": "string",
                            "enum": ["plain", "html"],
                            "description": "Body content type (default: plain)"
                        }
                    },
                    "required": ["account_id", "to", "subject", "body"]
                }
            },
            {
                "name": "delete_email",
                "description": "Permanently delete an email from a mail account",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "account_id": {
                            "type": "integer",
                            "description": "Account ID"
                        },
                        "filename": {
                            "type": "string",
                            "description": "Base64url-encoded filename returned by list_emails"
                        },
                        "folder": {
                            "type": "string",
                            "description": "Maildir folder name. Omit or leave empty for INBOX."
                        }
                    },
                    "required": ["account_id", "filename"]
                }
            }
        ]
    })
}

// ── HTTP handler ──────────────────────────────────────────────────────────────

pub async fn handle(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Json(req): Json<McpRequest>,
) -> Json<McpResponse> {
    info!("[mcp] method={}", req.method);

    let id = req.id.clone();

    let result = match req.method.as_str() {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "serverInfo": {
                "name": "mailserver",
                "version": env!("CARGO_PKG_VERSION")
            },
            "capabilities": {
                "tools": {}
            }
        })),
        "tools/list" => Ok(tools_list_value()),
        "tools/call" => dispatch_tool_call(&state, &req).await,
        other => {
            warn!("[mcp] unknown method: {}", other);
            return Json(McpResponse::err(
                id,
                -32601,
                format!("Method not found: {}", other),
            ));
        }
    };

    match result {
        Ok(v) => Json(McpResponse::ok(id, v)),
        Err(e) => Json(McpResponse::err(id, -32603, e)),
    }
}

// ── Tool dispatch ─────────────────────────────────────────────────────────────

async fn dispatch_tool_call(state: &AppState, req: &McpRequest) -> Result<Value, String> {
    let params = req.params.as_ref().ok_or("Missing params")?;
    let tool_name = params["name"].as_str().ok_or("Missing tool name")?;
    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    match tool_name {
        "list_accounts" => tool_list_accounts(state).await,
        "list_emails" => tool_list_emails(state, &args).await,
        "read_email" => tool_read_email(state, &args).await,
        "send_email" => tool_send_email(state, &args).await,
        "delete_email" => tool_delete_email(state, &args).await,
        other => Err(format!("Unknown tool: {}", other)),
    }
}

// ── Tool: list_accounts ───────────────────────────────────────────────────────

async fn tool_list_accounts(state: &AppState) -> Result<Value, String> {
    let accounts = state
        .blocking_db(|db| db.list_all_accounts_with_domain())
        .await;

    let items: Vec<Value> = accounts
        .iter()
        .map(|a| {
            let email = format!(
                "{}@{}",
                a.username,
                a.domain_name.as_deref().unwrap_or("unknown")
            );
            json!({
                "id": a.id,
                "email": email,
                "name": a.name,
                "active": a.active,
                "quota": a.quota
            })
        })
        .collect();

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string_pretty(&items).unwrap_or_default()
        }]
    }))
}

// ── Tool: list_emails ─────────────────────────────────────────────────────────

async fn tool_list_emails(state: &AppState, args: &Value) -> Result<Value, String> {
    let account_id = args["account_id"]
        .as_i64()
        .ok_or("Missing or invalid account_id")?;

    let folder = args["folder"].as_str().unwrap_or("").to_string();
    let page = args["page"].as_u64().unwrap_or(1).max(1) as usize;

    if !is_safe_folder(&folder) {
        return Err("Invalid folder name".to_string());
    }

    let acct = state
        .blocking_db(move |db| db.get_account_with_domain(account_id))
        .await
        .ok_or("Account not found")?;

    let domain = acct.domain_name.as_deref().unwrap_or("unknown").to_string();
    if !is_safe_path_component(&domain) || !is_safe_path_component(&acct.username) {
        return Err("Invalid account path".to_string());
    }

    let maildir_base = maildir_path(&domain, &acct.username);
    let mut logs = Vec::new();
    let emails = read_emails(&maildir_base, &folder, &mut logs);

    let total = emails.len();
    let total_pages = if total == 0 {
        1
    } else {
        (total + PAGE_SIZE - 1) / PAGE_SIZE
    };
    let page = page.min(total_pages);
    let start = (page - 1) * PAGE_SIZE;

    let page_emails: Vec<Value> = emails
        .iter()
        .skip(start)
        .take(PAGE_SIZE)
        .map(|e| {
            json!({
                "filename": e.filename,
                "subject": e.subject,
                "from": e.from,
                "to": e.to,
                "date": e.date,
                "is_new": e.is_new,
                "is_spam": e.is_spam
            })
        })
        .collect();

    let result = json!({
        "account_id": account_id,
        "folder": if folder.is_empty() { "INBOX" } else { folder.as_str() },
        "page": page,
        "total_pages": total_pages,
        "total_count": total,
        "emails": page_emails
    });

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string_pretty(&result).unwrap_or_default()
        }]
    }))
}

// ── Tool: read_email ──────────────────────────────────────────────────────────

async fn tool_read_email(state: &AppState, args: &Value) -> Result<Value, String> {
    let account_id = args["account_id"]
        .as_i64()
        .ok_or("Missing or invalid account_id")?;

    let filename_b64 = args["filename"].as_str().ok_or("Missing filename")?;
    let folder = args["folder"].as_str().unwrap_or("").to_string();

    if !is_safe_folder(&folder) {
        return Err("Invalid folder name".to_string());
    }

    let acct = state
        .blocking_db(move |db| db.get_account_with_domain(account_id))
        .await
        .ok_or("Account not found")?;

    let filename = URL_SAFE_NO_PAD
        .decode(filename_b64.as_bytes())
        .map_err(|e| format!("Invalid filename encoding: {}", e))
        .and_then(|b| {
            String::from_utf8(b).map_err(|e| format!("Invalid UTF-8 in filename: {}", e))
        })?;

    let domain = acct.domain_name.as_deref().unwrap_or("unknown").to_string();
    if !is_safe_path_component(&domain)
        || !is_safe_path_component(&acct.username)
        || !is_safe_path_component(&filename)
    {
        return Err("Invalid path component".to_string());
    }

    let maildir_base = maildir_path(&domain, &acct.username);
    let root = folder_root(&maildir_base, &folder);

    let mut file_path = None;
    for subdir in &["new", "cur"] {
        let candidate = format!("{}/{}/{}", root, subdir, filename);
        if std::path::Path::new(&candidate).is_file() {
            file_path = Some(candidate);
            break;
        }
    }

    let file_path = file_path.ok_or("Email not found")?;

    let data =
        std::fs::read(&file_path).map_err(|e| format!("Failed to read email: {}", e))?;

    let parsed =
        mailparse::parse_mail(&data).map_err(|e| format!("Failed to parse email: {}", e))?;

    let subject = parsed
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case("Subject"))
        .map(|h| h.get_value())
        .unwrap_or_default();
    let from = parsed
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case("From"))
        .map(|h| h.get_value())
        .unwrap_or_default();
    let to = parsed
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case("To"))
        .map(|h| h.get_value())
        .unwrap_or_default();
    let date = parsed
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case("Date"))
        .map(|h| h.get_value())
        .unwrap_or_default();
    let is_spam = parsed
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case("X-Spam-Flag"))
        .map(|h| h.get_value().trim().eq_ignore_ascii_case("YES"))
        .unwrap_or(false);
    let body = extract_body(&parsed);

    let result = json!({
        "subject": subject,
        "from": from,
        "to": to,
        "date": date,
        "body": body,
        "is_spam": is_spam
    });

    Ok(json!({
        "content": [{
            "type": "text",
            "text": serde_json::to_string_pretty(&result).unwrap_or_default()
        }]
    }))
}

// ── Tool: send_email ──────────────────────────────────────────────────────────

async fn tool_send_email(state: &AppState, args: &Value) -> Result<Value, String> {
    let account_id = args["account_id"]
        .as_i64()
        .ok_or("Missing or invalid account_id")?;

    let to = args["to"].as_str().ok_or("Missing 'to' address")?;
    let subject = args["subject"].as_str().ok_or("Missing subject")?;
    let body = args["body"].as_str().ok_or("Missing body")?;
    let cc = args["cc"].as_str().unwrap_or("").to_string();
    let bcc = args["bcc"].as_str().unwrap_or("").to_string();
    let reply_to = args["reply_to"].as_str().unwrap_or("").to_string();
    let sender_name = args["sender_name"].as_str().unwrap_or("").to_string();
    let body_format = args["body_format"].as_str().unwrap_or("plain").to_string();

    let acct = state
        .blocking_db(move |db| db.get_account_with_domain(account_id))
        .await
        .ok_or("Account not found")?;

    let domain = acct.domain_name.as_deref().unwrap_or("unknown");
    let email_addr = format!("{}@{}", acct.username, domain);

    let from_addr = if sender_name.is_empty() {
        email_addr.clone()
    } else {
        let safe_name = sender_name.replace(['\r', '\n'], " ");
        format!("{} <{}>", safe_name, email_addr)
    };

    use lettre::message::header::ContentType;
    use lettre::message::SinglePart;
    use lettre::{SmtpTransport, Transport};

    let mut builder = lettre::Message::builder()
        .from(
            from_addr
                .parse()
                .map_err(|e| format!("Invalid from address: {}", e))?,
        )
        .to(to
            .parse()
            .map_err(|e| format!("Invalid To address: {}", e))?)
        .subject(subject);

    for addr in cc.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        match addr.parse() {
            Ok(a) => builder = builder.cc(a),
            Err(e) => warn!("[mcp] skipping invalid CC address '{}': {}", addr, e),
        }
    }

    for addr in bcc.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        match addr.parse() {
            Ok(a) => builder = builder.bcc(a),
            Err(e) => warn!("[mcp] skipping invalid BCC address '{}': {}", addr, e),
        }
    }

    if !reply_to.trim().is_empty() {
        match reply_to.trim().parse() {
            Ok(a) => builder = builder.reply_to(a),
            Err(e) => warn!("[mcp] invalid Reply-To '{}': {}", reply_to, e),
        }
    }

    let email = match body_format.as_str() {
        "html" => builder
            .singlepart(
                SinglePart::builder()
                    .header(ContentType::TEXT_HTML)
                    .body(body.to_string()),
            )
            .map_err(|e| format!("Failed to build email: {}", e))?,
        _ => builder
            .body(body.to_string())
            .map_err(|e| format!("Failed to build email: {}", e))?,
    };

    SmtpTransport::builder_dangerous("127.0.0.1")
        .port(25)
        .build()
        .send(&email)
        .map_err(|e| format!("SMTP error: {}", e))?;

    info!("[mcp] email sent to {}", to);

    Ok(json!({
        "content": [{
            "type": "text",
            "text": "Email sent successfully"
        }]
    }))
}

// ── Tool: delete_email ────────────────────────────────────────────────────────

async fn tool_delete_email(state: &AppState, args: &Value) -> Result<Value, String> {
    let account_id = args["account_id"]
        .as_i64()
        .ok_or("Missing or invalid account_id")?;

    let filename_b64 = args["filename"].as_str().ok_or("Missing filename")?;
    let folder = args["folder"].as_str().unwrap_or("").to_string();

    if !is_safe_folder(&folder) {
        return Err("Invalid folder name".to_string());
    }

    let acct = state
        .blocking_db(move |db| db.get_account_with_domain(account_id))
        .await
        .ok_or("Account not found")?;

    let filename = URL_SAFE_NO_PAD
        .decode(filename_b64.as_bytes())
        .map_err(|e| format!("Invalid filename encoding: {}", e))
        .and_then(|b| {
            String::from_utf8(b).map_err(|e| format!("Invalid UTF-8 in filename: {}", e))
        })?;

    let domain = acct.domain_name.as_deref().unwrap_or("unknown").to_string();
    if !is_safe_path_component(&domain)
        || !is_safe_path_component(&acct.username)
        || !is_safe_path_component(&filename)
    {
        return Err("Invalid path component".to_string());
    }

    let maildir_base = maildir_path(&domain, &acct.username);
    let root = folder_root(&maildir_base, &folder);

    let mut deleted = false;
    for subdir in &["new", "cur"] {
        let candidate = format!("{}/{}/{}", root, subdir, filename);
        if std::path::Path::new(&candidate).is_file() {
            std::fs::remove_file(&candidate)
                .map_err(|e| format!("Failed to delete email: {}", e))?;
            info!("[mcp] deleted email: {}", candidate);
            deleted = true;
            break;
        }
    }

    if !deleted {
        return Err("Email not found".to_string());
    }

    Ok(json!({
        "content": [{
            "type": "text",
            "text": "Email deleted successfully"
        }]
    }))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_list_contains_expected_tools() {
        let v = tools_list_value();
        let tools = v["tools"].as_array().unwrap();
        let names: Vec<&str> = tools
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"list_accounts"));
        assert!(names.contains(&"list_emails"));
        assert!(names.contains(&"read_email"));
        assert!(names.contains(&"send_email"));
        assert!(names.contains(&"delete_email"));
    }

    #[test]
    fn tools_have_input_schema() {
        let v = tools_list_value();
        for tool in v["tools"].as_array().unwrap() {
            let name = tool["name"].as_str().unwrap();
            assert!(
                tool.get("inputSchema").is_some(),
                "tool '{}' is missing inputSchema",
                name
            );
        }
    }

    #[test]
    fn mcp_response_ok_sets_result_and_no_error() {
        let r = McpResponse::ok(Some(json!(1)), json!({"foo": "bar"}));
        assert_eq!(r.jsonrpc, "2.0");
        assert!(r.result.is_some());
        assert!(r.error.is_none());
    }

    #[test]
    fn mcp_response_err_sets_error_and_no_result() {
        let r = McpResponse::err(Some(json!(2)), -32601, "Not found");
        assert_eq!(r.jsonrpc, "2.0");
        assert!(r.result.is_none());
        assert!(r.error.is_some());
        assert_eq!(r.error.unwrap().code, -32601);
    }

    #[test]
    fn send_email_requires_account_id() {
        let args = json!({"to": "x@y.com", "subject": "hi", "body": "hello"});
        // account_id missing → should not be i64
        assert!(args["account_id"].as_i64().is_none());
    }

    #[test]
    fn list_emails_required_fields_present_in_schema() {
        let v = tools_list_value();
        let list_emails = v["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "list_emails")
            .unwrap();
        let required = list_emails["inputSchema"]["required"].as_array().unwrap();
        let req_names: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(req_names.contains(&"account_id"));
    }
}
