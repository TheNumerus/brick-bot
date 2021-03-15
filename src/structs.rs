use serde::{Deserialize, Serialize};
use serde_repr::*;

use crate::error::BotError;

#[derive(Deserialize, Debug, Clone)]
pub struct GuildInfo {
    pub id: String,
    pub name: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Channel {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub channel_type: ChannelType,
    pub last_message_id: Option<String>,
}

#[derive(Deserialize_repr, Debug, PartialEq, Clone, Copy)]
#[repr(u8)]
pub enum ChannelType {
    GuildText = 0,
    Dm = 1,
    Voice = 2,
    GroupDm = 3,
    Category = 4,
    News = 5,
    Store = 6,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Message {
    pub id: String,
    pub channel_id: String,
    pub content: String,
    pub mentions: Vec<User>,
    pub mention_roles: Option<Vec<String>>,
    pub author: User,
    pub reactions: Option<Vec<Reaction>>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct User {
    pub id: String,
    pub username: String,
    pub avatar: String,
}

impl User {
    pub fn get_avatar_url(&self) -> String {
        format!("https://cdn.discordapp.com/avatars/{}/{}.png", self.id, self.avatar)
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum DiscordResult<T> {
    Ok(T),
    Error { message: String, code: usize },
}

impl<T> From<DiscordResult<T>> for Result<T, BotError> {
    fn from(dr: DiscordResult<T>) -> Self {
        match dr {
            DiscordResult::Ok(val) => Ok(val),
            DiscordResult::Error { message, code } => Err(BotError::ApiError(format!("Error #{}, {}", code, message))),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct Payload<T = serde_json::Value> {
    pub op: Opcode,
    pub d: Option<T>,
    pub s: Option<usize>,
    pub t: Option<String>,
}

/// All gateway events in Discord are tagged with an opcode that denotes the payload type.
/// Your connection to our gateway may also sometimes close.
/// When it does, you will receive a close code that tells you what happened.
#[derive(Debug, Deserialize_repr, Clone, PartialEq, Serialize_repr, Copy)]
#[repr(u16)]
pub enum Opcode {
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

#[derive(Debug, Deserialize, Clone)]
pub struct Ready {
    pub session_id: String,
    pub user: User,
}

#[derive(Debug, Clone)]
pub enum DiscordEvent {
    MessageCreate(Message),
    MessageUpdate(Message),
    MessageDelete(MessageDeleteResponse),
    Ready(Ready),
    InteractionCreate(Interaction),
    ReactionAdd(ReactionAddResponse),
    ReactionRemove(ReactionRemoveResponse),
}

#[derive(Debug, Deserialize, Clone)]
pub struct MessageDeleteResponse {
    pub id: String,
    pub channel_id: String,
    pub guild_id: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Interaction {
    pub id: String,
    pub guild_id: String,
    pub channel_id: String,
    pub token: String,
    pub member: serde_json::Value,
    pub data: serde_json::Value,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ReactionAddResponse {
    pub user_id: String,
    pub channel_id: String,
    pub message_id: String,
    // TODO add member
    pub guild_id: Option<String>,
    pub emoji: Emoji,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ReactionRemoveResponse {
    pub user_id: String,
    pub channel_id: String,
    pub message_id: String,
    pub guild_id: Option<String>,
    pub emoji: Emoji,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Reaction {
    pub emoji: Emoji,
    pub count: usize,
    pub me: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Emoji {
    pub id: Option<String>,
    pub name: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Status {
    pub status: StatusType,
    pub activities: Option<Vec<Activity>>,
    pub afk: bool,
    pub since: Option<usize>,
}

impl Status {
    pub fn new(status_type: StatusType, name: impl AsRef<str>) -> Self {
        let mut activities = Vec::with_capacity(1);
        activities.push(Activity {
            name: String::from(name.as_ref()),
            activity_type: 0,
        });
        Self {
            status: status_type,
            activities: Some(activities),
            afk: false,
            since: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Copy)]
pub enum StatusType {
    #[serde(rename = "online")]
    Online,
    #[serde(rename = "dnd")]
    DoNotDisturb,
    #[serde(rename = "idle")]
    Idle,
    #[serde(rename = "invisible")]
    Invisible,
    #[serde(rename = "offline")]
    Offline,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Activity {
    pub name: String,
    #[serde(rename = "type")]
    pub activity_type: usize,
}
