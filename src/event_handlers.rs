use std::{collections::HashMap, sync::Arc, time::Instant};

use brick_bot::{
    config::Config,
    image_edit::brickify_gif,
    rest::*,
    structs::{DiscordResult, Message, ReactionAddResponse, User},
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

    if message.content.to_lowercase().contains("čtvrtek") && message.author.id != bot_id {
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
    for user in &message.mentions {
        if user.id == bot_id {
            if let Some(ref self_message) = config.self_brick_message {
                send_reply(&client, &message.channel_id, &message.id, self_message, &config).await?;
                info!("Bricked self");
                continue;
            }
        }

        let bricked_gif = gen_brick_gif(&bricked_gifs_cache, &message.author, &avatar_cache, &client, &brick_gif, config).await?;

        let image_res: DiscordResult<Message> = send_image(&client, &message.channel_id, &bricked_gif, &config).await?;
        Result::from(image_res)?;
        info!("Bricked user \"{}\"", user.username);
    }

    debug!("on_message_create response time: {}s", start.elapsed().as_secs_f32());

    Ok(())
}

pub async fn on_reaction_add(
    reaction: ReactionAddResponse,
    config: &Config,
    client: Client,
    avatar_cache: Arc<Mutex<AvatarCache>>,
    bot_id: Arc<Mutex<Option<String>>>,
    brick_gif: Arc<Vec<u8>>,
    bricked_gifs_cache: Arc<Mutex<HashMap<(String, String), Bytes>>>,
) -> Result<(), BotError> {
    // measure handler time
    let start = Instant::now();

    // discard non bricks
    if reaction.emoji.name != "🧱" {
        return Ok(());
    }

    // fetch message
    let message = get_message_from_channel(&client, config, &reaction.channel_id, &reaction.message_id).await?;
    let message = Result::from(message)?;

    // check if there is only one brick
    if let Some(ref reactions) = message.reactions {
        let brick_reaction = reactions.iter().find(|r| r.emoji.name == "🧱");
        if let Some(brick_reaction) = brick_reaction {
            if brick_reaction.count != 1 {
                return Ok(());
            }
        }
    }

    let bot_id = {
        match &*bot_id.lock().await {
            Some(id) => id.to_owned(),
            // if it's `None`, event `INTERACTION_CRATE` was recieved before bot was identified, which is impossible
            None => return Err(BotError::InternalError(String::from("Bot id not set"))),
        }
    };

    // Don't brick the bot
    if message.author.id == bot_id {
        return Ok(());
    }

    let bricked_gif = gen_brick_gif(&bricked_gifs_cache, &message.author, &avatar_cache, &client, &brick_gif, config).await?;

    let image_res: DiscordResult<Message> = send_image(&client, &reaction.channel_id, &bricked_gif, &config).await?;
    Result::from(image_res)?;
    info!("Bricked user \"{}\"", message.author.username);

    debug!("on_reaction_add response time: {}s", start.elapsed().as_secs_f32());

    Ok(())
}

async fn gen_brick_gif(
    bricked_gifs_cache: &Arc<Mutex<HashMap<(String, String), Bytes>>>,
    user: &User,
    avatar_cache: &Arc<Mutex<AvatarCache>>,
    client: &Client,
    brick_gif: &Arc<Vec<u8>>,
    config: &Config,
) -> Result<Bytes, BotError> {
    let bricked_gif = {
        let lock = bricked_gifs_cache.lock().await;
        lock.get(&(user.id.to_owned(), user.avatar.to_owned())).cloned()
    };
    let bricked_gif = match bricked_gif {
        Some(gif) => {
            debug!("Brick gif found in cache");
            gif
        }
        None => {
            let avatar = {
                let mut lock = avatar_cache.lock().await;
                let avatar = lock.get(client, &user).await?.clone();
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
    Ok(bricked_gif)
}
