use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
pub struct Config {
    pub token: String,
    pub self_brick_message: Option<String>,
    pub image_path: PathBuf,
    pub err_msg_tag_role: Option<String>,
    pub err_msg_tag_nobody: Option<String>,
}
