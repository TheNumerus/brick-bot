use std::{
    fmt::Display,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use tokio::{
    sync::{
        mpsc::{error::SendError, UnboundedReceiver, UnboundedSender},
        Mutex,
    },
    task::JoinHandle,
};
use tokio_tungstenite::tungstenite::Message;

use crate::{
    structs::{DiscordEvent, Message as DiscordMessage, Opcode, Payload, Ready},
    BotError,
};

pub struct BotBuilder {
    token: Option<String>,
}

impl BotBuilder {
    pub fn new() -> Self {
        Self { token: None }
    }

    /// Sets token fot bot to use
    pub fn token(&mut self, token: String) -> &mut Self {
        self.token = Some(token);
        self
    }

    /// Builds [`Bot`]
    pub fn build(&self) -> Result<(Bot, UnboundedReceiver<DiscordEvent>), BotError> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        // check for token
        if let None = self.token {
            return Err(BotError::InternalError(String::from("Cannot build bot without token.")));
        }

        let bot = Bot {
            tx,
            token: self.token.as_ref().unwrap().clone(),
            state: Arc::new(State::new()),
        };
        Ok((bot, rx))
    }
}

pub struct Bot {
    tx: UnboundedSender<DiscordEvent>,
    token: String,
    state: Arc<State>,
}

impl Bot {
    pub async fn run(&mut self) -> Result<(), BotError> {
        // will enter enother iteration only when discord needs another connection
        loop {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

            let (ws_stream, _ws_res) = tokio_tungstenite::connect_async("wss://gateway.discord.gg/?v=8&encoding=json")
                .await
                .map_err(|_e| BotError::InternalError(String::from("Failed to connect to Discord servers.")))?;
            log_message("WebSocket connected");

            let (mut write, mut read) = ws_stream.split();

            self.state.reset_session().await;

            let state_clone = Arc::clone(&self.state);
            // this task will send all messages from channel to websocket stream
            let sender = tokio::spawn(async move {
                while let Some(m) = rx.recv().await {
                    // check if connection is even open before we send something
                    if state_clone.connecion_open.load(Ordering::SeqCst) {
                        write.send(m).await?;
                    } else {
                        return Ok(());
                    }
                }
                Ok::<(), tokio_tungstenite::tungstenite::error::Error>(())
            });

            //let state = &self.state;

            // maybe something could be done to infer this type
            let heartbeater: Arc<Mutex<Option<JoinHandle<Result<(), SendError<Message>>>>>> = Arc::new(Mutex::new(None));

            while let Some(message) = read.next().await {
                // close on error
                if let Err(_) = message {
                    self.state.connecion_open.store(false, Ordering::SeqCst);
                    break;
                }

                let tx = tx.clone();
                let heartbeater = Arc::clone(&heartbeater);
                let state = Arc::clone(&self.state);
                let token = self.token.clone();

                let tx_out = self.tx.clone();

                tokio::spawn(async move {
                    // return if close connection
                    // message is always Ok(_) here
                    let message = message.unwrap();
                    if let Message::Close(_) = message {
                        state.connecion_open.store(false, Ordering::SeqCst);
                        return Ok(());
                    }

                    let msg = message.to_string();
                    let payload: Payload = serde_json::from_str(&msg).unwrap();

                    match &payload.op {
                        Opcode::Dispatch => {
                            // handle all events here

                            // update seq number
                            let s = payload.s.to_owned().unwrap();
                            state.sequence_number.store(s, Ordering::SeqCst);

                            if let Some(event) = payload.t {
                                if event == "MESSAGE_CREATE" {
                                    let message: DiscordMessage = serde_json::from_value(payload.d.unwrap()).unwrap();
                                    tx_out.send(DiscordEvent::MessageCreate(message)).unwrap();
                                } else if event == "READY" {
                                    // update session id for resuming later
                                    let data: Ready = serde_json::from_value(
                                        payload
                                            .d
                                            .ok_or_else(|| BotError::ApiError(String::from("event READY did not contain any data")))?,
                                    )?;

                                    *state.session_id.lock().await = Some(data.session_id.clone());

                                    tx_out.send(DiscordEvent::Ready(data)).unwrap();
                                }
                            }
                        }
                        Opcode::Heartbeat => {
                            // send new heartbeat on request
                            let hb_message = json! ({
                                "op": Opcode::Heartbeat,
                                "d": state.sequence_number.load(Ordering::SeqCst)
                            });
                            tx.send(Message::text(hb_message.to_string())).unwrap();
                        }
                        Opcode::Reconnect => {
                            // try resuming connection
                            let resume = {
                                let lock = state.session_id.lock().await;
                                let session_id = (*lock).as_ref().unwrap();

                                json!({
                                    "op": Opcode::Resume,
                                    "d": {
                                        "token": &token,
                                        "session_id": session_id,
                                        "seq": state.sequence_number.load(Ordering::SeqCst),
                                    }
                                })
                            };

                            tx.send(Message::Text(resume.to_string())).unwrap();
                            state.identified.store(true, Ordering::SeqCst);
                        }
                        Opcode::InvalidSession => {
                            // check if session can be resumed
                            let resumable = payload.d.unwrap().as_bool().unwrap();
                            if !resumable {
                                // Reset connection
                                heartbeater.lock().await.take().unwrap().abort();
                                state.connecion_open.store(false, Ordering::SeqCst);
                                return Ok(());
                            }

                            // try new identification
                            let identify_message = create_identify_message(&token);
                            tx.send(identify_message).unwrap();
                            state.identified.store(true, Ordering::SeqCst);
                        }
                        Opcode::Hello => {
                            // setup heartbeater on first reply
                            let heartbeat = payload.d.unwrap()["heartbeat_interval"].as_u64().unwrap();
                            state.connecion_open.store(true, Ordering::SeqCst);
                            let state = Arc::clone(&state);

                            *heartbeater.lock().await = Some(tokio::spawn(async move {
                                let mut interval = tokio::time::interval(Duration::from_millis(heartbeat));
                                loop {
                                    interval.tick().await;
                                    let hb_message = json! ({
                                        "op": Opcode::Heartbeat,
                                        "d": state.sequence_number.load(Ordering::SeqCst)
                                    });
                                    tx.send(Message::text(hb_message.to_string()))?;
                                }
                            }));
                        }
                        Opcode::HeartbeatAck => {
                            // on first ack send identify event
                            if !state.identified.load(Ordering::SeqCst) {
                                let identify_message = create_identify_message(&token);
                                tx.send(identify_message).unwrap();
                                state.identified.store(true, Ordering::SeqCst);
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

            match sender.await.map_err(|_e| BotError::InternalError(String::from("Task joining error"))) {
                Ok(_) => log_message("WebSocket reconnecting"),
                Err(e) => {
                    log_message(format!("Error {:#?}", e));
                }
            }
        }
    }
}

#[derive(Debug)]
struct State {
    identified: AtomicBool,
    connecion_open: AtomicBool,
    sequence_number: AtomicUsize,
    session_id: Mutex<Option<String>>,
}

impl State {
    pub fn new() -> Self {
        Self {
            identified: AtomicBool::new(false),
            connecion_open: AtomicBool::new(false),
            sequence_number: AtomicUsize::new(0),
            session_id: Mutex::new(None),
        }
    }

    /// Resets identification, sequence number, connection status and session_id
    pub async fn reset_session(&self) {
        self.identified.store(false, Ordering::SeqCst);
        self.connecion_open.store(false, Ordering::SeqCst);
        self.sequence_number.store(0, Ordering::SeqCst);
        *self.session_id.lock().await = None
    }
}

fn create_identify_message(token: &String) -> Message {
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

pub fn log_message<T: AsRef<str> + Display>(message: T) {
    let time = Utc::now();

    let formated_time = time.format("%F %T");

    println!("[{}] - {}", formated_time, message);
}