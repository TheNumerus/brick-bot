use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result};

use bytes::Bytes;

use chrono::prelude::*;

use reqwest::Client;
use tokio::sync::Mutex;
use tracing::{error, info};

use brick_bot::{
    bot::BotBuilder,
    structs::{DiscordEvent, Status, StatusType},
    AvatarCache,
};

/// Bot config storage and parsing
pub mod config;
use config::{Command, Config};
use tracing_subscriber::{fmt::format::FmtSpan, prelude::__tracing_subscriber_SubscriberExt, EnvFilter};
/// event handlers specific to brick-bot behaviour
mod event_handlers;
/// Image editing
pub mod image_edit;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let config = prepare_config().await?;

    let caches = Caches::new();

    let gifs = load_gifs(&config).await?;

    let client = Client::new();

    let (mut bot, mut event_rx, status_tx) = BotBuilder::new(config.token.as_str()).build()?;
    let bot_task = tokio::spawn(async move { bot.run().await });

    let bot_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let event_handler = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            match event {
                DiscordEvent::MessageCreate(message) => {
                    // clone all needed stuff
                    let caches = Arc::clone(&caches);
                    let gifs = Arc::clone(&gifs);
                    let client = client.clone();
                    let config = Arc::clone(&config);

                    let bot_id = match *bot_id.lock().await {
                        Some(ref id) => id.to_owned(),
                        None => {
                            error!("Bot id not set");
                            return;
                        },
                    };

                    tokio::spawn(async move {
                        let event_res = event_handlers::on_message_create(message, &config, client, caches, &bot_id, gifs).await;
                        if let Err(e) = event_res {
                            error!("{}", e);
                        }
                    });
                    status_tx.send(Status::new(StatusType::Online, select_message())).await.unwrap();
                }

                DiscordEvent::Ready(ready) => {
                    *bot_id.lock().await = Some(ready.user.id);
                    status_tx.send(Status::new(StatusType::Online, select_message())).await.unwrap();
                }

                DiscordEvent::ReactionAdd(reaction) => {
                    // clone all needed stuff
                    let caches = Arc::clone(&caches);
                    let gifs = Arc::clone(&gifs);
                    let client = client.clone();
                    let config = Arc::clone(&config);

                    let bot_id = match *bot_id.lock().await {
                        Some(ref id) => id.to_owned(),
                        None => {
                            error!("Bot id not set");
                            return;
                        },
                    };

                    tokio::spawn(async move {
                        let event_res = event_handlers::on_reaction_add(reaction, &config, client, caches, &bot_id, gifs).await;
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
        if let Command::GifReply(ref config) = command {
            let gif = tokio::fs::read(config.image_path.clone())
                .await
                .with_context(|| format!("cannot find image {:?} for command {} on given path", config.image_path, name))?;

            gifs.insert(name.to_owned(), gif);
        }
    }

    Ok(Arc::new(gifs))
}

fn select_message() -> &'static str {
    let now = Local::now();
    let num = rand::random::<u32>();

    match num % 1000 {
        0 => "THE GAME",
        1 => "opli je debílek",
        2 => "to mu je teda",
        _ => match now.weekday() {
            Weekday::Wed => "it's wednesday my dudes",
            Weekday::Thu => "posere tě krtek",
            _ => "with 🧱",
        },
    }
}

fn init_tracing() {
    let filter = EnvFilter::new("brick_bot=INFO");

    let console_logger = tracing_subscriber::fmt().with_span_events(FmtSpan::NEW | FmtSpan::CLOSE).finish();

    let subscriber = console_logger.with(filter);

    tracing::subscriber::set_global_default(subscriber).unwrap();
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct GifKey {
    pub user: String,
    pub avatar_id: String,
    pub command: String,
}

impl GifKey {
    pub fn new(user: String, avatar_id: String, command: String) -> Self {
        Self { user, avatar_id, command }
    }
}

pub struct Caches {
    avatars: AvatarCache,
    gifs: HashMap<GifKey, Bytes>,
}

impl Caches {
    pub fn new() -> Arc<Mutex<Self>> {
        let avatars = AvatarCache::new();
        let gifs = HashMap::new();

        let caches = Self { avatars, gifs };

        Arc::new(Mutex::new(caches))
    }
}
