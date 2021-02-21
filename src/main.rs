use std::{
    fmt::Display,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
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

use serde::de::DeserializeOwned;

use anyhow::{Context, Result};

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::{
    sync::{mpsc::error::SendError, Mutex},
    task::JoinHandle,
};
use tokio_tungstenite::tungstenite::protocol::Message;

use brick_bot::{
    avatar_cache::AvatarCache,
    config::Config,
    error::BotError,
    image_edit::brickify_gif,
    structs::{DiscordResult, Message as DiscordMessage, Opcode, Payload, User},
};

pub static TOKEN_HEADER: OnceCell<String> = OnceCell::new();
pub static BRICK_GIF: OnceCell<Vec<u8>> = OnceCell::new();

// TODO handle unwraps

#[tokio::main]
async fn main() -> Result<()> {
    let config = prepare_config().await?;

    let client = reqwest::Client::new();

    set_token(&config.token)?;

    BRICK_GIF
        .set(tokio::fs::read(&config.image_path).await.context("cannot find image on given path")?)
        .unwrap();

    let me: DiscordResult<User> = get_json(&client, "https://discord.com/api/users/@me").await?;
    let my_id = Result::from(me)?.id;

    let cache = Arc::new(Mutex::new(AvatarCache::new()));

    // will enter enother iteration only when discord needs another connection
    loop {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let (ws_stream, _ws_res) = tokio_tungstenite::connect_async("wss://gateway.discord.gg/?v=8&encoding=json")
            .await
            .context("Failed to connect")?;
        log_message("WebSocket connected");

        let (mut write, mut read) = ws_stream.split();

        // this task will send all messages from channel to websocket stream
        let sender = tokio::spawn(async move {
            while let Some(m) = rx.recv().await {
                write.send(m).await?;
            }
            Ok::<(), tokio_tungstenite::tungstenite::error::Error>(())
        });

        // used for sending identify event on first heartbeat ack
        let identified = Arc::new(AtomicBool::from(false));
        // stores last event number
        let sequence_number = Arc::new(AtomicUsize::from(0));

        // maybe something could be done to infer this type
        let heartbeater: Arc<Mutex<Option<JoinHandle<Result<(), SendError<Message>>>>>> = Arc::new(Mutex::new(None));
        let session_id = Arc::new(Mutex::new(String::from("")));

        while let Some(message) = read.next().await {
            let tx = tx.clone();
            let config = Arc::clone(&config);
            let cache = Arc::clone(&cache);
            let my_id = my_id.clone();
            let client = client.clone();
            let identified = Arc::clone(&identified);
            let session_id = Arc::clone(&session_id);
            let heartbeater = Arc::clone(&heartbeater);
            let sequence_number = Arc::clone(&sequence_number);

            tokio::spawn(async move {
                // return if close connection
                if let Ok(msg) = &message {
                    if let Message::Close(_) = msg {
                        return Ok(());
                    }
                }

                let msg = message.unwrap().to_string();
                let payload: Payload = serde_json::from_str(&msg).unwrap();

                match &payload.op {
                    Opcode::Dispatch => {
                        // handle all events here

                        // update seq number
                        let s = payload.s.to_owned().unwrap();
                        sequence_number.store(s, Ordering::SeqCst);

                        if let Some(event) = payload.t {
                            if event == "MESSAGE_CREATE" {
                                let message: DiscordMessage = serde_json::from_value(payload.d.unwrap()).unwrap();

                                // clone everything needed
                                let config = Arc::clone(&config);
                                let client = client.clone();
                                let cache = Arc::clone(&cache);
                                let my_id = my_id.clone();

                                tokio::spawn(async move {
                                    on_message_create(message, config, client, cache, &my_id).await?;
                                    Ok::<(), BotError>(())
                                });
                            } else if event == "READY" {
                                // update session id for resuming later
                                *session_id.lock().await = payload.d.clone().unwrap()["session_id"].as_str().unwrap().to_owned();
                            }
                        }
                    }
                    Opcode::Heartbeat => {
                        // send new heartbeat on request
                        let hb_message = json! ({
                            "op": Opcode::Heartbeat,
                            "d": sequence_number.load(Ordering::SeqCst)
                        });
                        tx.send(Message::text(hb_message.to_string())).unwrap();
                    }
                    Opcode::Reconnect => {
                        // try resuming connection
                        let resume = json!({
                            "op": Opcode::Resume,
                            "d": {
                                "token": &config.token,
                                "session_id": &*session_id.lock().await,
                                "seq": sequence_number.load(Ordering::SeqCst),
                            }
                        })
                        .to_string();
                        tx.send(Message::Text(resume)).unwrap();
                        identified.store(true, Ordering::SeqCst);
                    }
                    Opcode::InvalidSession => {
                        // check if session can be resumed
                        let resumable = payload.d.unwrap().as_bool().unwrap();
                        if !resumable {
                            // Reset connection
                            heartbeater.lock().await.take().unwrap().abort();
                            return Ok(());
                        }

                        // try new identification
                        let identify_message = create_identify_message(&config);
                        tx.send(identify_message).unwrap();
                        identified.store(true, Ordering::SeqCst);
                    }
                    Opcode::Hello => {
                        // setup heartbeater on first reply
                        let heartbeat = payload.d.unwrap()["heartbeat_interval"].as_u64().unwrap();
                        let seq_num = Arc::clone(&sequence_number);

                        *heartbeater.lock().await = Some(tokio::spawn(async move {
                            let mut interval = tokio::time::interval(Duration::from_millis(heartbeat));
                            loop {
                                interval.tick().await;
                                let hb_message = json! ({
                                    "op": Opcode::Heartbeat,
                                    "d": seq_num.load(Ordering::SeqCst)
                                });
                                tx.send(Message::text(hb_message.to_string()))?;
                            }
                        }));
                    }
                    Opcode::HeartbeatAck => {
                        // on first ack send identify event
                        if !identified.load(Ordering::SeqCst) {
                            let identify_message = create_identify_message(&config);
                            tx.send(identify_message).unwrap();
                            identified.store(true, Ordering::SeqCst);
                        }
                    }
                    Opcode::Identify | Opcode::PresenceUpdate | Opcode::VoiceStateUpdate | Opcode::Resume | Opcode::RequestGuildMembers => {
                        // according to Discord API docs, these opcodes are only sent, not recieveed, so if they are recieved, there is an error/bug
                        log_message(format!("Recieved unexpected opcode: {:?}", payload.op));
                    }
                }
                Ok::<(), BotError>(())
            });
        }

        match sender.await? {
            Ok(_) => log_message("Another connection iteration"),
            Err(e) => {
                log_message(format!("Error {:#?}", e));
            }
        }
    }
}

/// Called when bot recieves message
async fn on_message_create(
    message: DiscordMessage,
    config: Arc<Config>,
    client: Client,
    avatar_cache: Arc<Mutex<AvatarCache>>,
    my_id: &str,
) -> Result<(), BotError> {
    if !message.content.starts_with(&config.command) {
        return Ok(());
    }

    // send error messeage
    if message.mentions.is_empty() {
        match message.mention_roles {
            Some(roles) if !roles.is_empty() => {
                let res = send_reply(&client, &message.channel_id, &message.id, &config.err_msg_tag_role.clone().unwrap()).await?;
                Result::from(res)?;
                log_message("Error - tagged role");
            }
            _ => {
                let res = send_reply(&client, &message.channel_id, &message.id, &config.err_msg_tag_nobody.clone().unwrap()).await?;
                Result::from(res)?;
                log_message("Error - tagged nobody");
            }
        }
    }

    // brick everyone mentioned
    for user in message.mentions {
        if user.id == my_id {
            if let Some(ref self_message) = config.self_brick_message {
                send_reply(&client, &message.channel_id, &message.id, self_message).await?;
                log_message(format!("Bricked self"));
                continue;
            }
        }

        let avatar = {
            let mut lock = avatar_cache.lock().await;
            let avatar = lock.get(&client, &user).await?.clone();
            avatar
        };

        let image = brickify_gif(BRICK_GIF.get().unwrap(), &avatar, &config).await?;

        let image_res: DiscordResult<DiscordMessage> = send_image(&client, &message.channel_id, &image, &config).await?;
        Result::from(image_res)?;

        log_message(format!("Bricked user \"{}\"", user.username));
    }

    Ok(())
}

async fn prepare_config() -> Result<Arc<Config>> {
    let config = tokio::fs::read_to_string("bot.toml").await.context("Error loading bot configuration.")?;
    let config: Config = toml::from_str(&config).context("Could not parse settings")?;
    // It's safe to unwrap string options now
    Ok(Arc::from(config.set_missing()))
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

async fn send_reply(client: &Client, channel_id: &str, reply_id: &str, reply: &str) -> Result<DiscordResult<DiscordMessage>, BotError> {
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
        .json::<DiscordResult<DiscordMessage>>()
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

fn create_identify_message(config: &Arc<Config>) -> Message {
    let token = &config.token;
    // TODO move to config
    let activity_name = "Brickity brick 🧱";

    let identify = json!({
        "op": Opcode::Identify,
        "d": {
            "token": token,
            "intents": 512,
            "properties": {
                "$os": "windows",
                "$browser": "brick-bot",
                "$device": "brick-bot"
            },
            "presence": {
                "activities": [{
                    "name": activity_name,
                    "type": 0
                }],
                "status": "online",
                "since": 0,
                "afk": false
            },
        }
    })
    .to_string();

    Message::Text(identify)
}
