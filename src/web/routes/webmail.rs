use askama::Template;
use axum::{
    extract::{Path, Query, State},
    http::header,
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use log::{debug, error, info, warn};
use serde::Deserialize;

use crate::db::Account;
use crate::web::auth::AuthAdmin;
use crate::web::AppState;

// ── Helpers ──

pub(crate) fn is_safe_path_component(s: &str) -> bool {
    !s.is_empty() && !s.contains('/') && !s.contains('\\') && s != "." && s != ".."
}

/// Validate a Maildir subfolder name (e.g. ".Sent", ".Drafts.Sub").
/// Empty string is valid and means INBOX.
pub(crate) fn is_safe_folder(s: &str) -> bool {
    if s.is_empty() {
        return true; // INBOX
    }
    // Must start with "." and contain no path separators
    s.starts_with('.') && !s.contains('/') && !s.contains('\\') && s != ".."
}

const MAILDIR_ROOT: &str = "/data/mail";
const PAGE_SIZE: usize = 20;

pub(crate) fn maildir_path(domain: &str, username: &str) -> String {
    format!("{}/{}/{}/Maildir", MAILDIR_ROOT, domain, username)
}

fn sanitize_header_value(s: &str) -> String {
    s.replace(['\r', '\n'], " ")
        .chars()
        .filter(|c| !c.is_control())
        .collect()
}

pub(crate) fn folder_root(maildir_base: &str, folder: &str) -> String {
    if folder.is_empty() {
        maildir_base.to_string()
    } else {
        format!("{}/{}", maildir_base, folder)
    }
}

// ── Structures ──

#[allow(dead_code)]
pub struct WebmailEmail {
    pub filename: String,
    pub subject: String,
    pub from: String,
    pub to: String,
    pub date: String,
    pub is_new: bool,
    pub is_spam: bool,
}

pub struct WebmailFolder {
    pub name: String,
    pub display_name: String,
    pub depth: usize,
}

/// A top-level folder together with its direct and indirect descendants,
/// and a flag indicating whether the group should be rendered expanded
/// (i.e. when the current folder is this group's root or one of its children).
pub struct WebmailFolderGroup {
    pub folder: WebmailFolder,
    pub children: Vec<WebmailFolder>,
    pub open: bool,
}

#[derive(Deserialize)]
pub struct WebmailQuery {
    pub account_id: Option<i64>,
    pub folder: Option<String>,
    pub page: Option<usize>,
}

#[derive(Deserialize)]
pub struct DeleteForm {
    pub account_id: i64,
    pub folder: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct ComposePageQuery {
    pub account_id: Option<i64>,
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub cc: String,
    #[serde(default)]
    pub bcc: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub reply_to: String,
    #[serde(default)]
    pub in_reply_to: String,
    #[serde(default)]
    pub priority: String,
    #[serde(default)]
    pub sender_name: String,
    #[serde(default)]
    pub from_address: String,
    #[serde(default)]
    pub body_format: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub custom_headers: String,
}

#[derive(Deserialize)]
pub struct ComposeForm {
    pub account_id: i64,
    pub to: String,
    #[serde(default)]
    pub cc: String,
    #[serde(default)]
    pub bcc: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub reply_to: String,
    #[serde(default)]
    pub in_reply_to: String,
    #[serde(default)]
    pub priority: String,
    #[serde(default)]
    pub sender_name: String,
    #[serde(default)]
    pub from_address: String,
    #[serde(default)]
    pub body_format: String,
    #[serde(default)]
    pub custom_headers: String,
    pub body: String,
}

#[derive(Clone, Default)]
struct ComposeDefaults {
    to: String,
    cc: String,
    bcc: String,
    subject: String,
    reply_to: String,
    in_reply_to: String,
    priority: String,
    sender_name: String,
    from_address: String,
    body_format: String,
    body: String,
    custom_headers: String,
}

fn defaults_from_query(query: &ComposePageQuery) -> ComposeDefaults {
    ComposeDefaults {
        to: query.to.clone(),
        cc: query.cc.clone(),
        bcc: query.bcc.clone(),
        subject: query.subject.clone(),
        reply_to: query.reply_to.clone(),
        in_reply_to: query.in_reply_to.clone(),
        priority: if query.priority.is_empty() {
            "normal".to_string()
        } else {
            query.priority.clone()
        },
        sender_name: query.sender_name.clone(),
        from_address: query.from_address.clone(),
        body_format: if query.body_format.is_empty() {
            "plain".to_string()
        } else {
            query.body_format.clone()
        },
        body: query.body.clone(),
        custom_headers: query.custom_headers.clone(),
    }
}

fn defaults_from_form(form: &ComposeForm) -> ComposeDefaults {
    ComposeDefaults {
        to: form.to.clone(),
        cc: form.cc.clone(),
        bcc: form.bcc.clone(),
        subject: form.subject.clone(),
        reply_to: form.reply_to.clone(),
        in_reply_to: form.in_reply_to.clone(),
        priority: if form.priority.is_empty() {
            "normal".to_string()
        } else {
            form.priority.clone()
        },
        sender_name: form.sender_name.clone(),
        from_address: form.from_address.clone(),
        body_format: if form.body_format.is_empty() {
            "plain".to_string()
        } else {
            form.body_format.clone()
        },
        body: form.body.clone(),
        custom_headers: form.custom_headers.clone(),
    }
}

// ── Folder scanning ──

fn scan_folders(maildir_base: &str) -> Vec<WebmailFolder> {
    let mut folders = vec![WebmailFolder {
        name: String::new(),
        display_name: "INBOX".to_string(),
        depth: 0,
    }];

    if let Ok(entries) = std::fs::read_dir(maildir_base) {
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                e.file_type().map(|t| t.is_dir()).unwrap_or(false)
                    && name.starts_with('.')
                    && std::path::Path::new(&format!(
                        "{}/{}/cur",
                        maildir_base,
                        e.file_name().to_string_lossy()
                    ))
                    .is_dir()
            })
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        for name in names {
            // Depth = number of dots minus 1 (e.g., ".Sent" → depth 0, ".INBOX.Sent" → depth 1)
            let inner = name.trim_start_matches('.');
            let parts: Vec<&str> = inner.split('.').collect();
            let depth = parts.len().saturating_sub(1);
            let display_name = parts.last().copied().unwrap_or("").to_string();
            folders.push(WebmailFolder {
                name: name.clone(),
                display_name,
                depth,
            });
        }
    }
    folders
}

/// Walk up the folder name hierarchy (by stripping trailing `.component` segments)
/// and return the index of the first ancestor that already exists as a top-level group.
/// Returns `None` if no ancestor group is found.
fn find_ancestor_group(groups: &[WebmailFolderGroup], folder_name: &str) -> Option<usize> {
    let mut current = folder_name;
    loop {
        let parent_name = match current.rfind('.') {
            Some(p) if p > 0 => &current[..p],
            _ => return None,
        };
        if let Some(idx) = groups.iter().position(|g| g.folder.name == parent_name) {
            return Some(idx);
        }
        current = parent_name;
    }
}

/// Group a flat, sorted list of `WebmailFolder`s into a hierarchical structure.
/// Depth-0 folders become top-level `WebmailFolderGroup`s; deeper folders are
/// placed as children of their closest ancestor group.  Groups are marked `open`
/// when `current_folder` is the group root or one of its children.
fn group_folders(folders: Vec<WebmailFolder>, current_folder: &str) -> Vec<WebmailFolderGroup> {
    let mut groups: Vec<WebmailFolderGroup> = Vec::new();

    for folder in folders {
        if folder.depth == 0 {
            groups.push(WebmailFolderGroup {
                folder,
                children: Vec::new(),
                open: false,
            });
        } else {
            let folder_name = folder.name.clone();
            match find_ancestor_group(&groups, &folder_name) {
                Some(idx) => groups[idx].children.push(folder),
                None => {
                    warn!(
                        "[web] folder '{}' has no known parent group, adding as top-level",
                        folder_name
                    );
                    groups.push(WebmailFolderGroup {
                        folder,
                        children: Vec::new(),
                        open: false,
                    });
                }
            }
        }
    }

    for group in &mut groups {
        if group.folder.name == current_folder
            || group.children.iter().any(|c| c.name == current_folder)
        {
            group.open = true;
        }
    }

    groups
}

// ── Email reading ──

pub(crate) fn read_emails(maildir_base: &str, folder: &str, logs: &mut Vec<String>) -> Vec<WebmailEmail> {
    let root = folder_root(maildir_base, folder);
    let mut emails = Vec::new();

    // Create Maildir directories if they don't exist (INBOX only)
    if folder.is_empty() {
        for subdir in &["new", "cur", "tmp"] {
            let dir_path = format!("{}/{}", root, subdir);
            if let Err(e) = std::fs::create_dir_all(&dir_path) {
                logs.push(format!(
                    "Warning: Failed to create directory {}: {}",
                    dir_path, e
                ));
                warn!(
                    "[web] failed to create maildir directory {}: {}",
                    dir_path, e
                );
            }
        }
    }

    for (subdir, is_new) in &[("new", true), ("cur", false)] {
        let dir_path = format!("{}/{}", root, subdir);
        logs.push(format!("Scanning directory: {}", dir_path));
        match std::fs::read_dir(&dir_path) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }
                    let fname = entry.file_name().to_string_lossy().to_string();
                    match std::fs::read(&path) {
                        Ok(data) => match mailparse::parse_mail(&data) {
                            Ok(parsed) => {
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
                                let encoded = URL_SAFE_NO_PAD.encode(fname.as_bytes());
                                emails.push(WebmailEmail {
                                    filename: encoded,
                                    subject,
                                    from,
                                    to,
                                    date,
                                    is_new: *is_new,
                                    is_spam,
                                });
                            }
                            Err(e) => {
                                logs.push(format!("Failed to parse email {}: {}", fname, e));
                                warn!("[web] failed to parse email {}: {}", fname, e);
                            }
                        },
                        Err(e) => {
                            logs.push(format!("Failed to read file {}: {}", fname, e));
                            warn!("[web] failed to read email file {}: {}", fname, e);
                        }
                    }
                }
            }
            Err(e) => {
                logs.push(format!(
                    "Directory {} not found or not readable: {}",
                    dir_path, e
                ));
                debug!("[web] maildir directory {} not accessible: {}", dir_path, e);
            }
        }
    }
    emails
}

// ── Templates ──

#[derive(Template)]
#[template(path = "webmail/inbox.html")]
struct InboxTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    accounts: Vec<Account>,
    selected_account: Option<Account>,
    emails: Vec<WebmailEmail>,
    folder_groups: Vec<WebmailFolderGroup>,
    current_folder: String,
    current_folder_name: String,
    current_page: usize,
    total_pages: usize,
    prev_page: Option<usize>,
    next_page: Option<usize>,
    logs: Vec<String>,
}

#[derive(Template)]
#[template(path = "webmail/view.html")]
struct ViewTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    account: Account,
    subject: String,
    from: String,
    to: String,
    date: String,
    body: String,
    current_folder: String,
    current_folder_name: String,
    filename_b64: String,
    is_spam: bool,
}

#[derive(Template)]
#[template(path = "webmail/compose.html")]
struct ComposeTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    accounts: Vec<Account>,
    selected_account: Option<Account>,
    defaults: ComposeDefaults,
    send_log: Vec<String>,
}

// ── Handlers ──

pub async fn inbox(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Query(query): Query<WebmailQuery>,
) -> Html<String> {
    info!("[web] GET /webmail — inbox");
    let mut logs: Vec<String> = Vec::new();

    let accounts = state
        .blocking_db(|db| db.list_all_accounts_with_domain())
        .await;
    logs.push(format!("Loaded {} accounts from database", accounts.len()));

    let mut selected_account: Option<Account> = None;
    let mut all_emails: Vec<WebmailEmail> = Vec::new();
    let mut raw_folders: Vec<WebmailFolder> = Vec::new();

    let current_folder = query
        .folder
        .as_deref()
        .filter(|f| is_safe_folder(f))
        .unwrap_or("")
        .to_string();

    if let Some(account_id) = query.account_id {
        logs.push(format!("Account ID {} selected", account_id));
        let acct = state
            .blocking_db(move |db| db.get_account_with_domain(account_id))
            .await;
        if let Some(acct) = acct {
            let domain = acct.domain_name.as_deref().unwrap_or("unknown");
            if !is_safe_path_component(domain) || !is_safe_path_component(&acct.username) {
                logs.push("Invalid domain or username for path construction".to_string());
                warn!(
                    "[web] unsafe path component: domain={}, username={}",
                    domain, acct.username
                );
                selected_account = Some(acct);
            } else {
                let maildir_base = maildir_path(domain, &acct.username);
                logs.push(format!("Maildir path: {}", maildir_base));

                raw_folders = scan_folders(&maildir_base);
                all_emails = read_emails(&maildir_base, &current_folder, &mut logs);
                logs.push(format!("Total emails found: {}", all_emails.len()));
                selected_account = Some(acct);
            }
        } else {
            logs.push(format!("Account ID {} not found in database", account_id));
            warn!("[web] account id={} not found for webmail", account_id);
        }
    } else {
        logs.push("No account selected".to_string());
    }

    // Pagination
    let total = all_emails.len();
    let total_pages = if total == 0 {
        1
    } else {
        (total + PAGE_SIZE - 1) / PAGE_SIZE
    };
    let current_page = query.page.unwrap_or(1).max(1).min(total_pages);
    let start = (current_page - 1) * PAGE_SIZE;
    let end = (start + PAGE_SIZE).min(total);
    let emails = all_emails
        .into_iter()
        .skip(start)
        .take(end - start)
        .collect();

    let prev_page = if current_page > 1 {
        Some(current_page - 1)
    } else {
        None
    };
    let next_page = if current_page < total_pages {
        Some(current_page + 1)
    } else {
        None
    };

    let current_folder_name = if current_folder.is_empty() {
        "INBOX".to_string()
    } else {
        current_folder.trim_start_matches('.').to_string()
    };

    let folder_groups = group_folders(raw_folders, &current_folder);

    let tmpl = InboxTemplate {
        nav_active: "Webmail",
        flash: None,
        accounts,
        selected_account,
        emails,
        folder_groups,
        current_folder,
        current_folder_name,
        current_page,
        total_pages,
        prev_page,
        next_page,
        logs,
    };
    Html(tmpl.render().unwrap())
}

pub async fn view_email(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(filename_b64): Path<String>,
    Query(query): Query<WebmailQuery>,
) -> Response {
    info!("[web] GET /webmail/view/{} — viewing email", filename_b64);

    let account_id = match query.account_id {
        Some(id) => id,
        None => {
            warn!("[web] no account_id provided for email view");
            return Html("Missing account_id parameter".to_string()).into_response();
        }
    };

    let acct = match state
        .blocking_db(move |db| db.get_account_with_domain(account_id))
        .await
    {
        Some(a) => a,
        None => {
            warn!("[web] account id={} not found for email view", account_id);
            return Html("Account not found".to_string()).into_response();
        }
    };

    let filename = match URL_SAFE_NO_PAD.decode(filename_b64.as_bytes()) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                error!("[web] invalid UTF-8 in decoded filename");
                return Html("Invalid filename encoding".to_string()).into_response();
            }
        },
        Err(e) => {
            error!("[web] failed to decode base64 filename: {}", e);
            return Html("Invalid filename encoding".to_string()).into_response();
        }
    };

    debug!("[web] decoded filename: {}", filename);

    let domain = acct.domain_name.as_deref().unwrap_or("unknown");
    if !is_safe_path_component(domain)
        || !is_safe_path_component(&acct.username)
        || !is_safe_path_component(&filename)
    {
        warn!("[web] unsafe path component in view_email");
        return Html("Invalid path component".to_string()).into_response();
    }
    let maildir_base = maildir_path(domain, &acct.username);

    let current_folder = query
        .folder
        .as_deref()
        .filter(|f| is_safe_folder(f))
        .unwrap_or("")
        .to_string();

    let root = folder_root(&maildir_base, &current_folder);

    // Search in both new/ and cur/
    let mut file_path = None;
    for subdir in &["new", "cur"] {
        let candidate = format!("{}/{}/{}", root, subdir, filename);
        debug!("[web] checking path: {}", candidate);
        if std::path::Path::new(&candidate).is_file() {
            file_path = Some(candidate);
            break;
        }
    }

    let file_path = match file_path {
        Some(p) => p,
        None => {
            warn!("[web] email file not found: {}", filename);
            return Html("Email not found".to_string()).into_response();
        }
    };

    debug!("[web] reading email from: {}", file_path);
    let data = match std::fs::read(&file_path) {
        Ok(d) => d,
        Err(e) => {
            error!("[web] failed to read email file: {}", e);
            return Html("Failed to read email".to_string()).into_response();
        }
    };

    let parsed = match mailparse::parse_mail(&data) {
        Ok(p) => p,
        Err(e) => {
            error!("[web] failed to parse email: {}", e);
            return Html("Failed to parse email".to_string()).into_response();
        }
    };

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

    // Extract body: prefer text/plain, fall back to text/html (escaped)
    let body = extract_body(&parsed);
    debug!(
        "[web] parsed email: subject={}, from={}, body_len={}",
        subject,
        from,
        body.len()
    );

    let folder_name = if current_folder.is_empty() {
        "INBOX".to_string()
    } else {
        current_folder.trim_start_matches('.').to_string()
    };

    let tmpl = ViewTemplate {
        nav_active: "Webmail",
        flash: None,
        account: acct,
        subject,
        from,
        to,
        date,
        body,
        current_folder,
        current_folder_name: folder_name,
        filename_b64: filename_b64.clone(),
        is_spam,
    };
    Html(tmpl.render().unwrap()).into_response()
}

pub async fn download_email(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(filename_b64): Path<String>,
    Query(query): Query<WebmailQuery>,
) -> Response {
    info!(
        "[web] GET /webmail/download/{} — downloading email",
        filename_b64
    );

    let account_id = match query.account_id {
        Some(id) => id,
        None => {
            warn!("[web] no account_id provided for email download");
            return Html("Missing account_id parameter".to_string()).into_response();
        }
    };

    let acct = match state
        .blocking_db(move |db| db.get_account_with_domain(account_id))
        .await
    {
        Some(a) => a,
        None => {
            warn!(
                "[web] account id={} not found for email download",
                account_id
            );
            return Html("Account not found".to_string()).into_response();
        }
    };

    let filename = match URL_SAFE_NO_PAD.decode(filename_b64.as_bytes()) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                error!("[web] invalid UTF-8 in decoded filename for download");
                return Html("Invalid filename encoding".to_string()).into_response();
            }
        },
        Err(e) => {
            error!("[web] failed to decode base64 filename for download: {}", e);
            return Html("Invalid filename encoding".to_string()).into_response();
        }
    };

    let domain = acct.domain_name.as_deref().unwrap_or("unknown");
    let current_folder = query
        .folder
        .as_deref()
        .filter(|f| is_safe_folder(f))
        .unwrap_or("")
        .to_string();

    if !is_safe_path_component(domain)
        || !is_safe_path_component(&acct.username)
        || !is_safe_path_component(&filename)
        || !is_safe_folder(&current_folder)
    {
        warn!("[web] unsafe path component in download_email");
        return Html("Invalid path component".to_string()).into_response();
    }

    let maildir_base = maildir_path(domain, &acct.username);
    let root = folder_root(&maildir_base, &current_folder);

    let mut file_path = None;
    for subdir in &["new", "cur"] {
        let candidate = format!("{}/{}/{}", root, subdir, filename);
        if std::path::Path::new(&candidate).is_file() {
            file_path = Some(candidate);
            break;
        }
    }

    let file_path = match file_path {
        Some(p) => p,
        None => {
            warn!("[web] email file not found for download: {}", filename);
            return Html("Email not found".to_string()).into_response();
        }
    };

    let data = match std::fs::read(&file_path) {
        Ok(d) => d,
        Err(e) => {
            error!("[web] failed to read email file for download: {}", e);
            return Html("Failed to read email".to_string()).into_response();
        }
    };

    let safe_name = format!(
        "{}.eml",
        filename.replace(['"', '\\', '/', ':'], "_")
    );
    let encoded_name = urlencoding_simple(&safe_name);
    (
        [
            (header::CONTENT_TYPE, "message/rfc822".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!(
                    "attachment; filename=\"{}\"; filename*=UTF-8''{}",
                    safe_name, encoded_name
                ),
            ),
        ],
        data,
    )
        .into_response()
}

pub async fn reply_email(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(filename_b64): Path<String>,
    Query(query): Query<WebmailQuery>,
) -> Response {
    info!(
        "[web] GET /webmail/reply/{} — preparing reply",
        filename_b64
    );

    let account_id = match query.account_id {
        Some(id) => id,
        None => {
            warn!("[web] no account_id provided for email reply");
            return Html("Missing account_id parameter".to_string()).into_response();
        }
    };

    let acct = match state
        .blocking_db(move |db| db.get_account_with_domain(account_id))
        .await
    {
        Some(a) => a,
        None => {
            warn!("[web] account id={} not found for email reply", account_id);
            return Html("Account not found".to_string()).into_response();
        }
    };

    let filename = match URL_SAFE_NO_PAD.decode(filename_b64.as_bytes()) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                error!("[web] invalid UTF-8 in decoded filename for reply");
                return Html("Invalid filename encoding".to_string()).into_response();
            }
        },
        Err(e) => {
            error!("[web] failed to decode base64 filename for reply: {}", e);
            return Html("Invalid filename encoding".to_string()).into_response();
        }
    };

    let domain = acct.domain_name.as_deref().unwrap_or("unknown");
    let current_folder = query
        .folder
        .as_deref()
        .filter(|f| is_safe_folder(f))
        .unwrap_or("")
        .to_string();

    if !is_safe_path_component(domain)
        || !is_safe_path_component(&acct.username)
        || !is_safe_path_component(&filename)
        || !is_safe_folder(&current_folder)
    {
        warn!("[web] unsafe path component in reply_email");
        return Html("Invalid path component".to_string()).into_response();
    }

    let maildir_base = maildir_path(domain, &acct.username);
    let root = folder_root(&maildir_base, &current_folder);

    let mut file_path = None;
    for subdir in &["new", "cur"] {
        let candidate = format!("{}/{}/{}", root, subdir, filename);
        if std::path::Path::new(&candidate).is_file() {
            file_path = Some(candidate);
            break;
        }
    }

    let file_path = match file_path {
        Some(p) => p,
        None => {
            warn!("[web] email file not found for reply: {}", filename);
            return Html("Email not found".to_string()).into_response();
        }
    };

    let data = match std::fs::read(&file_path) {
        Ok(d) => d,
        Err(e) => {
            error!("[web] failed to read email file for reply: {}", e);
            return Html("Failed to read email".to_string()).into_response();
        }
    };

    let parsed = match mailparse::parse_mail(&data) {
        Ok(p) => p,
        Err(e) => {
            error!("[web] failed to parse email for reply: {}", e);
            return Html("Failed to parse email".to_string()).into_response();
        }
    };

    let subject_raw = parsed
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
    let reply_to = parsed
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case("Reply-To"))
        .map(|h| h.get_value())
        .unwrap_or_default();
    let message_id = parsed
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case("Message-ID"))
        .map(|h| h.get_value())
        .unwrap_or_default();
    let body = extract_body(&parsed);

    let mut defaults = ComposeDefaults {
        priority: "normal".to_string(),
        body_format: "plain".to_string(),
        ..ComposeDefaults::default()
    };
    let recipient = if !reply_to.trim().is_empty() {
        reply_to
    } else {
        from.clone()
    };
    let reply_subject = if subject_raw.to_lowercase().starts_with("re:") {
        subject_raw
    } else if subject_raw.is_empty() {
        "Re:".to_string()
    } else {
        format!("Re: {}", subject_raw)
    };

    defaults.to = sanitize_header_value(&recipient);
    defaults.subject = sanitize_header_value(&reply_subject);
    defaults.in_reply_to = sanitize_header_value(&message_id);
    if !body.is_empty() {
        let quoted = body
            .lines()
            .fold(String::new(), |mut acc, line| {
                if !acc.is_empty() {
                    acc.push('\n');
                }
                acc.push_str("> ");
                acc.push_str(line);
                acc
            });
        defaults.body = format!("\n\n{}", quoted);
    }

    let accounts = state
        .blocking_db(|db| db.list_all_accounts_with_domain())
        .await;

    let tmpl = ComposeTemplate {
        nav_active: "Webmail",
        flash: None,
        accounts,
        selected_account: Some(acct),
        defaults,
        send_log: Vec::new(),
    };

    Html(tmpl.render().unwrap()).into_response()
}

pub async fn delete_email(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(filename_b64): Path<String>,
    Form(form): Form<DeleteForm>,
) -> Response {
    info!(
        "[web] POST /webmail/delete/{} — deleting email",
        filename_b64
    );

    let acct = match state
        .blocking_db(move |db| db.get_account_with_domain(form.account_id))
        .await
    {
        Some(a) => a,
        None => {
            warn!("[web] account not found for delete");
            return Html("Account not found".to_string()).into_response();
        }
    };

    let filename = match URL_SAFE_NO_PAD.decode(filename_b64.as_bytes()) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                error!("[web] invalid UTF-8 in decoded filename for delete");
                return Html("Invalid filename encoding".to_string()).into_response();
            }
        },
        Err(e) => {
            error!("[web] failed to decode base64 filename for delete: {}", e);
            return Html("Invalid filename encoding".to_string()).into_response();
        }
    };

    let domain = acct.domain_name.as_deref().unwrap_or("unknown");
    let folder = form.folder.as_deref().unwrap_or("");
    if !is_safe_path_component(domain)
        || !is_safe_path_component(&acct.username)
        || !is_safe_path_component(&filename)
        || !is_safe_folder(folder)
    {
        warn!("[web] unsafe path component in delete_email");
        return Html("Invalid path component".to_string()).into_response();
    }

    let maildir_base = maildir_path(domain, &acct.username);
    let root = folder_root(&maildir_base, folder);

    let mut deleted = false;
    for subdir in &["new", "cur"] {
        let candidate = format!("{}/{}/{}", root, subdir, filename);
        if std::path::Path::new(&candidate).is_file() {
            if let Err(e) = std::fs::remove_file(&candidate) {
                error!("[web] failed to delete email file {}: {}", candidate, e);
                return Html(format!("Failed to delete email: {}", e)).into_response();
            }
            info!("[web] deleted email file: {}", candidate);
            deleted = true;
            break;
        }
    }

    if !deleted {
        warn!("[web] email file not found for deletion: {}", filename);
    }

    let redirect_url = format!(
        "/webmail?account_id={}&folder={}",
        acct.id,
        urlencoding_simple(folder)
    );
    Redirect::to(&redirect_url).into_response()
}

fn urlencoding_simple(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                vec![c]
            } else {
                format!("%{:02X}", c as u32).chars().collect()
            }
        })
        .collect()
}

pub(crate) fn extract_body(parsed: &mailparse::ParsedMail) -> String {
    // Try to find text/plain part first
    if let Some(text) = find_body_part(parsed, "text/plain") {
        return text;
    }
    // Fall back to text/html (escape HTML for safe display)
    if let Some(html) = find_body_part(parsed, "text/html") {
        return html
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
    }
    // Last resort: try top-level body
    parsed.get_body().unwrap_or_default()
}

pub(crate) fn find_body_part(parsed: &mailparse::ParsedMail, mime_type: &str) -> Option<String> {
    if parsed.subparts.is_empty() {
        let ctype = parsed.ctype.mimetype.to_lowercase();
        if ctype == mime_type {
            return parsed.get_body().ok();
        }
        return None;
    }
    for part in &parsed.subparts {
        if let Some(body) = find_body_part(part, mime_type) {
            return Some(body);
        }
    }
    None
}

pub async fn compose(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Query(query): Query<ComposePageQuery>,
) -> Html<String> {
    info!("[web] GET /webmail/compose — compose email form");

    let defaults = defaults_from_query(&query);
    let accounts = state
        .blocking_db(|db| db.list_all_accounts_with_domain())
        .await;

    let selected_account = if let Some(account_id) = query.account_id {
        state
            .blocking_db(move |db| db.get_account_with_domain(account_id))
            .await
    } else {
        None
    };

    let tmpl = ComposeTemplate {
        nav_active: "Webmail",
        flash: None,
        accounts,
        selected_account,
        defaults,
        send_log: Vec::new(),
    };
    Html(tmpl.render().unwrap())
}

pub async fn send_email(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<ComposeForm>,
) -> Html<String> {
    info!("[web] POST /webmail/send — sending email");
    let mut send_log: Vec<String> = Vec::new();
    let defaults = defaults_from_form(&form);
    let flash: Option<String>;

    send_log.push(format!("Looking up account ID {}", form.account_id));
    let account_id = form.account_id;
    let acct = state
        .blocking_db(move |db| db.get_account_with_domain(account_id))
        .await;

    let accounts = state
        .blocking_db(|db| db.list_all_accounts_with_domain())
        .await;

    match acct {
        Some(ref acct) => {
            let domain = acct.domain_name.as_deref().unwrap_or("unknown");
            let email_addr = format!("{}@{}", acct.username, domain);
            let sender_name = sanitize_header_value(form.sender_name.trim());
            let from_addr = if !form.from_address.trim().is_empty() {
                sanitize_header_value(form.from_address.trim())
            } else if sender_name.is_empty() {
                email_addr.clone()
            } else {
                format!("{} <{}>", sender_name, email_addr)
            };
            send_log.push(format!("From address: {}", from_addr));
            send_log.push(format!("To: {}", form.to));
            if !form.cc.trim().is_empty() {
                send_log.push(format!("CC: {}", form.cc));
            }
            if !form.bcc.trim().is_empty() {
                send_log.push(format!("BCC: {}", form.bcc));
            }
            if !form.reply_to.trim().is_empty() {
                send_log.push(format!("Reply-To: {}", form.reply_to));
            }
            if !form.in_reply_to.trim().is_empty() {
                send_log.push(format!("In-Reply-To: {}", form.in_reply_to));
            }
            if !form.priority.is_empty() && form.priority != "normal" {
                send_log.push(format!("Priority: {}", form.priority));
            }
            send_log.push(format!("Subject: {}", form.subject));
            send_log.push(format!("Body length: {} chars", form.body.len()));

            send_log.push("Building email message...".to_string());
            let mut builder = lettre::Message::builder()
                .from(from_addr.parse().unwrap_or_else(|e| {
                    send_log.push(format!(
                        "Warning: could not parse from address '{}': {}, using fallback",
                        from_addr, e
                    ));
                    "noreply@localhost".parse().unwrap()
                }))
                .to(match form.to.parse() {
                    Ok(addr) => addr,
                    Err(e) => {
                        send_log.push(format!("Invalid To address: {}", e));
                        error!("[web] invalid To address {}: {}", form.to, e);
                        flash = Some(format!("Invalid To address: {}", e));
                        let tmpl = ComposeTemplate {
                            nav_active: "Webmail",
                            flash: flash.as_deref(),
                            accounts,
                            selected_account: Some(acct.clone()),
                            defaults: defaults.clone(),
                            send_log,
                        };
                        return Html(tmpl.render().unwrap());
                    }
                })
                .subject(&form.subject);

            // Add CC recipients
            for addr in form.cc.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                match addr.parse() {
                    Ok(a) => builder = builder.cc(a),
                    Err(e) => send_log.push(format!(
                        "Warning: skipping invalid CC address '{}': {}",
                        addr, e
                    )),
                }
            }

            // Add BCC recipients
            for addr in form.bcc.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                match addr.parse() {
                    Ok(a) => builder = builder.bcc(a),
                    Err(e) => send_log.push(format!(
                        "Warning: skipping invalid BCC address '{}': {}",
                        addr, e
                    )),
                }
            }

            // Set Reply-To
            if !form.reply_to.trim().is_empty() {
                match form.reply_to.trim().parse() {
                    Ok(a) => builder = builder.reply_to(a),
                    Err(e) => send_log.push(format!(
                        "Warning: invalid Reply-To address '{}': {}",
                        form.reply_to, e
                    )),
                }
            }

            // Set In-Reply-To
            if !form.in_reply_to.trim().is_empty() {
                let in_reply_to = sanitize_header_value(form.in_reply_to.trim());
                builder = builder.in_reply_to(in_reply_to);
            }

            // Set priority via X-Priority header
            {
                use lettre::message::header::{HeaderName, HeaderValue};
                let priority_value = match form.priority.as_str() {
                    "lowest" => Some("5 (Lowest)"),
                    "low" => Some("4 (Low)"),
                    "high" => Some("2 (High)"),
                    "highest" => Some("1 (Highest)"),
                    _ => None, // "normal" or empty — no header needed
                };
                if let Some(val) = priority_value {
                    if let Ok(header_name) = HeaderName::new_from_ascii("X-Priority".to_string()) {
                        builder =
                            builder.raw_header(HeaderValue::new(header_name, val.to_string()));
                    }
                }
            }

            // Add custom headers (one per line, format: "Header-Name: value")
            {
                use lettre::message::header::{HeaderName, HeaderValue};
                for line in form
                    .custom_headers
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                {
                    if let Some((name, value)) = line.split_once(':') {
                        let name = name.trim();
                        let value = sanitize_header_value(value.trim());
                        if !name.is_empty() && !value.is_empty() {
                            match HeaderName::new_from_ascii(name.to_string()) {
                                Ok(header_name) => {
                                    builder = builder.raw_header(HeaderValue::new(
                                        header_name,
                                        value.to_string(),
                                    ));
                                    send_log.push(format!("Custom header: {}: {}", name, value));
                                }
                                Err(e) => {
                                    send_log.push(format!(
                                        "Warning: invalid header name '{}': {}",
                                        name, e
                                    ));
                                }
                            }
                        }
                    }
                }
            }

            let body_format = form.body_format.as_str();
            send_log.push(format!(
                "Body format: {}",
                if body_format.is_empty() {
                    "plain"
                } else {
                    body_format
                }
            ));
            use lettre::message::header::ContentType;
            use lettre::message::{MultiPart, SinglePart};

            let email = match body_format {
                "html" => {
                    match builder.singlepart(
                        SinglePart::builder()
                            .header(ContentType::TEXT_HTML)
                            .body(form.body.clone()),
                    ) {
                        Ok(email) => {
                            send_log.push("Email message built successfully (HTML)".to_string());
                            email
                        }
                        Err(e) => {
                            send_log.push(format!("Failed to build email: {}", e));
                            error!("[web] failed to build email: {}", e);
                            flash = Some(format!("Failed to build email: {}", e));
                            let tmpl = ComposeTemplate {
                                nav_active: "Webmail",
                                flash: flash.as_deref(),
                                accounts,
                                selected_account: Some(acct.clone()),
                                defaults: defaults.clone(),
                                send_log,
                            };
                            return Html(tmpl.render().unwrap());
                        }
                    }
                }
                "both" => {
                    match builder.multipart(
                        MultiPart::alternative()
                            .singlepart(
                                SinglePart::builder()
                                    .header(ContentType::TEXT_PLAIN)
                                    .body(form.body.clone()),
                            )
                            .singlepart(
                                SinglePart::builder()
                                    .header(ContentType::TEXT_HTML)
                                    .body(form.body.clone()),
                            ),
                    ) {
                        Ok(email) => {
                            send_log.push(
                                "Email message built successfully (plain + HTML)".to_string(),
                            );
                            email
                        }
                        Err(e) => {
                            send_log.push(format!("Failed to build email: {}", e));
                            error!("[web] failed to build email: {}", e);
                            flash = Some(format!("Failed to build email: {}", e));
                            let tmpl = ComposeTemplate {
                                nav_active: "Webmail",
                                flash: flash.as_deref(),
                                accounts,
                                selected_account: Some(acct.clone()),
                                defaults: defaults.clone(),
                                send_log,
                            };
                            return Html(tmpl.render().unwrap());
                        }
                    }
                }
                // "plain" or any unrecognised value — default to plain text
                _ => match builder.body(form.body.clone()) {
                    Ok(email) => {
                        send_log.push("Email message built successfully (plain text)".to_string());
                        email
                    }
                    Err(e) => {
                        send_log.push(format!("Failed to build email: {}", e));
                        error!("[web] failed to build email: {}", e);
                        flash = Some(format!("Failed to build email: {}", e));
                        let tmpl = ComposeTemplate {
                            nav_active: "Webmail",
                            flash: flash.as_deref(),
                            accounts,
                            selected_account: Some(acct.clone()),
                            defaults: defaults.clone(),
                            send_log,
                        };
                        return Html(tmpl.render().unwrap());
                    }
                },
            };

            send_log.push("Connecting to SMTP server at 127.0.0.1:25...".to_string());
            use lettre::{SmtpTransport, Transport};
            // builder_dangerous disables TLS — safe here because we connect to the
            // local Postfix instance on the loopback interface (same as filter.rs).
            match SmtpTransport::builder_dangerous("127.0.0.1")
                .port(25)
                .build()
                .send(&email)
            {
                Ok(response) => {
                    send_log.push(format!("SMTP response: {:?}", response));
                    send_log.push("Email sent successfully!".to_string());
                    info!("[web] email sent successfully to {}", form.to);
                    flash = Some("Email sent successfully!".to_string());
                }
                Err(e) => {
                    send_log.push(format!("SMTP error: {}", e));
                    error!("[web] failed to send email: {}", e);
                    flash = Some(format!("Failed to send email: {}", e));
                }
            }

            let tmpl = ComposeTemplate {
                nav_active: "Webmail",
                flash: flash.as_deref(),
                accounts,
                selected_account: Some(acct.clone()),
                defaults: defaults.clone(),
                send_log,
            };
            Html(tmpl.render().unwrap())
        }
        None => {
            send_log.push(format!("Account ID {} not found!", form.account_id));
            error!(
                "[web] account id={} not found for sending email",
                form.account_id
            );
            flash = Some("Account not found".to_string());
            let tmpl = ComposeTemplate {
                nav_active: "Webmail",
                flash: flash.as_deref(),
                accounts,
                selected_account: None,
                defaults,
                send_log,
            };
            Html(tmpl.render().unwrap())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        defaults_from_form, defaults_from_query, group_folders, is_safe_folder, maildir_path,
        ComposeForm, ComposePageQuery, WebmailFolder,
    };

    #[test]
    fn maildir_path_uses_data_mail_root() {
        let path = maildir_path("example.com", "alice");
        assert_eq!(path, "/data/mail/example.com/alice/Maildir");
    }

    #[test]
    fn safe_folder_empty_is_inbox() {
        assert!(is_safe_folder(""));
    }

    #[test]
    fn safe_folder_valid_names() {
        assert!(is_safe_folder(".Sent"));
        assert!(is_safe_folder(".Drafts"));
        assert!(is_safe_folder(".INBOX.Subfolder"));
    }

    #[test]
    fn safe_folder_rejects_path_traversal() {
        assert!(!is_safe_folder(".."));
        assert!(!is_safe_folder("../etc"));
        assert!(!is_safe_folder(".Sent/../../etc"));
        assert!(!is_safe_folder("noLeadingDot"));
    }

    #[test]
    fn group_folders_flat_no_children() {
        let folders = vec![
            WebmailFolder {
                name: String::new(),
                display_name: "INBOX".into(),
                depth: 0,
            },
            WebmailFolder {
                name: ".Sent".into(),
                display_name: "Sent".into(),
                depth: 0,
            },
            WebmailFolder {
                name: ".Drafts".into(),
                display_name: "Drafts".into(),
                depth: 0,
            },
        ];
        let groups = group_folders(folders, "");
        assert_eq!(groups.len(), 3);
        assert!(groups.iter().all(|g| g.children.is_empty()));
    }

    #[test]
    fn group_folders_nests_child_under_parent() {
        let folders = vec![
            WebmailFolder {
                name: String::new(),
                display_name: "INBOX".into(),
                depth: 0,
            },
            WebmailFolder {
                name: ".Archive".into(),
                display_name: "Archive".into(),
                depth: 0,
            },
            WebmailFolder {
                name: ".Archive.2023".into(),
                display_name: "2023".into(),
                depth: 1,
            },
        ];
        let groups = group_folders(folders, "");
        assert_eq!(groups.len(), 2);
        let archive = groups.iter().find(|g| g.folder.name == ".Archive").unwrap();
        assert_eq!(archive.children.len(), 1);
        assert_eq!(archive.children[0].name, ".Archive.2023");
    }

    #[test]
    fn group_folders_marks_open_for_current_folder() {
        let folders = vec![
            WebmailFolder {
                name: ".Archive".into(),
                display_name: "Archive".into(),
                depth: 0,
            },
            WebmailFolder {
                name: ".Archive.2023".into(),
                display_name: "2023".into(),
                depth: 1,
            },
        ];
        let groups = group_folders(folders, ".Archive.2023");
        let archive = groups.iter().find(|g| g.folder.name == ".Archive").unwrap();
        assert!(archive.open);
    }

    #[test]
    fn group_folders_nests_grandchild_under_root_ancestor() {
        let folders = vec![
            WebmailFolder {
                name: ".Archive".into(),
                display_name: "Archive".into(),
                depth: 0,
            },
            WebmailFolder {
                name: ".Archive.2023".into(),
                display_name: "2023".into(),
                depth: 1,
            },
            WebmailFolder {
                name: ".Archive.2023.Q1".into(),
                display_name: "Q1".into(),
                depth: 2,
            },
        ];
        let groups = group_folders(folders, "");
        // Grandchild is nested under the root ancestor group's children
        let archive = groups.iter().find(|g| g.folder.name == ".Archive").unwrap();
        assert_eq!(archive.children.len(), 2);
        assert!(archive
            .children
            .iter()
            .any(|c| c.name == ".Archive.2023.Q1"));
    }

    #[test]
    fn compose_defaults_from_query_sets_baseline_values() {
        let query = ComposePageQuery::default();
        let defaults = defaults_from_query(&query);
        assert_eq!(defaults.priority, "normal");
        assert_eq!(defaults.body_format, "plain");
    }

    #[test]
    fn compose_defaults_from_form_preserves_user_input() {
        let form = ComposeForm {
            account_id: 1,
            to: "to@example.com".into(),
            cc: "cc@example.com".into(),
            bcc: "bcc@example.com".into(),
            subject: "Hello".into(),
            reply_to: "reply@example.com".into(),
            in_reply_to: "<message-id@example.com>".into(),
            priority: "high".into(),
            sender_name: "Alice".into(),
            from_address: "alice@example.com".into(),
            body_format: "html".into(),
            custom_headers: "X-Test: 1".into(),
            body: "<p>Hi</p>".into(),
        };

        let defaults = defaults_from_form(&form);
        assert_eq!(defaults.to, "to@example.com");
        assert_eq!(defaults.cc, "cc@example.com");
        assert_eq!(defaults.subject, "Hello");
        assert_eq!(defaults.priority, "high");
        assert_eq!(defaults.body_format, "html");
        assert_eq!(defaults.in_reply_to, "<message-id@example.com>");
    }
}
