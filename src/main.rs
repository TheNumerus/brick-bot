use std::{collections::HashMap, fmt::Display, time::Duration};

use anyhow::{bail, Context, Result};
use bytes::Bytes;
use chrono::Utc;
use once_cell::sync::OnceCell;
use reqwest::{
    multipart::{Form, Part},
    Client,
};
use serde::de::DeserializeOwned;
use serde_json::json;
use thiserror::Error;
use tokio::time::sleep;

mod structs;
use structs::*;

mod avatar_cache;
use avatar_cache::AvatarCache;

mod config;
use config::Config;

mod image_edit;

pub static TOKEN_HEADER: OnceCell<String> = OnceCell::new();
pub static BRICK_GIF: OnceCell<Vec<u8>> = OnceCell::new();

#[tokio::main]
async fn main() -> Result<()> {
    let config = tokio::fs::read_to_string("bot.toml").await.context("Error loading bot configuration.")?;
    let config: Config = toml::from_str(&config).context("Could not parse settings")?;
    // It's safe to unwrap string options now
    let config = config.set_missing();

    set_token(&config.token)?;

    let client = reqwest::Client::new();
    let mut last_message_ids = HashMap::new();
    let mut cache = AvatarCache::new();

    let gateway: DiscordResult<serde_json::Value> = get_json(&client, "https://discord.com/api/gateway").await?;
    println!("{:#?}", gateway);

    BRICK_GIF
        .set(tokio::fs::read(&config.image_path).await.context("cannot find image on given path")?)
        .unwrap();

    let me: DiscordResult<User> = get_json(&client, "https://discord.com/api/users/@me").await?;
    let my_id = Result::from(me)?.id;

    log_message("Got self info");

    let guilds: DiscordResult<Vec<GuildInfo>> = get_json(&client, "https://discord.com/api/users/@me/guilds").await?;
    let guilds = Result::from(guilds)?;

    log_message("Got guild info");

    if guilds.is_empty() {
        bail!("Bot is not in any guild");
    }

    let channels_path = format!("https://discord.com/api/guilds/{}/channels", guilds[0].id);
    let channels: DiscordResult<Vec<Channel>> = get_json(&client, &channels_path).await?;

    log_message("Got channel info");

    let mut channels = Result::from(channels)?;

    channels.retain(|c| c.channel_type == ChannelType::GuildText);

    //init last message ids
    for channel in &channels {
        last_message_ids.insert(channel.id.clone(), channel.last_message_id.clone());
    }

    loop {
        // TODO Support reloading of channels
        for channel in &channels {
            let message_path = match &last_message_ids[&channel.id] {
                Some(timestamp) => format!("https://discord.com/api/channels/{}/messages?after={}", channel.id, timestamp),
                None => format!("https://discord.com/api/channels/{}/messages", channel.id),
            };

            let messages: DiscordResult<Vec<Message>> = get_json(&client, &message_path).await?;
            let messages = Result::from(messages)?;

            if messages.is_empty() {
                continue;
            }

            // now update timestamps
            last_message_ids.insert(channel.id.clone(), Some(messages.last().unwrap().id.clone()));

            for message in messages {
                if !message.content.starts_with(&config.command) {
                    continue;
                }

                // send error messeage
                if message.mentions.is_empty() {
                    match message.mention_roles {
                        Some(roles) if !roles.is_empty() => {
                            let res = send_reply(&client, &channel.id, &message.id, &config.err_msg_tag_role.clone().unwrap()).await?;
                            log_message("Error - tagged role");
                            last_message_ids.insert(channel.id.clone(), Some(res.id.clone()));
                        }
                        _ => {
                            let res = send_reply(&client, &channel.id, &message.id, &config.err_msg_tag_nobody.clone().unwrap()).await?;
                            log_message("Error - tagged nobody");
                            last_message_ids.insert(channel.id.clone(), Some(res.id.clone()));
                        }
                    }
                }

                for user in message.mentions {
                    if user.id == my_id {
                        if let Some(ref self_message) = config.self_brick_message {
                            let res = send_reply(&client, &channel.id, &message.id, self_message).await?;
                            last_message_ids.insert(channel.id.clone(), Some(res.id.clone()));
                            log_message(format!("Bricked self"));
                            continue;
                        }
                    }

                    let avatar = cache.get(&client, &user).await?;

                    let image = image_edit::brickify_gif(avatar, &config).await?;

                    //send avatar for now
                    let image_res: DiscordResult<Message> = send_image(&client, &channel.id, &image, &config).await?;
                    let image_res = Result::from(image_res)?;

                    log_message(format!("Bricked user \"{}\"", user.username));

                    last_message_ids.insert(channel.id.clone(), Some(image_res.id.clone()));
                }
            }
            sleep(Duration::from_secs(2)).await;
        }
    }
}

async fn get_json<T: DeserializeOwned>(client: &Client, path: &str) -> Result<DiscordResult<T>, BotError> {
    client
        .get(path)
        .header("Authorization", TOKEN_HEADER.get().ok_or(BotError::InternalError)?)
        .send()
        .await?
        .json::<DiscordResult<T>>()
        .await
        .map_err(|e| e.into())
}

/// Mainly used for development
#[allow(dead_code)]
async fn get_text(client: &Client, path: &str) -> Result<String, BotError> {
    client
        .get(path)
        .header("Authorization", TOKEN_HEADER.get().ok_or(BotError::InternalError)?)
        .send()
        .await?
        .text()
        .await
        .map_err(|e| e.into())
}

async fn send_image(client: &Client, channel_id: &str, image: &Bytes, config: &Config) -> Result<DiscordResult<Message>, BotError> {
    let url = format!("https://discord.com/api/channels/{}/messages", channel_id);

    let image_bytes = image.as_ref().to_owned();

    let file_part = Part::bytes(image_bytes).file_name(config.image_name.clone().unwrap());

    let form = Form::new().part("file", file_part);

    client
        .post(&url)
        .header("Authorization", TOKEN_HEADER.get().ok_or(BotError::InternalError)?)
        .multipart(form)
        .send()
        .await?
        .json::<DiscordResult<Message>>()
        .await
        .map_err(|e| e.into())
}

async fn send_reply(client: &Client, channel_id: &str, reply_id: &str, reply: &str) -> Result<Message, BotError> {
    let url = format!("https://discord.com/api/channels/{}/messages", channel_id);

    let json = json!({
        "content": reply,
        "message_reference": {
            "message_id": reply_id
         }
    });

    client
        .post(&url)
        .header("Authorization", TOKEN_HEADER.get().ok_or(BotError::InternalError)?)
        .json(&json)
        .send()
        .await?
        .json::<Message>()
        .await
        .map_err(|e| e.into())
}

#[derive(Error, Debug)]
pub enum BotError {
    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),
    #[error(transparent)]
    SerdeError(#[from] serde_json::Error),
    #[error(transparent)]
    ImageError(#[from] image::error::ImageError),
    #[error("Internal Error")]
    InternalError,
    #[error("Api Error: `{0}`")]
    ApiError(String),
}

fn set_token(token: &str) -> Result<()> {
    TOKEN_HEADER.set(format!("Bot {}", token)).unwrap();
    Ok(())
}

fn log_message<T: AsRef<str> + Display>(message: T) {
    let time = Utc::now();

    let formated_time = time.format("%F %T");

    println!("[{}] - {}", formated_time, message);
}
