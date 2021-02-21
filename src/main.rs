use std::{
    fmt::Display,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use chrono::Utc;
use once_cell::sync::OnceCell;

use anyhow::{Context, Result};

use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::{
    sync::{mpsc::error::SendError, Mutex},
    task::JoinHandle,
};
use tokio_tungstenite::tungstenite::protocol::Message;

use brick_bot::{
    config::Config,
    structs::{Message as DiscordMessage, Opcode, Payload, Ready},
    AvatarCache, BotError,
};

/// event handlers specific to brick-bot behaviour
mod event_handlers;

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

    // stores bot id for special interactions
    // bot id will probably not change while running, so store it here, so it's not reset on every connection loop
    let bot_id = Arc::new(Mutex::new(None));

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
            let bot_id = Arc::clone(&bot_id);
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

                                tokio::spawn(async move {
                                    event_handlers::on_message_create(message, config, client, cache, bot_id).await?;
                                    Ok::<(), BotError>(())
                                });
                            } else if event == "READY" {
                                // update session id for resuming later
                                let data: Ready = serde_json::from_value(
                                    payload
                                        .d
                                        .ok_or_else(|| BotError::ApiError(String::from("event READY did not contain any data")))?,
                                )?;

                                *session_id.lock().await = data.session_id;
                                *bot_id.lock().await = Some(data.user.id);
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

async fn prepare_config() -> Result<Arc<Config>> {
    let config = tokio::fs::read_to_string("bot.toml").await.context("Error loading bot configuration.")?;
    let config: Config = toml::from_str(&config).context("Could not parse settings")?;
    // It's safe to unwrap string options now
    Ok(Arc::from(config.set_missing()))
}

// TODO move these too

fn set_token(token: &str) -> Result<()> {
    TOKEN_HEADER.set(format!("Bot {}", token)).unwrap();
    Ok(())
}

pub fn log_message<T: AsRef<str> + Display>(message: T) {
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
