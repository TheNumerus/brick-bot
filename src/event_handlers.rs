use std::{collections::HashMap, sync::Arc, time::Instant};

use brick_bot::{
    rest::*,
    structs::{Message, ReactionAddResponse, User},
    BotError,
};

use crate::config::{Command, Config};
use crate::image_edit::brickify_gif;
use crate::Caches;

use bytes::Bytes;
use log::{debug, info};
use reqwest::Client;
use tokio::sync::Mutex;

/// Called when bot recieves message
pub async fn on_message_create(
    message: Message,
    config: &Config,
    client: Client,
    caches: Arc<Mutex<Caches>>,
    bot_id: Arc<Mutex<Option<String>>>,
    gifs: Arc<HashMap<String, Vec<u8>>>,
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
        let res = NewMessageBuilder::new(&client, &config.token, &message.channel_id)
            .message("posere tě krtek")
            .reply_to(&message.id)
            .send()
            .await?;
        Result::from(res)?;
        info!("Pooped {}", message.author.username);
        return Ok(());
    }

    for (command_name, command) in &config.commands {
        if !message.content.starts_with(&command.command) {
            continue;
        }

        // send error messeage
        if message.mentions.is_empty() {
            let message_builder = NewMessageBuilder::new(&client, &config.token, &message.channel_id).reply_to(&message.id);

            let message_builder = match message.mention_roles {
                Some(ref roles) if !roles.is_empty() => {
                    info!("Error - tagged role");
                    message_builder.message(&config.err_msg_tag_role)
                }
                _ => {
                    info!("Error - tagged nobody");
                    message_builder.message(&config.err_msg_tag_nobody)
                }
            };

            let reply_response = message_builder.send().await?;
            Result::from(reply_response)?;
        }

        // brick everyone mentioned
        for user in &message.mentions {
            if user.id == bot_id {
                if let Some(ref self_message) = command.self_tag_message {
                    NewMessageBuilder::new(&&client, &config.token, &message.channel_id)
                        .message(self_message)
                        .reply_to(&message.id)
                        .send()
                        .await?;
                    info!("Command {} used on bot", command_name);
                    continue;
                }
            }

            let bricked_gif = gen_brick_gif(&caches, &user, &client, &gifs[command_name], config, command).await?;

            let image_res = NewMessageBuilder::new(&client, &config.token, &message.channel_id)
                .file_with_filename(bricked_gif.to_vec(), &command.image_name)
                .send()
                .await?;

            Result::from(image_res)?;
            info!("Command \"{}\" used on user \"{}\"", command_name, user.username);
        }
    }

    debug!("on_message_create response time: {}s", start.elapsed().as_secs_f32());

    Ok(())
}

pub async fn on_reaction_add(
    reaction: ReactionAddResponse,
    config: &Config,
    client: Client,
    caches: Arc<Mutex<Caches>>,
    bot_id: Arc<Mutex<Option<String>>>,
    gifs: Arc<HashMap<String, Vec<u8>>>,
) -> Result<(), BotError> {
    // measure handler time
    let start = Instant::now();

    // discard non bricks
    if reaction.emoji.name != "🧱" {
        return Ok(());
    }

    // fetch message
    let message = get_message_from_channel(&client, &config.token, &reaction.channel_id, &reaction.message_id).await?;
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

    let bricked_gif = gen_brick_gif(&caches, &message.author, &client, &gifs["brick"], config, &config.commands["brick"]).await?;

    let image_res = NewMessageBuilder::new(&client, &config.token, &reaction.channel_id)
        .file_with_filename(bricked_gif.to_vec(), &config.commands["brick"].image_name)
        .send()
        .await?;
    Result::from(image_res)?;
    info!("Bricked user \"{}\"", message.author.username);

    debug!("on_reaction_add response time: {}s", start.elapsed().as_secs_f32());

    Ok(())
}

async fn gen_brick_gif(
    caches: &Arc<Mutex<Caches>>,
    user: &User,
    client: &Client,
    brick_gif: &Vec<u8>,
    config: &Config,
    command: &Command,
) -> Result<Bytes, BotError> {
    let bricked_gif = {
        let lock = caches.lock().await;
        lock.gifs
            .get(&(user.id.to_owned(), user.avatar.to_owned(), command.command.to_owned()))
            .cloned()
    };
    let bricked_gif = match bricked_gif {
        Some(gif) => {
            debug!("Brick gif found in cache");
            gif
        }
        None => {
            let avatar = {
                let mut lock = caches.lock().await;
                let avatar = lock.avatars.get(client, &user).await?.clone();
                avatar
            };

            let gif = brickify_gif(&brick_gif, &avatar, &config, &command)?;

            {
                let mut lock = caches.lock().await;
                lock.gifs
                    .insert((user.id.clone(), user.avatar.clone(), command.command.to_owned()), gif.clone());
            }

            gif
        }
    };
    Ok(bricked_gif)
}
