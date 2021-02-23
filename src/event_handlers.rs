use std::{collections::HashMap, sync::Arc, time::Instant};

use brick_bot::{
    config::Config,
    image_edit::brickify_gif,
    rest::{send_image, send_reply},
    structs::{DiscordResult, Message},
    AvatarCache, BotError,
};
use bytes::Bytes;
use log::{debug, info};
use reqwest::Client;
use tokio::sync::Mutex;

/// Called when bot recieves message
pub async fn on_message_create(
    message: Message,
    config: &Config,
    client: Client,
    avatar_cache: Arc<Mutex<AvatarCache>>,
    bot_id: Arc<Mutex<Option<String>>>,
    brick_gif: Arc<Vec<u8>>,
    bricked_gifs_cache: Arc<Mutex<HashMap<(String, String), Bytes>>>,
) -> Result<(), BotError> {
    // measure handler time
    let start = Instant::now();

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
        info!("Pooped {}", message.author.username);
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
                info!("Error - tagged role");
            }
            _ => {
                let res = send_reply(&client, &message.channel_id, &message.id, &config.err_msg_tag_nobody.clone(), &config).await?;
                Result::from(res)?;
                info!("Error - tagged nobody");
            }
        }
    }

    // brick everyone mentioned
    for user in message.mentions {
        if user.id == bot_id {
            if let Some(ref self_message) = config.self_brick_message {
                send_reply(&client, &message.channel_id, &message.id, self_message, &config).await?;
                info!("Bricked self");
                continue;
            }
        }
        let bricked_gif = {
            let lock = bricked_gifs_cache.lock().await;
            lock.get(&(user.id.clone(), user.avatar.clone())).cloned()
        };

        let bricked_gif = match bricked_gif {
            Some(gif) => {
                debug!("Brick gif found in cache");
                gif
            }
            None => {
                let avatar = {
                    let mut lock = avatar_cache.lock().await;
                    let avatar = lock.get(&client, &user).await?.clone();
                    avatar
                };

                let gif = brickify_gif(&brick_gif, &avatar, &config)?;

                {
                    let mut lock = bricked_gifs_cache.lock().await;
                    lock.insert((user.id.clone(), user.avatar.clone()), gif.clone());
                }

                gif
            }
        };

        let image_res: DiscordResult<Message> = send_image(&client, &message.channel_id, &bricked_gif, &config).await?;
        Result::from(image_res)?;
        info!("Bricked user \"{}\"", user.username);
    }

    debug!("on_message_create response time: {}s", start.elapsed().as_secs_f32());

    Ok(())
}
