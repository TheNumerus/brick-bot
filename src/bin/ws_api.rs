use std::{
    fmt::Display,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use bytes::Bytes;
use chrono::Utc;
use once_cell::sync::OnceCell;
use reqwest::{
    multipart::{Form, Part},
    Client,
};
use serde_repr::*;

use futures_channel::mpsc::TrySendError;
use serde::{de::DeserializeOwned, Deserialize};

use anyhow::{Context, Result};

use futures_util::StreamExt;
use serde_json::json;
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};

use brick_bot::{
    avatar_cache::AvatarCache,
    config::Config,
    error::BotError,
    image_edit::brickify_gif,
    structs::{DiscordResult, Message as DiscordMessage, User},
};

pub static TOKEN_HEADER: OnceCell<String> = OnceCell::new();
pub static BRICK_GIF: OnceCell<Vec<u8>> = OnceCell::new();

// TODO handle unwraps

#[tokio::main]
async fn main() -> Result<()> {
    let config = tokio::fs::read_to_string("bot.toml").await.context("Error loading bot configuration.")?;
    let config: Config = toml::from_str(&config).context("Could not parse settings")?;
    // It's safe to unwrap string options now
    let config = config.set_missing();

    let client = reqwest::Client::new();

    set_token(&config.token)?;

    let identified = Arc::from(AtomicBool::from(false));

    BRICK_GIF
        .set(tokio::fs::read(&config.image_path).await.context("cannot find image on given path")?)
        .unwrap();

    let me: DiscordResult<User> = get_json(&client, "https://discord.com/api/users/@me").await?;
    let my_id = Result::from(me)?.id;

    let mut cache = AvatarCache::new();

    let (tx, rx) = futures_channel::mpsc::unbounded();

    let (ws_stream, _ws_res) = connect_async("wss://gateway.discord.gg/?v=8&encoding=json").await.expect("Failed to connect");
    println!("WebSocket handshake has been successfully completed");

    let (write, mut read) = ws_stream.split();

    let sender = rx.map(Ok).forward(write);
    tokio::pin!(sender);

    let config_clone = config.clone();
    let ws_to_stdout = tokio::spawn(async move {
        loop {
            let tx = tx.clone();
            match read.next().await {
                Some(message) => {
                    println!("{:#?}", message);
                    let msg = message.unwrap().to_string();
                    let payload: Payload = serde_json::from_str(&msg).unwrap();
                    println!("{:#?}", payload);
                    if payload.op == Opcode::Hello {
                        let heartbeat = payload.d.unwrap()["heartbeat_interval"].as_u64().unwrap();
                        // TODO not handling reconnections for now
                        let heartbeater = tokio::spawn(async move {
                            loop {
                                let hb_message = json! ({
                                    "op": Opcode::Heartbeat,
                                    "d": 0
                                });
                                tx.unbounded_send(Message::text(hb_message.to_string()))?;
                                println!("sent heartbeat");
                                sleep(Duration::from_millis(heartbeat)).await;
                            }
                            // TODO warning, but error when line is deleted
                            Ok::<(), TrySendError<Message>>(())
                        });
                    } else if payload.op == Opcode::HeartbeatAck {
                        if !identified.load(Ordering::SeqCst) {
                            // TODO add activity to config
                            let identify = json!({
                                "op": Opcode::Identify,
                                "d": {
                                    "token": &config_clone.token,
                                    "intents": 512,
                                    "properties": {
                                        "$os": "windows",
                                        "$browser": "brick-bot",
                                        "$device": "brick-bot"
                                    },
                                    "presence": {
                                        "activities": [{
                                            "name": "Brickity brick 🧱",
                                            "type": 0
                                        }],
                                        "status": "online",
                                        "since": 0,
                                        "afk": false
                                    },
                                }
                            })
                            .to_string();
                            tx.unbounded_send(Message::Text(identify.to_string())).unwrap();
                            identified.store(true, Ordering::SeqCst);
                        }
                    } else if payload.op == Opcode::Dispatch {
                        if let Some(event) = payload.t {
                            if event == "MESSAGE_CREATE" {
                                let message: DiscordMessage = serde_json::from_value(payload.d.unwrap()).unwrap();

                                if !message.content.starts_with(&config.command) {
                                    continue;
                                }

                                // send error messeage
                                if message.mentions.is_empty() {
                                    match message.mention_roles {
                                        Some(roles) if !roles.is_empty() => {
                                            let res = send_reply(&client, &message.channel_id, &message.id, &config.err_msg_tag_role.clone().unwrap())
                                                .await
                                                .unwrap();
                                            log_message("Error - tagged role");
                                        }
                                        _ => {
                                            let res = send_reply(&client, &message.channel_id, &message.id, &config.err_msg_tag_nobody.clone().unwrap())
                                                .await
                                                .unwrap();
                                            log_message("Error - tagged nobody");
                                        }
                                    }
                                }

                                for user in message.mentions {
                                    if user.id == my_id {
                                        if let Some(ref self_message) = config.self_brick_message {
                                            let res = send_reply(&client, &message.channel_id, &message.id, self_message).await.unwrap();
                                            log_message(format!("Bricked self"));
                                            continue;
                                        }
                                    }

                                    let avatar = cache.get(&client, &user).await.unwrap();

                                    let image = brickify_gif(BRICK_GIF.get().unwrap(), avatar, &config).await.unwrap();

                                    //send avatar for now
                                    let image_res: DiscordResult<DiscordMessage> = send_image(&client, &message.channel_id, &image, &config).await.unwrap();
                                    let image_res = Result::from(image_res).unwrap();

                                    log_message(format!("Bricked user \"{}\"", user.username));
                                }
                            }
                        }
                    }
                }
                None => {
                    break;
                }
            }
        }
    });

    // TODO this feels wrong
    tokio::join! {sender, ws_to_stdout};
    Ok(())
}

// TODO move
#[derive(Debug, Deserialize)]
struct Payload<T = serde_json::Value> {
    op: Opcode,
    d: Option<T>,
    s: Option<usize>,
    t: Option<String>,
}

// TODO move
/// All gateway events in Discord are tagged with an opcode that denotes the payload type.
/// Your connection to our gateway may also sometimes close.
/// When it does, you will receive a close code that tells you what happened.
#[derive(Debug, Deserialize_repr, Clone, PartialEq, Serialize_repr)]
#[repr(u16)]
enum Opcode {
    /// An event was dispatched.
    Dispatch = 0,
    /// Fired periodically by the client to keep the connection alive.
    Heartbeat = 1,
    /// Starts a new session during the initial handshake.
    Identify = 2,
    /// Update the client's presence.
    PresenceUpdate = 3,
    /// Used to join/leave or move between voice channels.
    VoiceStateUpdate = 4,
    /// Resume a previous session that was disconnected.
    Resume = 6,
    /// You should attempt to reconnect and resume immediately.
    Reconnect = 7,
    /// Request information about offline guild members in a large guild.
    RequestGuildMembers = 8,
    /// The session has been invalidated. You should reconnect and identify/resume accordingly.
    InvalidSession = 9,
    /// Sent immediately after connecting, contains the heartbeat_interval to use.
    Hello = 10,
    /// Sent in response to receiving a heartbeat to acknowledge that it has been received.
    HeartbeatAck = 11,
}
// TODO move these too

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

async fn send_image(client: &Client, channel_id: &str, image: &Bytes, config: &Config) -> Result<DiscordResult<DiscordMessage>, BotError> {
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
        .json::<DiscordResult<DiscordMessage>>()
        .await
        .map_err(|e| e.into())
}

async fn send_reply(client: &Client, channel_id: &str, reply_id: &str, reply: &str) -> Result<DiscordMessage, BotError> {
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
        .json::<DiscordMessage>()
        .await
        .map_err(|e| e.into())
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
