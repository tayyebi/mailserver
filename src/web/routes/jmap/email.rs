use serde_json::{json, Value};
use std::collections::HashMap;

use super::super::AppState;
use super::{
    account_email, get_session_state, mailbox_dir, mailbox_id_from_dir, scan_maildir_folders,
    JmapAuth,
};

// ── Email/get ─────────────────────────────────────────────────────────────────

pub async fn email_get(
    state: &AppState,
    auth: &JmapAuth,
    args: &Value,
) -> super::DispatchResult {
    let account_id = auth.account_id;
    let email = account_email(&auth.account);
    let mdir = super::maildir_path(
        auth.account.domain_name.as_deref().unwrap_or(""),
        &auth.account.username,
    );

    let ids: Vec<String> = match args.get("ids") {
        Some(Value::Array(arr)) => arr.iter().filter_map(|v| v.as_str().map(String::from)).collect(),
        Some(Value::String(s)) if s == "all" => {
            // Return all email IDs (expensive! but spec allows it)
            let mut all = Vec::new();
            let folders = scan_maildir_folders(&mdir);
            for (dir_name, _) in &folders {
                let mb_id = mailbox_id_from_dir(dir_name);
                let mb_dir = mailbox_dir(&mdir, &mb_id);
                let files = list_maildir_files(&mb_dir);
                for (fname, _) in &files {
                    all.push(format!("email:{}", fname));
                }
            }
            all
        }
        _ => return super::DispatchResult {
            name: "Email/get".to_string(),
            args: json!({
                "accountId": account_id.to_string(),
                "state": get_session_state(state, account_id).await,
                "list": [],
                "notFound": [],
            }),
        },
    };

    let properties = args
        .get("properties")
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        });

    let mut emails = Vec::new();
    let mut not_found = Vec::new();

    for id in &ids {
        let filename = match id.strip_prefix("email:") {
            Some(f) => f.to_string(),
            None => {
                not_found.push(id.clone());
                continue;
            }
        };

        // Find the file in any mailbox
        let file_data = find_email_file(&mdir, &filename);
        match file_data {
            Some((data, mailbox_ids)) => {
                let parsed = parse_email_to_jmap(&data, &filename, &mailbox_ids, &email, &properties);
                emails.push(parsed);
            }
            None => {
                not_found.push(id.clone());
            }
        }
    }

    super::DispatchResult {
        name: "Email/get".to_string(),
        args: json!({
            "accountId": account_id.to_string(),
            "state": get_session_state(state, account_id).await,
            "list": emails,
            "notFound": not_found,
        }),
    }
}

fn find_email_file(mdir: &str, filename: &str) -> Option<(Vec<u8>, Vec<String>)> {
    use std::fs;
    let folders = scan_maildir_folders(mdir);

    for (dir_name, _) in &folders {
        let mb_id = mailbox_id_from_dir(dir_name);
        let mb_dir = mailbox_dir(mdir, &mb_id);

        for sub in &["cur", "new"] {
            let path = format!("{}/{}/{}", mb_dir, sub, filename);
            if let Ok(data) = fs::read(&path) {
                return Some((data, vec![mb_id]));
            }
        }
    }
    None
}

fn list_maildir_files(mb_dir: &str) -> Vec<(String, String)> {
    use std::fs;
    let mut files = Vec::new();

    for sub in &["new", "cur"] {
        if let Ok(entries) = fs::read_dir(format!("{}/{}", mb_dir, sub)) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let sub = sub.to_string();
                files.push((name, sub));
            }
        }
    }

    files
}

fn parse_email_to_jmap(
    data: &[u8],
    filename: &str,
    mailbox_ids: &[String],
    my_email: &str,
    properties: &Option<Vec<String>>,
) -> Value {
    let msg = match mailparse::parse_mail(data) {
        Ok(m) => m,
        Err(_) => {
            return json!({
                "id": format!("email:{}", filename),
                "blobId": filename,
                "size": data.len(),
                "error": "parse error",
            });
        }
    };

    let headers = extract_headers(&msg);
    let size = data.len() as i64;
    let subject = headers.get("subject").cloned().unwrap_or_default();
    let from_str = headers.get("from").cloned().unwrap_or_default();
    let to_str = headers.get("to").cloned().unwrap_or_default();
    let cc_str = headers.get("cc").cloned().unwrap_or_default();
    let _bcc_str = headers.get("bcc").cloned().unwrap_or_default();
    let date_str = headers.get("date").cloned().unwrap_or_default();
    let msg_id = headers.get("message-id").cloned().unwrap_or_default();
    let in_reply_to = headers.get("in-reply-to").cloned().unwrap_or_default();
    let references = headers.get("references").cloned().unwrap_or_default();

    // Parse addresses
    let from_addrs = parse_address_list(&from_str);
    let to_addrs = parse_address_list(&to_str);
    let cc_addrs = parse_address_list(&cc_str);

    // Detect thread by In-Reply-To / References
    let thread_id = compute_thread_id(&msg_id, &in_reply_to, &references, &subject);

    // Detect Maildir flags from filename
    let keywords = parse_maildir_keywords(filename);

    // Folders this email belongs to
    let mut mailbox_ids_map = serde_json::Map::new();
    for mb_id in mailbox_ids {
        mailbox_ids_map.insert(mb_id.clone(), Value::Bool(true));
    }

    // Parse body parts
    let (text_body, html_body, attachments, body_structure) = parse_body_parts(&msg, my_email);

    // Preview
    let preview = text_body
        .first()
        .and_then(|p| p.get("value"))
        .and_then(|v| v.as_str())
        .map(|s| {
            s.chars().take(256).collect::<String>()
        })
        .unwrap_or_default();

    // Build result, filtering by properties if requested
    let mut result = serde_json::Map::new();
    result.insert("id".to_string(), Value::String(format!("email:{}", filename)));
    result.insert("blobId".to_string(), Value::String(filename.to_string()));
    result.insert("threadId".to_string(), Value::String(thread_id));
    result.insert("mailboxIds".to_string(), Value::Object(mailbox_ids_map));
    result.insert("keywords".to_string(), keywords);
    result.insert("size".to_string(), json!(size));
    result.insert("receivedAt".to_string(), Value::String(normalize_date(&date_str)));
    result.insert("from".to_string(), from_addrs);
    result.insert("to".to_string(), to_addrs);
    result.insert("cc".to_string(), cc_addrs);
    result.insert("bcc".to_string(), json!([]));
    result.insert("subject".to_string(), Value::String(subject));
    result.insert("messageId".to_string(), Value::String(msg_id));
    result.insert("inReplyTo".to_string(), Value::String(in_reply_to));
    result.insert("references".to_string(), Value::String(references));
    result.insert("preview".to_string(), Value::String(preview));
    result.insert("bodyStructure".to_string(), body_structure);
    result.insert("textBody".to_string(), Value::Array(text_body));
    result.insert("htmlBody".to_string(), Value::Array(html_body));
    result.insert("attachments".to_string(), Value::Array(attachments));

    if let Some(props) = properties {
        result.retain(|k, _| {
            k == "id" || props.contains(k)
        });
    }

    Value::Object(result)
}

pub(super) fn extract_headers(msg: &mailparse::ParsedMail) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for hdr in &msg.headers {
        let key = hdr.get_key().to_lowercase();
        let val = hdr.get_value();
        map.insert(key, val);
    }
    map
}

fn parse_address_list(input: &str) -> Value {
    if input.is_empty() {
        return json!([]);
    }
    let addrs: Vec<Value> = input
        .split(',')
        .filter_map(|part| {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                return None;
            }
            // Try to extract name and email
            if let Some(email_start) = trimmed.find('<') {
                let name = trimmed[..email_start].trim().to_string();
                let email_end = trimmed.find('>').unwrap_or(trimmed.len());
                let email = trimmed[email_start + 1..email_end].trim().to_string();
                Some(json!({
                    "name": name,
                    "email": email,
                }))
            } else {
                // Just an email address
                Some(json!({
                    "name": "",
                    "email": trimmed.to_string(),
                }))
            }
        })
        .collect();
    json!(addrs)
}

fn parse_maildir_keywords(filename: &str) -> Value {
    let mut kw = serde_json::Map::new();
    kw.insert("$seen".to_string(), Value::Bool(false));
    kw.insert("$flagged".to_string(), Value::Bool(false));
    kw.insert("$draft".to_string(), Value::Bool(false));

    if let Some(flags_pos) = filename.find(":2,") {
        let flags = &filename[flags_pos + 3..];
        let flags_lower = flags.to_lowercase();
        kw.insert("$seen".to_string(), Value::Bool(flags_lower.contains('s')));
        kw.insert("$flagged".to_string(), Value::Bool(flags_lower.contains('f')));
        kw.insert("$draft".to_string(), Value::Bool(flags_lower.contains('d')));
    }

    Value::Object(kw)
}

fn parse_body_parts(
    msg: &mailparse::ParsedMail,
    _my_email: &str,
) -> (Vec<Value>, Vec<Value>, Vec<Value>, Value) {
    let mut text_parts = Vec::new();
    let mut html_parts = Vec::new();
    let mut attachments = Vec::new();

    walk_mime_parts(msg, &mut text_parts, &mut html_parts, &mut attachments);

    let text_body: Vec<Value> = text_parts
        .into_iter()
        .enumerate()
        .map(|(i, t)| {
            json!({
                "partId": format!("p{}", i + 1),
                "blobId": null,
                "size": t.len(),
                "type": "text/plain",
            })
        })
        .collect();

    let html_body: Vec<Value> = html_parts
        .into_iter()
        .enumerate()
        .map(|(i, h)| {
            json!({
                "partId": format!("h{}", i + 1),
                "blobId": null,
                "size": h.len(),
                "type": "text/html",
            })
        })
        .collect();

    let attachments: Vec<Value> = attachments
        .into_iter()
        .map(|a| {
            json!({
                "blobId": null,
                "size": a.size,
                "type": a.mime_type,
                "name": a.filename,
            })
        })
        .collect();

    // Simple body structure
    let body_structure = json!({
        "type": "multipart/mixed",
        "subParts": [
            {"type": "text/plain"},
            {"type": "text/html"},
        ]
    });

    (text_body, html_body, attachments, body_structure)
}

struct AttachmentInfo {
    size: usize,
    mime_type: String,
    filename: String,
}

fn walk_mime_parts(
    msg: &mailparse::ParsedMail,
    text_parts: &mut Vec<Vec<u8>>,
    html_parts: &mut Vec<Vec<u8>>,
    attachments: &mut Vec<AttachmentInfo>,
) {
    let ctype = msg.ctype.mimetype.to_lowercase();
    let is_attachment = msg.ctype.params.contains_key("name")
        || msg.ctype.params.contains_key("filename");

    if ctype == "text/plain" && !is_attachment {
        if let Ok(body) = msg.get_body() {
            text_parts.push(body.into_bytes());
        }
    } else if ctype == "text/html" && !is_attachment {
        if let Ok(body) = msg.get_body() {
            html_parts.push(body.into_bytes());
        }
    } else if is_attachment || ctype.starts_with("application/") || ctype.starts_with("image/")
        || ctype.starts_with("audio/") || ctype.starts_with("video/")
    {
        let data = msg.get_body_raw().unwrap_or_default();
        let filename = msg
            .ctype
            .params
            .get("name")
            .or_else(|| msg.ctype.params.get("filename"))
            .cloned()
            .unwrap_or_default();
        attachments.push(AttachmentInfo {
            size: data.len(),
            mime_type: ctype,
            filename: filename.replace('"', ""),
        });
    }

    // Recurse into sub-parts
    for sub_msg in &msg.subparts {
        walk_mime_parts(sub_msg, text_parts, html_parts, attachments);
    }
}

fn normalize_date(date_str: &str) -> String {
    if date_str.is_empty() {
        return chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    }
    // Try parsing common email date formats
    for fmt in &[
        "%a, %d %b %Y %H:%M:%S %z",
        "%a, %d %b %Y %H:%M:%S %Z",
        "%d %b %Y %H:%M:%S %z",
        "%d %b %Y %H:%M:%S %Z",
        "%Y-%m-%dT%H:%M:%S%.fZ",
        "%Y-%m-%dT%H:%M:%S%.f%:z",
    ] {
        if let Ok(dt) = chrono::DateTime::parse_from_str(date_str, fmt) {
            return dt.to_rfc3339();
        }
    }
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

pub(super) fn compute_thread_id(
    _msg_id: &str,
    in_reply_to: &str,
    references: &str,
    subject: &str,
) -> String {
    // Use first message in references chain, or in-reply-to, or normalized subject
    if !references.is_empty() {
        if let Some(first) = references.split_whitespace().next() {
            let tid = first.trim_matches('<').trim_matches('>');
            return format!("thread:{}", tid);
        }
    }
    if !in_reply_to.is_empty() {
        let tid = in_reply_to.trim_matches('<').trim_matches('>');
        return format!("thread:{}", tid);
    }
    // Fallback: hash the normalized subject
    let normalized = normalize_subject(subject);
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    normalized.hash(&mut hasher);
    format!("thread:{:016x}", hasher.finish())
}

fn normalize_subject(subject: &str) -> String {
    let s = subject.to_lowercase();
    let s = s.trim_start_matches("re: ");
    let s = s.trim_start_matches("re:");
    let s = s.trim_start_matches("fwd: ");
    let s = s.trim_start_matches("fwd:");
    let s = s.trim_start_matches("fw: ");
    let s = s.trim_start_matches("fw:");
    s.to_string()
}

// ── Email/query ───────────────────────────────────────────────────────────────

pub async fn email_query(
    state: &AppState,
    auth: &JmapAuth,
    args: &Value,
) -> super::DispatchResult {
    let account_id = auth.account_id;
    let mdir = super::maildir_path(
        auth.account.domain_name.as_deref().unwrap_or(""),
        &auth.account.username,
    );

    // Determine which mailbox to search
    let target_mailbox = args
        .get("filter")
        .and_then(|f| f.get("inMailbox"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "mailbox:inbox".to_string());

    let mb_dir = mailbox_dir(&mdir, &target_mailbox);
    let files = list_maildir_files(&mb_dir);

    // Build ID list with basic info (just read headers for filtering)
    let mut results: Vec<EmailQueryEntry> = files
        .iter()
        .filter_map(|(fname, _sub)| {
            // Read just the first 8KB to get headers
            let full_path = if fname.contains(":2,") {
                format!("{}/cur/{}", mb_dir, fname)
            } else {
                format!("{}/new/{}", mb_dir, fname)
            };

            let data = std::fs::read(&full_path).ok()?;
            let msg = mailparse::parse_mail(&data).ok()?;
            let headers = super::email::extract_headers(&msg);

            let subject = headers.get("subject").cloned().unwrap_or_default();
            let from = headers.get("from").cloned().unwrap_or_default();
            let to = headers.get("to").cloned().unwrap_or_default();
            let cc = headers.get("cc").cloned().unwrap_or_default();
            let date_str = headers.get("date").cloned().unwrap_or_default();
            let _msg_id = headers.get("message-id").cloned().unwrap_or_default();
            let _in_reply_to = headers.get("in-reply-to").cloned().unwrap_or_default();
            let _references = headers.get("references").cloned().unwrap_or_default();

            // Apply filters
            if let Some(filter) = args.get("filter") {
                if !apply_filter(filter, &subject, &from, &to, &cc) {
                    return None;
                }
            }

            let received_at = normalize_date(&date_str);
            let dt = chrono::DateTime::parse_from_rfc3339(&received_at)
                .unwrap_or_else(|_| chrono::Utc::now().into());

            Some(EmailQueryEntry {
                id: format!("email:{}", fname),
                received_at: dt,
                subject,
                size: data.len() as i64,
            })
        })
        .collect();

    // Sort
    let sort_reverse = args
        .get("sort")
        .and_then(|s| s.as_array())
        .and_then(|arr| arr.first())
        .and_then(|s| s.get("isAscending"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    results.sort_by(|a, b| {
        let cmp = a.received_at.cmp(&b.received_at);
        if sort_reverse { cmp } else { cmp.reverse() }
    });

    // Paginate
    let position = args.get("position").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;

    let total = results.len() as i64;
    let page: Vec<String> = results
        .into_iter()
        .skip(position)
        .take(limit)
        .map(|e| e.id)
        .collect();

    let session_state = get_session_state(state, account_id).await;

    super::DispatchResult {
        name: "Email/query".to_string(),
        args: json!({
            "accountId": account_id.to_string(),
            "queryState": session_state,
            "canCalculateChanges": false,
            "position": position,
            "limit": limit,
            "ids": page,
            "total": total,
        }),
    }
}

struct EmailQueryEntry {
    id: String,
    received_at: chrono::DateTime<chrono::FixedOffset>,
    #[allow(dead_code)]
    subject: String,
    #[allow(dead_code)]
    size: i64,
}

fn apply_filter(
    filter: &Value,
    subject: &str,
    from: &str,
    to: &str,
    cc: &str,
) -> bool {
    let subj_lower = subject.to_lowercase();
    let from_lower = from.to_lowercase();
    let to_lower = to.to_lowercase();
    let cc_lower = cc.to_lowercase();

    // subject filter
    if let Some(val) = filter.get("subject").and_then(|v| v.as_str()) {
        if !subj_lower.contains(&val.to_lowercase()) {
            return false;
        }
    }

    // from filter
    if let Some(val) = filter.get("from").and_then(|v| v.as_str()) {
        if !from_lower.contains(&val.to_lowercase()) {
            return false;
        }
    }

    // to filter
    if let Some(val) = filter.get("to").and_then(|v| v.as_str()) {
        if !to_lower.contains(&val.to_lowercase()) {
            return false;
        }
    }

    // cc filter
    if let Some(val) = filter.get("cc").and_then(|v| v.as_str()) {
        if !cc_lower.contains(&val.to_lowercase()) {
            return false;
        }
    }

    true
}

// ── Email/getChanges ──────────────────────────────────────────────────────────

pub async fn email_get_changes(
    state: &AppState,
    auth: &JmapAuth,
    args: &Value,
) -> super::DispatchResult {
    let account_id = auth.account_id;
    let since_state = args
        .get("sinceState")
        .and_then(|v| v.as_str())
        .unwrap_or("0")
        .to_string();

    let mdir = super::maildir_path(
        auth.account.domain_name.as_deref().unwrap_or(""),
        &auth.account.username,
    );

    // Get current email IDs per mailbox
    let folders = scan_maildir_folders(&mdir);
    let mut current_emails: Vec<(String, Vec<String>)> = Vec::new();

    for (dir_name, _) in &folders {
        let mb_id = mailbox_id_from_dir(dir_name);
        let mb_dir = mailbox_dir(&mdir, &mb_id);
        let files = list_maildir_files(&mb_dir);
        for (fname, _sub) in &files {
            let email_id = format!("email:{}", fname);
            current_emails.push((email_id, vec![mb_id.clone()]));
        }
    }

    // Get stored snapshot
    let (current_state, stored_snapshot): (String, String) = state
        .blocking_db(move |db| db.get_jmap_state(account_id))
        .await;

    if since_state == current_state {
        return super::DispatchResult {
            name: "Email/getChanges".to_string(),
            args: json!({
                "accountId": account_id.to_string(),
                "oldState": since_state,
                "newState": current_state,
                "hasMoreChanges": false,
                "created": [],
                "updated": [],
                "destroyed": [],
            }),
        };
    }

    // Parse stored snapshot to get previous email IDs
    let stored_ids: Vec<String> =
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&stored_snapshot) {
            v.as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
                .unwrap_or_default()
        } else {
            vec![]
        };

    let current_ids: Vec<String> = current_emails.iter().map(|(id, _)| id.clone()).collect();

    let stored_set: std::collections::HashSet<&str> =
        stored_ids.iter().map(|s| s.as_str()).collect();
    let current_set: std::collections::HashSet<&str> =
        current_ids.iter().map(|s| s.as_str()).collect();

    let created: Vec<&str> = current_set.difference(&stored_set).copied().collect();
    let destroyed: Vec<&str> = stored_set.difference(&current_set).copied().collect();

    // Update snapshot
    let new_state = format!("{}", current_state.parse::<i64>().unwrap_or(0) + 1);
    let new_state_clone = new_state.clone();
    let new_snapshot = serde_json::to_string(&current_ids).unwrap_or_default();

    state
        .blocking_db(move |db| {
            db.update_jmap_state(account_id, &new_state_clone, &new_snapshot);
        })
        .await;

    super::DispatchResult {
        name: "Email/getChanges".to_string(),
        args: json!({
            "accountId": account_id.to_string(),
            "oldState": since_state,
            "newState": new_state,
            "hasMoreChanges": false,
            "created": created,
            "updated": [],
            "destroyed": destroyed,
        }),
    }
}
