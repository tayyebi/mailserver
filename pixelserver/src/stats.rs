use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tracing::{debug, error};

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
        let request = PixelRequest {
            timestamp: Utc::now(),
            client_ip: client_ip.to_string(),
        };

        self.pixel_requests
            .entry(message_id.to_string())
            .or_insert_with(Vec::new)
            .push(request);

        // Keep only recent requests (last 1000 per message)
        if let Some(requests) = self.pixel_requests.get_mut(message_id) {
            if requests.len() > 1000 {
                requests.drain(0..requests.len() - 1000);
            }
        }

        // Limit total messages tracked in memory
        if self.pixel_requests.len() > 10000 {
            // Remove oldest entries
            let mut keys: Vec<_> = self.pixel_requests.keys().cloned().collect();
            keys.sort();
            for key in keys.iter().take(self.pixel_requests.len() - 9000) {
                self.pixel_requests.remove(key);
            }
        }
    }

    pub async fn compute_stats(&self, data_dir: &PathBuf) -> Value {
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

        // Scan data directory for message metadata
        if let Ok(entries) = fs::read_dir(data_dir) {
            for entry in entries.flatten() {
                if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                    let message_dir = entry.path();
                    let meta_file = message_dir.join("meta.json");

                    if meta_file.exists() {
                        match self.process_message_metadata(&meta_file, &mut stats, &mut all_ips, &mut user_agent_counts, &mut recent_events) {
                            Ok(_) => {}
                            Err(e) => {
                                error!(file = ?meta_file, error = %e, "Failed to process metadata file");
                            }
                        }
                    }
                }
            }
        }

        stats.unique_ips = all_ips.len() as u32;

        // Sort and limit recent activity
        recent_events.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        stats.recent_activity = recent_events.into_iter().take(50).collect();

        // Sort user agents by count
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
            total_messages = stats.total_messages,
            tracked_messages = stats.tracked_messages,
            opened_messages = stats.opened_messages,
            "Computed statistics"
        );

        serde_json::to_value(stats).unwrap_or_else(|_| serde_json::json!({}))
    }

    fn process_message_metadata(
        &self,
        meta_file: &PathBuf,
        stats: &mut SystemStats,
        all_ips: &mut std::collections::HashSet<String>,
        user_agent_counts: &mut HashMap<String, u32>,
        recent_events: &mut Vec<RecentActivity>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let content = fs::read_to_string(meta_file)?;
        let metadata: MessageMetadata = serde_json::from_str(&content)?;

        stats.total_messages += 1;

        if metadata.tracking_enabled {
            stats.tracked_messages += 1;

            if metadata.opened {
                stats.opened_messages += 1;
                stats.total_opens += metadata.open_count;

                // Process tracking events
                for event in &metadata.tracking_events {
                    all_ips.insert(event.client_ip.clone());

                    // Count user agents
                    let ua = if event.user_agent.len() > 100 {
                        format!("{}...", &event.user_agent[..97])
                    } else {
                        event.user_agent.clone()
                    };
                    *user_agent_counts.entry(ua).or_insert(0) += 1;

                    // Add to recent activity
                    recent_events.push(RecentActivity {
                        message_id: metadata.id.clone(),
                        timestamp: event.timestamp,
                        client_ip: event.client_ip.clone(),
                        user_agent: event.user_agent.clone(),
                    });
                }
            }
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
