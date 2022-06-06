use std::{collections::HashMap, sync::Arc};

use brick_bot::{
    rest::*,
    structs::{Message, ReactionAddResponse, User},
    BotError,
};

use crate::{config::KeywordPosition, Caches};
use crate::{
    config::{Command, Config},
    GifKey,
};
use crate::{
    config::{GifReplyConfig, ReplyConfig},
    image_edit::brickify_gif,
};

use bytes::Bytes;
use reqwest::Client;
use tokio::sync::Mutex;
use tracing::{debug, info};

/// Called when bot recieves message
#[tracing::instrument(skip_all)]
pub async fn on_message_create(
    message: Message,
    config: &Config,
    client: Client,
    caches: Arc<Mutex<Caches>>,
    bot_id: &str,
    gifs: Arc<HashMap<String, Vec<u8>>>,
) -> Result<(), BotError> {
    for (command_name, command) in &config.commands {
        let replied = match command {
            Command::Reply(reply_config) => reply_to_message(&message, &client, config, bot_id, reply_config).await?,
            Command::GifReply(gif_config) => {
                let base_gif = &gifs[command_name];
                reply_with_gif(&message, &client, config, bot_id, &caches, base_gif, gif_config).await?
            }
        };
        if replied {
            info!("Used command {} on {}", command_name, message.author.username);
        }
    }

    Ok(())
}

#[tracing::instrument(skip_all)]
pub async fn on_reaction_add(
    reaction: ReactionAddResponse,
    config: &Config,
    client: Client,
    caches: Arc<Mutex<Caches>>,
    bot_id: &str,
    gifs: Arc<HashMap<String, Vec<u8>>>,
) -> Result<(), BotError> {
    let message = get_message_from_channel(&client, &config.token, &reaction.channel_id, &reaction.message_id).await?;
    let message = Result::from(message)?;

    for (command_name, command) in &config.commands {
        if let Command::GifReply(gif_config) = command {
            let base_gif = &gifs[command_name];
            let replied = reply_to_reaction(&reaction, &message, &client, config, bot_id, &caches, base_gif, gif_config).await?;
            if replied {
                info!("Used command {} on {}", command_name, message.author.username);
            }
        }
    }

    Ok(())
}

async fn reply_to_message(message: &Message, client: &Client, config: &Config, bot_id: &str, reply_config: &ReplyConfig) -> Result<bool, BotError> {
    if message.author.id == bot_id {
        return Ok(false);
    }

    let is_user_target = match reply_config.whitelist {
        Some(ref users) => users.contains(&message.author.username),
        None => true,
    };

    if !is_user_target {
        return Ok(false);
    }

    let content = message.content.to_lowercase();

    let should_invoke = match reply_config.position {
        KeywordPosition::Start => content.starts_with(&reply_config.keyword),
        KeywordPosition::Everywhere => content.contains(&reply_config.keyword),
    };

    if !should_invoke {
        return Ok(false);
    }

    let res = NewMessageBuilder::new(client, &config.token, &message.channel_id)
        .message(&reply_config.reply)
        .reply_to(&message.id)
        .send()
        .await?;
    Result::from(res).map(|_| true)
}

async fn reply_with_gif(
    message: &Message,
    client: &Client,
    config: &Config,
    bot_id: &str,
    caches: &Arc<Mutex<Caches>>,
    base_gif: &[u8],
    gif_config: &GifReplyConfig,
) -> Result<bool, BotError> {
    let content = message.content.to_lowercase();

    let should_invoke = match gif_config.position {
        KeywordPosition::Start => content.starts_with(&gif_config.keyword),
        KeywordPosition::Everywhere => content.contains(&gif_config.keyword),
    };

    if !should_invoke {
        return Ok(false);
    }

    // send error messeage
    if message.mentions.is_empty() {
        let message_builder = NewMessageBuilder::new(client, &config.token, &message.channel_id).reply_to(&message.id);

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

    // send gif to everyone mentioned
    for user in &message.mentions {
        if let Some(msg) = &message.referenced_message {
            if user.id == msg.author.id {
                continue;
            }
        }
        if user.id == bot_id {
            if let Some(ref self_message) = gif_config.self_tag_message {
                NewMessageBuilder::new(client, &config.token, &message.channel_id)
                    .message(self_message)
                    .reply_to(&message.id)
                    .send()
                    .await?;
                continue;
            }
        }

        let bricked_gif = gen_brick_gif(caches, user, client, base_gif, config, gif_config).await?;

        let image_res = NewMessageBuilder::new(client, &config.token, &message.channel_id)
            .file_with_filename(bricked_gif.to_vec(), &gif_config.image_name)
            .send()
            .await?;

        Result::from(image_res).map(|_| true)?;
    }

    Ok(true)
}

async fn reply_to_reaction(
    reaction: &ReactionAddResponse,
    message: &Message,
    client: &Client,
    config: &Config,
    bot_id: &str,
    caches: &Arc<Mutex<Caches>>,
    base_gif: &[u8],
    gif_config: &GifReplyConfig,
) -> Result<bool, BotError> {
    let emoji = match gif_config.emoji {
        Some(ref emoji) => emoji,
        None => return Ok(false),
    };

    if reaction.emoji.name != *emoji {
        return Ok(false);
    }

    // check if there is only one reaction
    if let Some(ref reactions) = message.reactions {
        let brick_reaction = reactions.iter().find(|r| r.emoji.name == *emoji);
        if let Some(brick_reaction) = brick_reaction {
            if brick_reaction.count != 1 {
                return Ok(false);
            }
        }
    }

    if message.author.id == bot_id {
        if let Some(ref self_message) = gif_config.self_tag_message {
            let res = NewMessageBuilder::new(client, &config.token, &message.channel_id)
                .message(self_message)
                .reply_to(&message.id)
                .send()
                .await?;
            return Result::from(res).map(|_| true);
        }
    }

    let bricked_gif = gen_brick_gif(caches, &message.author, client, base_gif, config, gif_config).await?;

    let image_res = NewMessageBuilder::new(client, &config.token, &message.channel_id)
        .file_with_filename(bricked_gif.to_vec(), &gif_config.image_name)
        .send()
        .await?;

    Result::from(image_res).map(|_| true)
}

#[tracing::instrument(skip(caches, client, base_gif, config))]
async fn gen_brick_gif(
    caches: &Arc<Mutex<Caches>>,
    user: &User,
    client: &Client,
    base_gif: &[u8],
    config: &Config,
    gif_config: &GifReplyConfig,
) -> Result<Bytes, BotError> {
    let bricked_gif = {
        let lock = caches.lock().await;
        lock.gifs
            .get(&GifKey::new(user.id.to_owned(), user.avatar.to_owned(), gif_config.keyword.to_owned()))
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
                let avatar = lock.avatars.get(client, user).await?.clone();
                avatar
            };

            let gif = brickify_gif(base_gif, &avatar, config, gif_config)?;

            {
                let mut lock = caches.lock().await;
                lock.gifs
                    .insert(GifKey::new(user.id.clone(), user.avatar.clone(), gif_config.keyword.to_owned()), gif.clone());
            }

            gif
        }
    };
    Ok(bricked_gif)
}
