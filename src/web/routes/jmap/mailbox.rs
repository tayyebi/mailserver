use serde_json::{json, Value};

use super::super::AppState;
use super::{get_session_state, mailbox_dir, mailbox_id_from_dir, scan_maildir_folders, JmapAuth};

// ── Mailbox/get ───────────────────────────────────────────────────────────────

pub async fn mailbox_get(
    state: &AppState,
    auth: &JmapAuth,
    args: &Value,
) -> super::DispatchResult {
    let account_id = auth.account_id;

    let mdir = super::maildir_path(
        auth.account.domain_name.as_deref().unwrap_or(""),
        &auth.account.username,
    );

    let folders = scan_maildir_folders(&mdir);
    let mut mailboxes = Vec::new();

    for (dir_name, display_name) in &folders {
        let mailbox_id = mailbox_id_from_dir(dir_name);
        let mb_dir = mailbox_dir(&mdir, &mailbox_id);

        // Count total and unread
        let (total, unread) = count_emails(&mb_dir);

        let role = match display_name.as_str() {
            "INBOX" => Some("inbox"),
            "Sent" => Some("sent"),
            "Drafts" => Some("drafts"),
            "Junk" => Some("junk"),
            "Trash" => Some("trash"),
            "Archive" => Some("archive"),
            _ => None,
        };

        let sort_order = match role {
            Some("inbox") => 0,
            Some("sent") => 1,
            Some("drafts") => 2,
            Some("archive") => 3,
            Some("junk") => 4,
            Some("trash") => 5,
            _ => 10,
        };

        mailboxes.push(json!({
            "id": mailbox_id,
            "name": display_name,
            "parentId": null,
            "role": role,
            "sortOrder": sort_order,
            "totalEmails": total,
            "unreadEmails": unread,
            "totalThreads": total,
            "myRights": {
                "mayReadItems": true,
                "mayAddItems": false,
                "mayRemoveItems": false,
                "mayDelete": false,
                "mayRename": false,
            },
            "isSubscribed": true,
        }));
    }

    let session_state = get_session_state(state, account_id).await;

    let mut response = json!({
        "accountId": account_id.to_string(),
        "state": session_state,
        "list": mailboxes,
        "notFound": [],
    });

    // Handle properties filter
    if let Some(props) = args.get("properties").and_then(|p| p.as_array()) {
        let prop_names: Vec<&str> = props.iter().filter_map(|v| v.as_str()).collect();
        if let Some(list) = response.get_mut("list").and_then(|l| l.as_array_mut()) {
            for mb in list {
                if let Some(obj) = mb.as_object_mut() {
                    obj.retain(|k, _| prop_names.contains(&k.as_str()));
                }
            }
        }
    }

    super::DispatchResult {
        name: "Mailbox/get".to_string(),
        args: response,
    }
}

fn count_emails(mb_dir: &str) -> (i64, i64) {
    use std::fs;
    let mut total = 0i64;
    let mut unread = 0i64;

    // new/ = unread
    if let Ok(entries) = fs::read_dir(format!("{}/new", mb_dir)) {
        let count = entries.flatten().count() as i64;
        unread += count;
        total += count;
    }

    // cur/ = read + unread with flags
    if let Ok(entries) = fs::read_dir(format!("{}/cur", mb_dir)) {
        for entry in entries.flatten() {
            total += 1;
            let name = entry.file_name().to_string_lossy().to_string();
            // Maildir flags are after ",2," in the filename
            // Seen flag = :2,S — without S means unread
            if !name.contains(":2,") || !name.contains(",S") {
                unread += 1;
            }
        }
    }

    (total, unread)
}

// ── Mailbox/getChanges ────────────────────────────────────────────────────────

pub async fn mailbox_get_changes(
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

    // Scan current mailboxes
    let mdir = super::maildir_path(
        auth.account.domain_name.as_deref().unwrap_or(""),
        &auth.account.username,
    );
    let folders = scan_maildir_folders(&mdir);
    let current_ids: Vec<String> = folders.iter().map(|(_, d)| super::mailbox_id_from_dir(d)).collect();

    // Get stored snapshot from jmap_state
    let (current_state, stored_snapshot): (String, String) = state
        .blocking_db(move |db| db.get_jmap_state(account_id))
        .await;

    if since_state == current_state {
        return super::DispatchResult {
            name: "Mailbox/getChanges".to_string(),
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

    // Parse stored snapshot to get previous IDs
    let stored_ids: Vec<String> = if let Ok(v) = serde_json::from_str::<serde_json::Value>(&stored_snapshot) {
        v.as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default()
    } else {
        vec![]
    };

    let stored_set: std::collections::HashSet<&str> =
        stored_ids.iter().map(|s| s.as_str()).collect();
    let current_set: std::collections::HashSet<&str> =
        current_ids.iter().map(|s| s.as_str()).collect();

    let created: Vec<&str> = current_set.difference(&stored_set).copied().collect();
    let destroyed: Vec<&str> = stored_set.difference(&current_set).copied().collect();

    // Update snapshot and state if changed
    let new_state = format!("{}", current_state.parse::<i64>().unwrap_or(0) + 1);
    let new_state_clone = new_state.clone();
    let new_snapshot = serde_json::to_string(&current_ids).unwrap_or_default();

    state
        .blocking_db(move |db| {
            db.update_jmap_state(account_id, &new_state_clone, &new_snapshot);
        })
        .await;

    super::DispatchResult {
        name: "Mailbox/getChanges".to_string(),
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
