use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuildConfig {
    pub starboard_channel: Option<u64>,
    pub min_stars: u64,
    #[serde(default = "default_emoji")]
    pub starboard_emoji: String,
    pub starred_messages: HashSet<u64>,
    #[serde(default)]
    pub starboard_msg_ids: HashMap<u64, u64>,
}

fn default_emoji() -> String {
    "⭐".to_string()
}

impl Default for GuildConfig {
    fn default() -> Self {
        Self {
            starboard_channel: None,
            min_stars: 3,
            starboard_emoji: default_emoji(),
            starred_messages: HashSet::new(),
            starboard_msg_ids: HashMap::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Store {
    pub guilds: HashMap<String, GuildConfig>,
    #[serde(skip)]
    path: String,
}

impl Store {
    pub async fn load(path: &str) -> Self {
        match tokio::fs::read_to_string(path).await {
            Ok(content) => match serde_json::from_str::<Store>(&content) {
                Ok(mut store) => {
                    store.path = path.to_string();
                    store
                }
                Err(e) => {
                    tracing::warn!("Failed to parse {path}: {e}. Starting fresh.");
                    Self::empty(path)
                }
            },
            Err(_) => Self::empty(path),
        }
    }

    fn empty(path: &str) -> Self {
        Self {
            guilds: HashMap::new(),
            path: path.to_string(),
        }
    }

    pub async fn save(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let content = serde_json::to_string_pretty(self)?;
        tokio::fs::write(&self.path, content).await?;
        Ok(())
    }

    pub fn guild(&self, guild_id: u64) -> GuildConfig {
        self.guilds
            .get(&guild_id.to_string())
            .cloned()
            .unwrap_or_default()
    }

    pub fn guild_mut(&mut self, guild_id: u64) -> &mut GuildConfig {
        self.guilds
            .entry(guild_id.to_string())
            .or_default()
    }

    pub fn is_starred(&self, guild_id: u64, message_id: u64) -> bool {
        self.guilds
            .get(&guild_id.to_string())
            .map_or(false, |c| c.starred_messages.contains(&message_id))
    }

    pub fn mark_starred(&mut self, guild_id: u64, message_id: u64) {
        self.guild_mut(guild_id).starred_messages.insert(message_id);
    }

    pub fn set_starboard_msg(&mut self, guild_id: u64, message_id: u64, starboard_msg_id: u64) {
        self.guild_mut(guild_id)
            .starboard_msg_ids
            .insert(message_id, starboard_msg_id);
    }

    pub fn get_starboard_msg(&self, guild_id: u64, message_id: u64) -> Option<u64> {
        self.guilds
            .get(&guild_id.to_string())
            .and_then(|c| c.starboard_msg_ids.get(&message_id).copied())
    }
}
