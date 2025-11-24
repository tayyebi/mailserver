/*
 * Statistics Collection and Aggregation
 * 
 * This module handles the collection, storage, and computation of tracking statistics.
 * 
 * Key responsibilities:
 * - Recording individual pixel requests in memory.
 * - Aggregating statistics from the file system (scanning metadata files).
 * - Computing metrics like total messages, open rates, unique IPs, and top user agents.
 * - Managing recent activity logs.
 * - Providing a thread-safe structure (`StatsCollector`) for the application state.
 */

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tracing::{debug, error, info, warn};

use crate::MessageMetadata;

#[derive(Debug, Clone, Serialize)]
pub struct SystemStats {
    pub total_messages: u32,
    pub tracked_messages: u32,
    pub opened_messages: u32,
    pub total_opens: u32,
    pub unique_ips: u32,
    pub recent_activity: Vec<RecentActivity>,
    pub top_user_agents: Vec<UserAgentStat>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecentActivity {
    pub message_id: String,
    pub timestamp: DateTime<Utc>,
    pub client_ip: String,
    pub user_agent: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct UserAgentStat {
    pub user_agent: String,
    pub count: u32,
}

#[derive(Debug)]
pub struct StatsCollector {
    pixel_requests: HashMap<String, Vec<PixelRequest>>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields stored for potential future use in statistics or debugging
struct PixelRequest {
    timestamp: DateTime<Utc>,
    client_ip: String,
}

impl StatsCollector {
    pub fn new() -> Self {
        Self {
            pixel_requests: HashMap::new(),
        }
    }

    pub fn record_pixel_request(&mut self, message_id: &str, client_ip: &str) {
        debug!(
            message_id = %message_id,
            client_ip = %client_ip,
            "Recording pixel request"
        );
        
        let request = PixelRequest {
            timestamp: Utc::now(),
            client_ip: client_ip.to_string(),
        };

        let previous_count = self.pixel_requests
            .get(message_id)
            .map(|v| v.len())
            .unwrap_or(0);
        
        self.pixel_requests
            .entry(message_id.to_string())
            .or_insert_with(Vec::new)
            .push(request);

        let new_count = self.pixel_requests
            .get(message_id)
            .map(|v| v.len())
            .unwrap_or(0);

        debug!(
            message_id = %message_id,
            previous_count = previous_count,
            new_count = new_count,
            "Pixel request recorded"
        );

        // Keep only recent requests (last 1000 per message)
        if let Some(requests) = self.pixel_requests.get_mut(message_id) {
            if requests.len() > 1000 {
                let removed = requests.len() - 1000;
                requests.drain(0..removed);
                debug!(
                    message_id = %message_id,
                    removed_count = removed,
                    remaining_count = requests.len(),
                    "Trimmed old requests for message"
                );
            }
        }

        // Limit total messages tracked in memory
        let total_messages = self.pixel_requests.len();
        if total_messages > 10000 {
            let to_remove = total_messages - 9000;
            warn!(
                total_messages = total_messages,
                to_remove = to_remove,
                "Memory limit reached, removing oldest message entries"
            );
            // Remove oldest entries
            let mut keys: Vec<_> = self.pixel_requests.keys().cloned().collect();
            keys.sort();
            for key in keys.iter().take(to_remove) {
                self.pixel_requests.remove(key);
            }
            info!(
                remaining_messages = self.pixel_requests.len(),
                removed_messages = to_remove,
                "Cleaned up old message entries"
            );
        }
    }

    pub async fn compute_stats(&self, data_dir: &PathBuf) -> Value {
        info!(data_dir = ?data_dir, "Starting statistics computation");
        
        let mut stats = SystemStats {
            total_messages: 0,
            tracked_messages: 0,
            opened_messages: 0,
            total_opens: 0,
            unique_ips: 0,
            recent_activity: Vec::new(),
            top_user_agents: Vec::new(),
        };

        let mut all_ips = std::collections::HashSet::new();
        let mut user_agent_counts: HashMap<String, u32> = HashMap::new();
        let mut recent_events = Vec::new();
        let mut processed_count = 0u32;
        let mut error_count = 0u32;

        debug!(data_dir = ?data_dir, "Scanning data directory for message metadata");
        // Scan data directory for message metadata
        if let Ok(entries) = fs::read_dir(data_dir) {
            for entry in entries.flatten() {
                if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                    let message_dir = entry.path();
                    let meta_file = message_dir.join("meta.json");

                    if meta_file.exists() {
                        processed_count += 1;
                        match self.process_message_metadata(&meta_file, &mut stats, &mut all_ips, &mut user_agent_counts, &mut recent_events) {
                            Ok(_) => {
                                debug!(
                                    meta_file = ?meta_file,
                                    processed_count = processed_count,
                                    "Processed metadata file"
                                );
                            }
                            Err(e) => {
                                error_count += 1;
                                error!(
                                    file = ?meta_file,
                                    error = %e,
                                    processed_count = processed_count,
                                    error_count = error_count,
                                    "Failed to process metadata file"
                                );
                            }
                        }
                    } else {
                        debug!(
                            message_dir = ?message_dir,
                            "Directory found but no meta.json file"
                        );
                    }
                }
            }
        } else {
            warn!(
                data_dir = ?data_dir,
                "Failed to read data directory"
            );
        }

        debug!(
            processed_count = processed_count,
            error_count = error_count,
            total_messages = stats.total_messages,
            "Finished scanning directory"
        );

        stats.unique_ips = all_ips.len() as u32;
        debug!(
            unique_ips = stats.unique_ips,
            "Computed unique IPs"
        );

        // Sort and limit recent activity
        debug!(
            recent_events_count = recent_events.len(),
            "Sorting recent activity"
        );
        recent_events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        stats.recent_activity = recent_events.into_iter().take(50).collect();
        debug!(
            recent_activity_count = stats.recent_activity.len(),
            "Limited recent activity"
        );

        // Sort user agents by count
        debug!(
            user_agent_count = user_agent_counts.len(),
            "Sorting user agents"
        );
        let mut ua_vec: Vec<_> = user_agent_counts.into_iter().collect();
        ua_vec.sort_by(|a, b| b.1.cmp(&a.1));
        stats.top_user_agents = ua_vec
            .into_iter()
            .take(10)
            .map(|(ua, count)| UserAgentStat {
                user_agent: ua,
                count,
            })
            .collect();
        debug!(
            top_user_agents_count = stats.top_user_agents.len(),
            "Limited top user agents"
        );

        info!(
            total_messages = stats.total_messages,
            tracked_messages = stats.tracked_messages,
            opened_messages = stats.opened_messages,
            total_opens = stats.total_opens,
            unique_ips = stats.unique_ips,
            processed_count = processed_count,
            error_count = error_count,
            "Statistics computation completed"
        );

        serde_json::to_value(stats).unwrap_or_else(|_| {
            error!("Failed to serialize statistics");
            serde_json::json!({})
        })
    }

    fn process_message_metadata(
        &self,
        meta_file: &PathBuf,
        stats: &mut SystemStats,
        all_ips: &mut std::collections::HashSet<String>,
        user_agent_counts: &mut HashMap<String, u32>,
        recent_events: &mut Vec<RecentActivity>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        debug!(meta_file = ?meta_file, "Reading metadata file");
        let content = fs::read_to_string(meta_file)?;
        debug!(
            meta_file = ?meta_file,
            content_size = content.len(),
            "Parsing metadata JSON"
        );
        let metadata: MessageMetadata = serde_json::from_str(&content)?;

        stats.total_messages += 1;
        debug!(
            message_id = %metadata.id,
            tracking_enabled = metadata.tracking_enabled,
            opened = metadata.opened,
            open_count = metadata.open_count,
            "Processing message metadata"
        );

        if metadata.tracking_enabled {
            stats.tracked_messages += 1;

            if metadata.opened {
                stats.opened_messages += 1;
                stats.total_opens += metadata.open_count;
                debug!(
                    message_id = %metadata.id,
                    open_count = metadata.open_count,
                    event_count = metadata.tracking_events.len(),
                    "Processing tracking events"
                );

                // Process tracking events
                for event in &metadata.tracking_events {
                    let ip_added = all_ips.insert(event.client_ip.clone());
                    if ip_added {
                        debug!(
                            message_id = %metadata.id,
                            client_ip = %event.client_ip,
                            "New unique IP added"
                        );
                    }

                    // Count user agents
                    let ua = if event.user_agent.len() > 100 {
                        format!("{}...", &event.user_agent[..97])
                    } else {
                        event.user_agent.clone()
                    };
                    let count = user_agent_counts.entry(ua.clone()).or_insert(0);
                    *count += 1;
                    debug!(
                        message_id = %metadata.id,
                        user_agent = %ua,
                        count = *count,
                        "Updated user agent count"
                    );

                    // Add to recent activity
                    recent_events.push(RecentActivity {
                        message_id: metadata.id.clone(),
                        timestamp: event.timestamp,
                        client_ip: event.client_ip.clone(),
                        user_agent: event.user_agent.clone(),
                    });
                }
                debug!(
                    message_id = %metadata.id,
                    events_processed = metadata.tracking_events.len(),
                    "Finished processing tracking events"
                );
            } else {
                debug!(
                    message_id = %metadata.id,
                    "Message tracked but not opened"
                );
            }
        } else {
            debug!(
                message_id = %metadata.id,
                "Message not tracked"
            );
        }

        Ok(())
    }
}

impl Default for StatsCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_stats_collector_new() {
        let collector = StatsCollector::new();
        assert!(collector.pixel_requests.is_empty());
    }

    #[test]
    fn test_record_pixel_request() {
        let mut collector = StatsCollector::new();
        collector.record_pixel_request("test-id", "192.168.1.1");
        
        assert_eq!(collector.pixel_requests.len(), 1);
        assert!(collector.pixel_requests.contains_key("test-id"));
    }

    #[tokio::test]
    async fn test_compute_stats_empty_dir() {
        let collector = StatsCollector::new();
        let temp_dir = TempDir::new().unwrap();
        
        let stats = collector.compute_stats(&temp_dir.path().to_path_buf()).await;
        
        assert!(stats.is_object());
        let stats_obj = stats.as_object().unwrap();
        assert_eq!(stats_obj.get("total_messages").unwrap().as_u64().unwrap(), 0);
    }
}
