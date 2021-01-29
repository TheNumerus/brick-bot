use serde::Deserialize;
use serde_repr::*;

#[derive(Deserialize, Debug)]
pub struct GuildInfo {
    pub id: String,
    pub name: String,
}

#[derive(Deserialize, Debug)]
pub struct Channel {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub channel_type: ChannelType,
    pub last_message_id: Option<String>,
}

#[derive(Deserialize_repr, Debug, PartialEq)]
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

#[derive(Deserialize, Debug)]
pub struct Message {
    pub id: String,
    pub content: String,
    pub mentions: Vec<User>,
}

#[derive(Deserialize, Debug)]
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
