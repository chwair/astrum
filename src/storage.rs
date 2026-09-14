use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuildConfig {
    pub starboard_channel: Option<u64>,
    pub min_stars: u64,
    #[serde(default)]
    pub starboard_emojis: Vec<String>,
    // single-emoji field from older configs, folded into starboard_emojis on load
    #[serde(default, skip_serializing)]
    starboard_emoji: Option<String>,
    pub starred_messages: HashSet<u64>,
    #[serde(default)]
    pub starboard_msg_ids: HashMap<u64, u64>,
}

fn default_emojis() -> Vec<String> {
    vec!["⭐".to_string()]
}

impl Default for GuildConfig {
    fn default() -> Self {
        Self {
            starboard_channel: None,
            min_stars: 3,
            starboard_emojis: default_emojis(),
            starboard_emoji: None,
            starred_messages: HashSet::new(),
            starboard_msg_ids: HashMap::new(),
        }
    }
}

impl GuildConfig {
    // fills the emoji list from the legacy field, or the default if there's nothing to migrate
    fn migrate_emojis(&mut self) {
        if !self.starboard_emojis.is_empty() {
            return;
        }
        self.starboard_emojis = match self.starboard_emoji.take() {
            Some(legacy) => vec![crate::emoji::normalize(&legacy).unwrap_or(legacy)],
            None => default_emojis(),
        };
    }
}

// a user's avatar, uploaded once to the app's emoji list and reused until they change it
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvatarEmoji {
    pub hash: String,
    pub emoji_id: u64,
    #[serde(default)]
    pub used_at: u64,
}

impl AvatarEmoji {
    // the inline form discord renders in message text
    fn mention(&self, user_id: u64) -> String {
        format!("<:av{user_id}:{}>", self.emoji_id)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Store {
    pub guilds: HashMap<String, GuildConfig>,
    #[serde(default)]
    pub avatars: HashMap<String, AvatarEmoji>,
    #[serde(skip)]
    path: String,
}

impl Store {
    pub async fn load(path: &str) -> Self {
        match tokio::fs::read_to_string(path).await {
            Ok(content) => match serde_json::from_str::<Store>(&content) {
                Ok(mut store) => {
                    store.path = path.to_string();
                    for cfg in store.guilds.values_mut() {
                        cfg.migrate_emojis();
                    }
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
            avatars: HashMap::new(),
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

    // the cached avatar emoji for a user, marked as just used so it survives eviction
    pub fn avatar(&mut self, user_id: u64, hash: &str) -> Option<String> {
        let entry = self.avatars.get_mut(&user_id.to_string())?;
        if entry.hash != hash {
            return None;
        }
        entry.used_at = now();
        Some(entry.mention(user_id))
    }

    // forgets the user's outdated avatar and, if the app is full, the least recently used one,
    // returning the emojis that should be deleted
    pub fn take_stale_avatars(&mut self, user_id: u64, max: usize) -> Vec<u64> {
        let mut stale = Vec::new();
        if let Some(entry) = self.avatars.remove(&user_id.to_string()) {
            stale.push(entry.emoji_id);
        }
        while self.avatars.len() >= max {
            let oldest = self
                .avatars
                .iter()
                .min_by_key(|(_, entry)| entry.used_at)
                .map(|(id, _)| id.clone());
            match oldest {
                Some(id) => {
                    if let Some(entry) = self.avatars.remove(&id) {
                        stale.push(entry.emoji_id);
                    }
                }
                None => break,
            }
        }
        stale
    }

    pub fn set_avatar(&mut self, user_id: u64, hash: String, emoji_id: u64) -> String {
        let entry = AvatarEmoji {
            hash,
            emoji_id,
            used_at: now(),
        };
        let mention = entry.mention(user_id);
        self.avatars.insert(user_id.to_string(), entry);
        mention
    }
}

// seconds since the epoch, used to order avatars by how recently they were needed
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
