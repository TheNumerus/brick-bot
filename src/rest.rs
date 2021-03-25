use bytes::Bytes;
use reqwest::{
    multipart::{Form, Part},
    Client,
};
use serde_json::json;

use crate::{
    error::BotError,
    structs::{DiscordResult, Message},
};

/// Sends image to specified channel id
pub async fn send_image(client: &Client, channel_id: &str, image: &Bytes, image_name: String, token: &str) -> Result<DiscordResult<Message>, BotError> {
    let url = format!("https://discord.com/api/channels/{}/messages", channel_id);

    let image_bytes = image.as_ref().to_owned();

    let file_part = Part::bytes(image_bytes).file_name(image_name);

    let form = Form::new().part("file", file_part);

    let token = format!("Bot {}", token);

    client
        .post(&url)
        .header("Authorization", token)
        .multipart(form)
        .send()
        .await?
        .json::<DiscordResult<Message>>()
        .await
        .map_err(|e| e.into())
}

/// Sends image to specified channel id
pub async fn send_reply(client: &Client, channel_id: &str, reply_id: &str, reply: &str, token: &str) -> Result<DiscordResult<Message>, BotError> {
    let url = format!("https://discord.com/api/channels/{}/messages", channel_id);

    let json = json!({
        "content": reply,
        "message_reference": {
            "message_id": reply_id
         }
    });

    let token = format!("Bot {}", token);

    client
        .post(&url)
        .header("Authorization", token)
        .json(&json)
        .send()
        .await?
        .json::<DiscordResult<Message>>()
        .await
        .map_err(|e| e.into())
}

pub async fn send_message_to_channel(client: &Client, channel_id: &str, message: &str, token: &str) -> Result<DiscordResult<Message>, BotError> {
    let url = format!("https://discord.com/api/channels/{}/messages", channel_id);

    let json = json!({ "content": message });

    let token = format!("Bot {}", token);

    client
        .post(&url)
        .header("Authorization", token)
        .json(&json)
        .send()
        .await?
        .json::<DiscordResult<Message>>()
        .await
        .map_err(|e| e.into())
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
