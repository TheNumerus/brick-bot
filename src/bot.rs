use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use log::{debug, error, info};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use tokio::{
    sync::{
        mpsc::{error::SendError, Receiver, Sender},
        Mutex,
    },
    task::JoinHandle,
};
use tokio_tungstenite::tungstenite::Message;

use crate::{
    structs::{DiscordEvent, Interaction, Message as DiscordMessage, Opcode, Payload, ReactionAddResponse, Ready, Status},
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
    pub fn build(&self) -> Result<(Bot, Receiver<DiscordEvent>, Sender<Status>), BotError> {
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(10);
        let (status_tx, status_rx) = tokio::sync::mpsc::channel(10);
        // check for token
        if let None = self.token {
            return Err(BotError::InternalError(String::from("Cannot build bot without token.")));
        }

        let bot = Bot {
            event_tx,
            status_rx: Some(status_rx),
            token: self.token.as_ref().unwrap().clone(),
            state: Arc::new(State::new()),
        };
        Ok((bot, event_rx, status_tx))
    }
}

pub struct Bot {
    event_tx: Sender<DiscordEvent>,
    status_rx: Option<Receiver<Status>>,
    token: String,
    state: Arc<State>,
}

impl Bot {
    pub async fn run(&mut self) -> Result<(), BotError> {
        // start state changer
        let state_rx = Arc::new(Mutex::new(self.status_rx.take().unwrap()));

        // will enter enother iteration only when discord needs another connection
        loop {
            let (sender_tx, mut sender_rx) = tokio::sync::mpsc::channel(10);

            let (ws_stream, _ws_res) = tokio_tungstenite::connect_async("wss://gateway.discord.gg/?v=8&encoding=json")
                .await
                .map_err(|_e| BotError::InternalError(String::from("Failed to connect to Discord servers.")))?;
            info!("WebSocket connected");

            let (mut write, mut read) = ws_stream.split();

            self.state.reset_session().await;

            let state_clone = Arc::clone(&self.state);
            // this task will send all messages from channel to websocket stream
            let sender = tokio::spawn(async move {
                while let Some(m) = sender_rx.recv().await {
                    // check if connection is even open before we send something
                    if state_clone.connecion_open.load(Ordering::SeqCst) {
                        write.send(m).await?;
                        debug!("Sent payload");
                    } else {
                        return Ok(());
                    }
                }
                Ok::<(), tokio_tungstenite::tungstenite::error::Error>(())
            });

            let state = Arc::clone(&self.state);
            let state_rx = Arc::clone(&state_rx);
            let sender_tx_clone = sender_tx.clone();
            let _state_changer = tokio::spawn(async move {
                while let Some(new_state) = {
                    let x = state_rx.lock().await.recv().await;
                    x
                } {
                    if state.identified.load(Ordering::SeqCst) {
                        let json = json!({
                            "op": Opcode::PresenceUpdate,
                            "d": new_state
                        });
                        sender_tx_clone.send(Message::text(json.to_string())).await?;
                    } else {
                        return Ok::<_, SendError<Message>>(state_rx);
                    }
                }
                Ok::<_, SendError<Message>>(state_rx)
            });

            // maybe something could be done to infer this type
            let heartbeater: Arc<Mutex<Option<JoinHandle<Result<(), SendError<Message>>>>>> = Arc::new(Mutex::new(None));

            while let Some(message) = read.next().await {
                // close on error
                if let Err(_) = message {
                    self.state.connecion_open.store(false, Ordering::SeqCst);
                    break;
                }

                let message = message.unwrap();

                let sender_tx = sender_tx.clone();
                let event_tx = self.event_tx.clone();
                let heartbeater = Arc::clone(&heartbeater);
                let state = Arc::clone(&self.state);
                let token = self.token.clone();

                tokio::spawn(async move {
                    let response_result = Self::handle_message(message, state, event_tx, sender_tx, token, heartbeater).await;
                    if let Err(err) = response_result {
                        error!("{:#?}", err);
                    }
                    Ok::<(), BotError>(())
                });
            }

            match sender.await.map_err(|_e| BotError::InternalError(String::from("Task joining error"))) {
                Ok(_) => info!("WebSocket reconnecting"),
                Err(e) => {
                    error!("Error {:#?}", e);
                }
            }
        }
    }

    async fn handle_message(
        message: Message,
        state: Arc<State>,
        event_tx: Sender<DiscordEvent>,
        sender_tx: Sender<Message>,
        token: String,
        heartbeater: Arc<Mutex<Option<JoinHandle<Result<(), SendError<Message>>>>>>,
    ) -> Result<(), BotError> {
        // return if connection closed
        if let Message::Close(frame) = message {
            state.connecion_open.store(false, Ordering::SeqCst);
            info!("Connection closed");
            if let Some(frame) = frame {
                error!("Error {}: {}", frame.code, frame.reason);
            }

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

                let event_name = payload.t.ok_or_else(|| BotError::ApiError(String::from("recieved event without name")))?;
                debug!("Recieved event \"{}\"", event_name);

                match event_name.as_str() {
                    "MESSAGE_CREATE" => {
                        let message: DiscordMessage = serde_json::from_value(payload.d.unwrap()).unwrap();
                        event_tx.send(DiscordEvent::MessageCreate(message)).await.unwrap();
                    }
                    "READY" => {
                        let ready: Ready = payload_data_to_exact_type(payload.d, &event_name)?;

                        // update session id for resuming later
                        *state.session_id.lock().await = Some(ready.session_id.clone());

                        event_tx.send(DiscordEvent::Ready(ready)).await.unwrap();
                    }
                    "INTERACTION_CREATE" => {
                        let interaction: Interaction = payload_data_to_exact_type(payload.d, &event_name)?;
                        event_tx.send(DiscordEvent::InteractionCreate(interaction)).await.unwrap();
                    }
                    "MESSAGE_REACTION_ADD" => {
                        let reaction: ReactionAddResponse = payload_data_to_exact_type(payload.d, &event_name)?;
                        event_tx.send(DiscordEvent::ReactionAdd(reaction)).await.unwrap();
                    }
                    _ => {
                        info!("Recieved unsupported event {}", event_name);
                        return Ok(());
                    }
                }
            }
            Opcode::Heartbeat => {
                // send new heartbeat on request
                let hb_message = json! ({
                    "op": Opcode::Heartbeat,
                    "d": state.sequence_number.load(Ordering::SeqCst)
                });
                sender_tx.send(Message::text(hb_message.to_string())).await.unwrap();
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

                sender_tx.send(Message::Text(resume.to_string())).await.unwrap();
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
                sender_tx.send(identify_message).await.unwrap();
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
                        sender_tx.send(Message::text(hb_message.to_string())).await?;
                        debug!("Sent heartbeat");
                    }
                }));
            }
            Opcode::HeartbeatAck => {
                // on first ack send identify event
                if !state.identified.load(Ordering::SeqCst) {
                    let identify_message = create_identify_message(&token);
                    sender_tx.send(identify_message).await.unwrap();
                    state.identified.store(true, Ordering::SeqCst);
                }
            }
            Opcode::Identify | Opcode::PresenceUpdate | Opcode::VoiceStateUpdate | Opcode::Resume | Opcode::RequestGuildMembers => {
                // according to Discord API docs, these opcodes are only sent, not recieveed, so if they are recieved, there is an error/bug
                error!("Recieved unexpected opcode: {:?}", payload.op);
            }
        }
        Ok::<(), BotError>(())
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
            "intents": 1536,
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

fn payload_data_to_exact_type<T>(data: Option<Value>, event_name: &str) -> Result<T, BotError>
where
    T: DeserializeOwned,
{
    match data {
        Some(data) => {
            let deserialized: T = serde_json::from_value(data)?;
            Ok(deserialized)
        }
        None => {
            let err_msg = format!("event {} did not contain any data", event_name);
            Err(BotError::ApiError(err_msg))
        }
    }
}
