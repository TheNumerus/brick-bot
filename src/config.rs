use serde::Deserialize;
use std::{collections::HashMap, path::PathBuf};

/// Brick-bot configuration storage
#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    /// Token to use when communicationg with Discord API
    pub token: String,
    /// Commands to listen to
    pub commands: HashMap<String, Command>,
    /// Error text sent on incorrect usage
    #[serde(default = "defaults::tag_role_msg")]
    pub err_msg_tag_role: String,
    /// Error text sent on incorrect usage
    #[serde(default = "defaults::tag_nobody")]
    pub err_msg_tag_nobody: String,
    /// If true, bot will use alpha channel of player avatar on compositing
    #[serde(default = "defaults::avatar_alpha")]
    pub use_avatar_alpha: bool,
}

/// Keyframe of animaiton
#[derive(Deserialize, Debug, Clone)]
pub struct Keyframe {
    pub x: u32,
    pub y: u32,
    pub scale: Option<f32>,
    pub visible: Option<bool>,
}

mod defaults {
    pub fn tag_role_msg() -> String {
        String::from("Error, tag user, not role")
    }

    pub fn tag_nobody() -> String {
        String::from("Error, tag user")
    }

    pub fn image_name() -> String {
        String::from("response.gif")
    }

    pub fn avatar_alpha() -> bool {
        true
    }
}

#[derive(Deserialize, Debug, Clone)]
pub enum KeywordPosition {
    #[serde(rename = "start")]
    Start,
    #[serde(rename = "everywhere")]
    Everywhere,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ReplyConfig {
    /// Where to search for keyword
    pub position: KeywordPosition,
    /// Keyword for command execution
    pub keyword: String,
    /// Text of reply message
    pub reply: String,
    /// Whitelist of users to reply to
    pub whitelist: Option<Vec<String>>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct GifReplyConfig {
    /// Where to search for keyword
    pub position: KeywordPosition,
    /// Keyword for command execution
    pub keyword: String,
    /// Optional reaction activation
    pub emoji: Option<String>,
    /// Path to brick gif to use
    pub image_path: PathBuf,
    /// If `Some`, bot will respond with this message if mentioned
    pub self_tag_message: Option<String>,
    /// Gif will be sent with this name to guild
    #[serde(default = "defaults::image_name")]
    pub image_name: String,
    /// Keyframes for avatar animation
    pub keyframes: HashMap<String, Keyframe>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(tag = "kind")]
pub enum Command {
    #[serde(rename = "reply")]
    Reply(ReplyConfig),
    #[serde(rename = "gif_reply")]
    GifReply(GifReplyConfig),
}
