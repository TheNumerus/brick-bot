use bytes::Bytes;
use reqwest::{
    multipart::{Form, Part},
    Client,
};
use serde_json::json;

use crate::{
    config::Config,
    error::BotError,
    structs::{DiscordResult, Message},
};

/// Sends image to specified channel id
pub async fn send_image(client: &Client, channel_id: &str, image: &Bytes, config: &Config) -> Result<DiscordResult<Message>, BotError> {
    let url = format!("https://discord.com/api/channels/{}/messages", channel_id);

    let image_bytes = image.as_ref().to_owned();

    let file_part = Part::bytes(image_bytes).file_name(config.image_name.clone().unwrap());

    let form = Form::new().part("file", file_part);

    let token = format!("Bot {}", config.token);

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
pub async fn send_reply(client: &Client, channel_id: &str, reply_id: &str, reply: &str, config: &Config) -> Result<DiscordResult<Message>, BotError> {
    let url = format!("https://discord.com/api/channels/{}/messages", channel_id);

    let json = json!({
        "content": reply,
        "message_reference": {
            "message_id": reply_id
         }
    });

    let token = format!("Bot {}", config.token);

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
