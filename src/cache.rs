use std::collections::HashMap;

use bytes::Bytes;
use reqwest::Client;

use crate::{error::BotError, structs::User};

/// Caches images in memory based of user avatars
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

        if let None = self.storage.get(&key) {
            // get avatar only if none in cache
            let avatar = client.get(&user.get_avatar_url()).send().await?.bytes().await?;
            self.storage.insert(key.clone(), avatar);
        }

        // can unwrap now
        Ok(self.storage.get(&key).unwrap())
    }
}
