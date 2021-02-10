use serde::Deserialize;
use std::{collections::HashMap, path::PathBuf};

#[derive(Deserialize, Debug)]
pub struct Config {
    pub token: String,
    pub self_brick_message: Option<String>,
    pub image_path: PathBuf,
    pub err_msg_tag_role: Option<String>,
    pub err_msg_tag_nobody: Option<String>,
    pub use_avatar_alpha: Option<bool>,
    pub command: String,
    pub image_name: Option<String>,
    pub keyframes: HashMap<String, Keyframe>,
}

#[derive(Deserialize, Debug)]
pub struct Keyframe {
    pub x: u32,
    pub y: u32,
    pub scale: Option<f32>,
    pub visible: Option<bool>,
}
