use std::borrow::Cow;

use reqwest::{
    multipart::{Form, Part},
    Client,
};
use serde_json::json;

use crate::{
    error::BotError,
    structs::{DiscordResult, Message},
};

/// Builder for requests with new messages
pub struct NewMessageBuilder<'c> {
    client: &'c Client,
    channel: Cow<'c, str>,
    token: String,
    message: Option<String>,
    reply_id: Option<String>,
    file: Option<Vec<u8>>,
    file_name: Option<String>,
}

impl<'c> NewMessageBuilder<'c> {
    pub fn new(client: &'c Client, token: &str, channel_id: &'c str) -> Self {
        Self {
            client,
            token: format!("Bot {}", token),
            message: None,
            reply_id: None,
            file: None,
            file_name: None,
            channel: channel_id.into(),
        }
    }

    pub fn message<S: AsRef<str>>(mut self, message: S) -> Self {
        self.message = Some(message.as_ref().to_string());
        self
    }

    pub fn reply_to<S: AsRef<str>>(mut self, reply_id: S) -> Self {
        self.reply_id = Some(reply_id.as_ref().to_string());
        self
    }

    /// Will attach file to output message
    pub fn file_with_filename<S: AsRef<str>>(mut self, file: Vec<u8>, filename: S) -> Self {
        self.file = Some(file);
        self.file_name = Some(filename.as_ref().to_string());
        self
    }

    /// Sends message
    pub async fn send(self) -> Result<DiscordResult<Message>, BotError> {
        if self.message.is_none() && self.file.is_none() {
            return Err(BotError::InternalError(String::from("Cannot create message without any content")));
        }

        let url = format!("https://discord.com/api/channels/{}/messages", self.channel);

        let json = match &self.reply_id {
            Some(reply_id) => json!({
                "content": self.message.as_ref().unwrap_or(&String::new()),
                "message_reference": {
                    "message_id": reply_id
                 }
            }),
            None => json!({
                "content": self.message.as_ref().unwrap_or(&String::new())
            }),
        };

        let rb = match self.file {
            Some(file) => {
                // if file is `Some`, file_name must be too
                let file_part = Part::bytes(file).file_name(self.file_name.unwrap());
                let mut form = Form::new().part("file", file_part);
                if self.message.is_some() {
                    let json_part = Part::text(json.to_string());
                    form = form.part("payload_json", json_part);
                }
                self.client.post(&url).header("Authorization", &self.token).multipart(form)
            }
            None => self.client.post(&url).header("Authorization", &self.token).json(&json),
        };

        rb.send().await?.json::<DiscordResult<Message>>().await.map_err(|e| e.into())
    }
}

pub async fn send_interaction_response(
    client: &Client,
    interaction_id: &str,
    interaction_token: &str,
    token: &str,
    reply: Option<&str>,
) -> Result<String, BotError> {
    let url = format!("https://discord.com/api/v8/interactions/{}/{}/callback", interaction_id, interaction_token);

    let json = match reply {
        Some(message) => {
            json!({
                "type": 3,
                "data": {"content": message}
            })
        }
        None => {
            json!({
                "type": 5
            })
        }
    };

    let token = format!("Bot {}", token);

    client
        .post(&url)
        .header("Authorization", token)
        .json(&json)
        .send()
        .await?
        .text()
        .await
        .map_err(|e| e.into())
}

pub async fn get_message_from_channel(client: &Client, token: &str, channel_id: &str, message_id: &str) -> Result<DiscordResult<Message>, BotError> {
    let url = format!("https://discord.com/api/channels/{}/messages/{}", channel_id, message_id);
    let token = format!("Bot {}", token);

    client
        .get(&url)
        .header("Authorization", token)
        .send()
        .await?
        .json::<DiscordResult<Message>>()
        .await
        .map_err(|e| e.into())
}

pub async fn register_commands(client: &Client, token: &str, command: &serde_json::Value, app_id: &str) -> Result<String, BotError> {
    let token = format!("Bot {}", token);

    let url = format!("https://discord.com/api/applications/{}/commands", app_id);

    client
        .post(&url)
        .header("Authorization", token)
        .json(&command)
        .send()
        .await?
        .text()
        .await
        .map_err(|e| e.into())
}
