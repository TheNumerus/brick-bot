use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result};

use log::{error, info, LevelFilter};
use reqwest::Client;
use simple_logger::SimpleLogger;
use tokio::sync::Mutex;

use brick_bot::{
    bot::BotBuilder,
    config::Config,
    structs::{DiscordEvent, Status, StatusType},
    AvatarCache, BotError,
};

/// event handlers specific to brick-bot behaviour
mod event_handlers;

#[tokio::main]
async fn main() -> Result<()> {
    SimpleLogger::new()
        .with_level(LevelFilter::Warn)
        .with_module_level("brick_bot", LevelFilter::Info)
        .init()
        .context("Logger could not be enabled")?;

    let config = prepare_config().await?;
    let cache = Arc::new(Mutex::new(AvatarCache::new()));
    let bricked_cache = Arc::new(Mutex::new(HashMap::new()));

    let brick_gif = tokio::fs::read(config.image_path.clone()).await.context("cannot find image on given path")?;
    let brick_gif = Arc::new(brick_gif);

    let client = Client::new();

    let (mut bot, mut event_rx, _status_tx) = BotBuilder::new().token(config.token.clone()).build()?;
    let bot_task = tokio::spawn(async move { bot.run().await });

    let bot_id = Arc::new(Mutex::new(None));

    let event_handler = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            match event {
                DiscordEvent::MessageCreate(message) => {
                    // clone all needed stuff
                    let cache = Arc::clone(&cache);
                    let bricked_cache = Arc::clone(&bricked_cache);
                    let bot_id = Arc::clone(&bot_id);
                    let brick_gif = Arc::clone(&brick_gif);
                    let client = client.clone();
                    let config = Arc::clone(&config);

                    tokio::spawn(async move {
                        event_handlers::on_message_create(message, &config, client, cache, bot_id, brick_gif, bricked_cache).await?;
                        Ok::<(), BotError>(())
                    });
                    _status_tx.send(Status::new(StatusType::Online, "🦆 quack")).await.unwrap();
                }

                DiscordEvent::Ready(ready) => {
                    *bot_id.lock().await = Some(ready.user.id);
                }

                DiscordEvent::ReactionAdd(reaction) => {
                    // clone all needed stuff
                    let cache = Arc::clone(&cache);
                    let bricked_cache = Arc::clone(&bricked_cache);
                    let bot_id = Arc::clone(&bot_id);
                    let brick_gif = Arc::clone(&brick_gif);
                    let client = client.clone();
                    let config = Arc::clone(&config);

                    tokio::spawn(async move {
                        event_handlers::on_reaction_add(reaction, &config, client, cache, bot_id, brick_gif, bricked_cache).await?;
                        Ok::<(), BotError>(())
                    });
                }

                _ => {}
            }
        }
    });

    let res = tokio::try_join!(bot_task, event_handler);
    match res {
        Err(e) => error!("Shutting down with {:?}", e),
        _ => info!("Shutting down gracefully"),
    }

    Ok(())
}

async fn prepare_config() -> Result<Arc<Config>> {
    let config = tokio::fs::read_to_string("bot.toml").await.context("Error loading bot configuration.")?;
    let config: Config = toml::from_str(&config).context("Could not parse settings")?;

    let config = Arc::new(config);
    Ok(config)
}
