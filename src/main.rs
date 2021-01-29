use std::{collections::HashMap, time::Duration};

use anyhow::{bail, Context, Result};
use bytes::Bytes;
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

pub static TOKEN_HEADER: OnceCell<String> = OnceCell::new();

#[tokio::main]
async fn main() -> Result<()> {
    set_token()?;

    let client = reqwest::Client::new();
    let mut last_message_ids = HashMap::new();
    let mut avatars: HashMap<(String, String), Bytes> = HashMap::new();

    let my_id = get_json::<User>(&client, "https://discord.com/api/users/@me").await?.id;

    let guilds: Vec<GuildInfo> = get_json(&client, "https://discord.com/api/users/@me/guilds").await?;

    if guilds.is_empty() {
        bail!("Bot is not in any channel");
    }

    let channels_path = format!("https://discord.com/api/guilds/{}/channels", guilds[0].id);
    let mut channels: Vec<Channel> = get_json(&client, &channels_path).await?;
    channels.retain(|c| c.channel_type == ChannelType::GuildText);

    //init last message ids
    for channel in &channels {
        last_message_ids.insert(channel.id.clone(), channel.last_message_id.clone());
    }

    loop {
        for channel in &channels {
            let message_path = match &last_message_ids[&channel.id] {
                Some(timestamp) => format!("https://discord.com/api/channels/{}/messages?after={}", channel.id, timestamp),
                None => format!("https://discord.com/api/channels/{}/messages", channel.id),
            };

            let mut messages: Vec<Message> = get_json(&client, &message_path).await?;

            if messages.is_empty() {
                continue;
            }

            // now update timestamps
            last_message_ids.insert(channel.id.clone(), Some(messages.last().unwrap().id.clone()));

            messages.retain(|m| !m.mentions.is_empty());

            for message in messages {
                if !message.content.starts_with("!brick") {
                    continue;
                }
                for user in message.mentions {
                    if user.id == my_id {
                        let res = send_reply(&client, &channel.id, &message.id).await?;
                        last_message_ids.insert(channel.id.clone(), Some(res.id.clone()));
                        continue;
                    }

                    if !avatars.contains_key(&(user.id.clone(), user.avatar.clone())) {
                        println!("{} -  {}", user.username, user.get_avatar_url());

                        let image = client.get(&user.get_avatar_url()).send().await?.bytes().await?;

                        avatars.insert((user.id.clone(), user.avatar.clone()), image);
                    }

                    println!("sending avatar... - {}, {}", channel.name, user.username);

                    //send avatar for now
                    let image_res = send_image(&client, &channel.id, avatars.get(&(user.id, user.avatar)).unwrap()).await?;

                    last_message_ids.insert(channel.id.clone(), Some(image_res.id.clone()));
                }
            }
            sleep(Duration::from_secs(2)).await;
        }
    }
}

async fn get_json<T: DeserializeOwned>(client: &Client, path: &str) -> Result<T, BotError> {
    client
        .get(path)
        .header("Authorization", TOKEN_HEADER.get().ok_or(BotError::InternalError)?)
        .send()
        .await?
        .json::<T>()
        .await
        .map_err(|e| e.into())
}

async fn send_image(client: &Client, channel_id: &str, image: &Bytes) -> Result<Message, BotError> {
    let url = format!("https://discord.com/api/channels/{}/messages", channel_id);

    let image_bytes = image.as_ref().to_owned();

    let file_part = Part::bytes(image_bytes).file_name("brick_that_fucker.png");

    let form = Form::new().part("file", file_part);

    client
        .post(&url)
        .header("Authorization", TOKEN_HEADER.get().ok_or(BotError::InternalError)?)
        .multipart(form)
        .send()
        .await?
        .json::<Message>()
        .await
        .map_err(|e| e.into())
}

async fn send_reply(client: &Client, channel_id: &str, reply_id: &str) -> Result<Message, BotError> {
    let url = format!("https://discord.com/api/channels/{}/messages", channel_id);

    let json = json!({
        "content": "good try fucker",
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
enum BotError {
    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error),
    #[error(transparent)]
    SerdeError(#[from] serde_json::Error),
    #[error("Internal Error")]
    InternalError,
}

fn set_token() -> Result<()> {
    let token = std::env::var("BRICKBOTTOKEN").context("Cannot find BRICKBOTTOKEN in environment")?;
    TOKEN_HEADER.set(format!("Bot {}", token)).unwrap();
    Ok(())
}
