use serde::Deserialize;
use std::{collections::HashMap, path::PathBuf};

#[derive(Deserialize, Debug, Clone)]
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

impl Config {
    pub fn set_missing(self) -> Self {
        Self {
            err_msg_tag_role: self.err_msg_tag_role.or(Some(String::from("Error, tag user, not role"))),
            err_msg_tag_nobody: self.err_msg_tag_nobody.or(Some(String::from("Error, tag user"))),
            image_name: self.image_name.or(Some(String::from("brick.gif"))),
            use_avatar_alpha: self.use_avatar_alpha.or(Some(true)),
            ..self
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
pub struct Keyframe {
    pub x: u32,
    pub y: u32,
    pub scale: Option<f32>,
    pub visible: Option<bool>,
}
