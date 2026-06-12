use serde_json::{json, Value};

use super::super::AppState;
use super::{get_session_state, JmapAuth};

// ── Thread/get ───────────────────────────────────────────────────────────────

pub async fn thread_get(
    state: &AppState,
    auth: &JmapAuth,
    args: &Value,
) -> super::DispatchResult {
    let account_id = auth.account_id;
    let mdir = super::maildir_path(
        auth.account.domain_name.as_deref().unwrap_or(""),
        &auth.account.username,
    );

    let ids: Vec<String> = match args.get("ids") {
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        _ => {
            return super::DispatchResult {
                name: "Thread/get".to_string(),
                args: json!({
                    "accountId": account_id.to_string(),
                    "state": get_session_state(state, account_id).await,
                    "list": [],
                    "notFound": [],
                }),
            };
        }
    };

    // Build thread membership by scanning all emails
    let mut thread_map: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    let mut email_to_thread: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    let folders = super::scan_maildir_folders(&mdir);
    for (dir_name, _) in &folders {
        let mb_id = super::mailbox_id_from_dir(dir_name);
        let mb_dir = super::mailbox_dir(&mdir, &mb_id);

        for sub in &["new", "cur"] {
            let dir = format!("{}/{}", mb_dir, sub);
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let path = entry.path();
                    if let Ok(data) = std::fs::read(&path) {
                        if let Ok(msg) = mailparse::parse_mail(&data) {
                            let headers = super::email::extract_headers(&msg);
                            let msg_id = headers.get("message-id").cloned().unwrap_or_default();
                            let in_reply_to =
                                headers.get("in-reply-to").cloned().unwrap_or_default();
                            let references =
                                headers.get("references").cloned().unwrap_or_default();
                            let subject = headers.get("subject").cloned().unwrap_or_default();

                            let tid = super::email::compute_thread_id(
                                &msg_id, &in_reply_to, &references, &subject,
                            );
                            let email_id = format!("email:{}", name);
                            thread_map
                                .entry(tid.clone())
                                .or_default()
                                .push(email_id.clone());
                            email_to_thread.insert(email_id, tid);
                        }
                    }
                }
            }
        }
    }

    let mut threads = Vec::new();
    let mut not_found = Vec::new();

    for tid in &ids {
        if let Some(email_ids) = thread_map.get(tid) {
            threads.push(json!({
                "id": tid,
                "emailIds": email_ids,
            }));
        } else {
            not_found.push(tid.clone());
        }
    }

    super::DispatchResult {
        name: "Thread/get".to_string(),
        args: json!({
            "accountId": account_id.to_string(),
            "state": get_session_state(state, account_id).await,
            "list": threads,
            "notFound": not_found,
        }),
    }
}
