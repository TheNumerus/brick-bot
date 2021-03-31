use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result};

use bytes::Bytes;

use log::{error, info, LevelFilter};
use reqwest::Client;
use simple_logger::SimpleLogger;
use tokio::sync::Mutex;

use brick_bot::{
    bot::BotBuilder,
    structs::{DiscordEvent, Status, StatusType},
    AvatarCache,
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

    let caches = Caches::new();

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
                    let caches = Arc::clone(&caches);
                    let bot_id = Arc::clone(&bot_id);
                    let gifs = Arc::clone(&gifs);
                    let client = client.clone();
                    let config = Arc::clone(&config);

                    tokio::spawn(async move {
                        let event_res = event_handlers::on_message_create(message, &config, client, caches, bot_id, gifs).await;
                        if let Err(e) = event_res {
                            error!("{}", e);
                        }
                    });
                    _status_tx.send(Status::new(StatusType::Online, "🦆 quack")).await.unwrap();
                }

                DiscordEvent::Ready(ready) => {
                    *bot_id.lock().await = Some(ready.user.id);
                    _status_tx.send(Status::new(StatusType::Online, "Bricking idiots 🧱")).await.unwrap();
                }

                DiscordEvent::ReactionAdd(reaction) => {
                    // clone all needed stuff
                    let caches = Arc::clone(&caches);
                    let bot_id = Arc::clone(&bot_id);
                    let gifs = Arc::clone(&gifs);
                    let client = client.clone();
                    let config = Arc::clone(&config);

                    tokio::spawn(async move {
                        let event_res = event_handlers::on_reaction_add(reaction, &config, client, caches, bot_id, gifs).await;
                        if let Err(e) = event_res {
                            error!("{}", e);
                        }
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
        let gif = tokio::fs::read(command.image_path.clone())
            .await
            .with_context(|| format!("cannot find image {:?} for command {} on given path", command.image_path, name))?;

        gifs.insert(name.to_owned(), gif);
    }

    Ok(Arc::new(gifs))
}

pub struct Caches {
    avatars: AvatarCache,
    gifs: HashMap<(String, String, String), Bytes>,
}

impl Caches {
    pub fn new() -> Arc<Mutex<Self>> {
        let avatars = AvatarCache::new();
        let gifs = HashMap::new();

        let caches = Self { avatars, gifs };

        Arc::new(Mutex::new(caches))
    }
}
