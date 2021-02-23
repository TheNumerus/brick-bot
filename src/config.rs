use serde::Deserialize;
use std::{collections::HashMap, path::PathBuf};

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    pub token: String,
    pub command: String,
    pub image_path: PathBuf,

    pub self_brick_message: Option<String>,
    #[serde(default = "defaults::tag_role_msg")]
    pub err_msg_tag_role: String,
    #[serde(default = "defaults::tag_nobody")]
    pub err_msg_tag_nobody: String,
    #[serde(default = "defaults::avatar_alpha")]
    pub use_avatar_alpha: bool,

    #[serde(default = "defaults::image_name")]
    pub image_name: String,
    pub keyframes: HashMap<String, Keyframe>,
}

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
        String::from("brick.gif")
    }

    pub fn avatar_alpha() -> bool {
        true
    }
}
