use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result};

use log::{error, info, LevelFilter};
use reqwest::Client;
use simple_logger::SimpleLogger;
use tokio::sync::Mutex;

use brick_bot::{
    bot::BotBuilder,
    structs::{DiscordEvent, Status, StatusType},
    AvatarCache, BotError,
};

/// Bot config storage and parsing
pub mod config;
use config::Config;
/// event handlers specific to brick-bot behaviour
mod event_handlers;
/// Image editing
pub mod image_edit;

#[tokio::main]
async fn main() -> Result<()> {
    SimpleLogger::new()
        .with_level(LevelFilter::Warn)
        .with_module_level("brick_bot", LevelFilter::Info)
        .init()
        .context("Logger could not be enabled")?;

    let config = prepare_config().await?;
    let cache = Arc::new(Mutex::new(AvatarCache::new()));
    let gif_cache = Arc::new(Mutex::new(HashMap::new()));

    let gifs = load_gifs(&config).await?;

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
                    let gif_cache = Arc::clone(&gif_cache);
                    let bot_id = Arc::clone(&bot_id);
                    let gifs = Arc::clone(&gifs);
                    let client = client.clone();
                    let config = Arc::clone(&config);

                    tokio::spawn(async move {
                        event_handlers::on_message_create(message, &config, client, cache, bot_id, gifs, gif_cache).await?;
                        Ok::<(), BotError>(())
                    });
                    _status_tx.send(Status::new(StatusType::Online, "🦆 quack")).await.unwrap();
                }

                DiscordEvent::Ready(ready) => {
                    *bot_id.lock().await = Some(ready.user.id);
                    _status_tx.send(Status::new(StatusType::Online, "Bricking idiots 🧱")).await.unwrap();
                }

                DiscordEvent::ReactionAdd(reaction) => {
                    // clone all needed stuff
                    let cache = Arc::clone(&cache);
                    let gif_cache = Arc::clone(&gif_cache);
                    let bot_id = Arc::clone(&bot_id);
                    let gifs = Arc::clone(&gifs);
                    let client = client.clone();
                    let config = Arc::clone(&config);

                    tokio::spawn(async move {
                        event_handlers::on_reaction_add(reaction, &config, client, cache, bot_id, gifs, gif_cache).await?;
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
        Ok((bot_res, _handler_res)) => {
            info!("Shutting down");
            if let Err(e) = bot_res {
                error!("{}", e);
            }
        }
    }

    Ok(())
}

async fn prepare_config() -> Result<Arc<Config>> {
    let config = tokio::fs::read_to_string("bot.toml").await.context("Error loading bot configuration.")?;
    let config: Config = toml::from_str(&config).context("Could not parse settings")?;

    let config = Arc::new(config);
    Ok(config)
}

async fn load_gifs(config: &Arc<Config>) -> Result<Arc<HashMap<String, Vec<u8>>>> {
    let mut gifs = HashMap::new();

    for (name, command) in &config.commands {
        let gif = tokio::fs::read(command.image_path.clone()).await.context("cannot find image on given path")?;

        gifs.insert(name.to_owned(), gif);
    }

    //let brick_gif = tokio::fs::read(config.image_path.clone()).await.context("cannot find image on given path")?;

    Ok(Arc::new(gifs))
}
