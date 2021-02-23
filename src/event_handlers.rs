use std::sync::Arc;

use brick_bot::{
    config::Config,
    image_edit::brickify_gif,
    rest::{send_image, send_reply},
    structs::{DiscordResult, Message},
    AvatarCache, BotError,
};
use reqwest::Client;
use tokio::sync::Mutex;

use crate::log_message;

/// Called when bot recieves message
pub async fn on_message_create(
    message: Message,
    config: &Config,
    client: Client,
    avatar_cache: Arc<Mutex<AvatarCache>>,
    bot_id: Arc<Mutex<Option<String>>>,
    brick_gif: Arc<Vec<u8>>,
) -> Result<(), BotError> {
    // need id before anything else
    let bot_id = {
        match &*bot_id.lock().await {
            Some(id) => id.to_owned(),
            // if it's `None`, event `MESSAGE_CRATE` was recieved before bot was identified, which is impossible
            None => return Err(BotError::InternalError(String::from("Bot id not set"))),
        }
    };

    if message.content.contains("čtvrtek") && message.author.id != bot_id {
        let res = send_reply(&client, &message.channel_id, &message.id, "posere tě krtek", &config).await?;
        Result::from(res)?;
        log_message(format!("Pooped {}", message.author.username));
        return Ok(());
    }

    if !message.content.starts_with(&config.command) {
        return Ok(());
    }

    // send error messeage
    if message.mentions.is_empty() {
        match message.mention_roles {
            Some(roles) if !roles.is_empty() => {
                let res = send_reply(&client, &message.channel_id, &message.id, &config.err_msg_tag_role.clone(), &config).await?;
                Result::from(res)?;
                log_message("Error - tagged role");
            }
            _ => {
                let res = send_reply(&client, &message.channel_id, &message.id, &config.err_msg_tag_nobody.clone(), &config).await?;
                Result::from(res)?;
                log_message("Error - tagged nobody");
            }
        }
    }

    // brick everyone mentioned
    for user in message.mentions {
        if user.id == bot_id {
            if let Some(ref self_message) = config.self_brick_message {
                send_reply(&client, &message.channel_id, &message.id, self_message, &config).await?;
                log_message(format!("Bricked self"));
                continue;
            }
        }

        let avatar = {
            let mut lock = avatar_cache.lock().await;
            let avatar = lock.get(&client, &user).await?.clone();
            avatar
        };

        let image = brickify_gif(&brick_gif, &avatar, &config).await?;

        let image_res: DiscordResult<Message> = send_image(&client, &message.channel_id, &image, &config).await?;
        Result::from(image_res)?;

        log_message(format!("Bricked user \"{}\"", user.username));
    }

    Ok(())
}
