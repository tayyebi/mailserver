use serde::Deserialize;
use std::collections::HashMap;
use tokio::fs;
use anyhow::Result;

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    pub whitelist: Vec<String>,
    pub services: HashMap<u16, String>,
}

pub async fn load_config(path: &str) -> Result<Config> {
    let content = match fs::read_to_string(path).await {
        Ok(c) => c,
        Err(_) => {
            // Default config if file missing
            // Fallback defaults as requested
            r#"{
                "whitelist": ["127.0.0.1", "::1"],
                "services": {
                    "2222": "ssh",
                    "8080": "http",
                    "2525": "smtp"
                }
            }"#.to_string()
        }
    };
    let config: Config = serde_json::from_str(&content)?;
    Ok(config)
}
