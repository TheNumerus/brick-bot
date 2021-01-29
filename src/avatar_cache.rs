use std::collections::HashMap;

use bytes::Bytes;
use reqwest::Client;

use crate::{structs::User, BotError};

/// Caches avatar images in memory
pub struct AvatarCache {
    storage: HashMap<(String, String), Bytes>,
}

impl AvatarCache {
    /// Creates new avatar cache
    pub fn new() -> Self {
        Self { storage: HashMap::new() }
    }

    /// Returns image from cache, or downloads new
    pub async fn get(&mut self, client: &Client, user: &User) -> Result<&Bytes, BotError> {
        let key = (user.id.clone(), user.avatar.clone());

        Ok(self
            .storage
            .entry(key)
            .or_insert(client.get(&user.get_avatar_url()).send().await?.bytes().await?))
    }
}
