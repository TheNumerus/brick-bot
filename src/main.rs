use std::{fmt::Display, sync::Arc};

use chrono::Utc;

use anyhow::{Context, Result};

use tokio::sync::Mutex;

use brick_bot::{bot::BotBuilder, config::Config, structs::DiscordEvent, AvatarCache, BotError};

/// event handlers specific to brick-bot behaviour
mod event_handlers;

#[tokio::main]
async fn main() -> Result<()> {
    let config = prepare_config().await?;
    let cache = Arc::new(Mutex::new(AvatarCache::new()));

    let brick_gif = tokio::fs::read(config.image_path.clone()).await.context("cannot find image on given path")?;
    let brick_gif = Arc::new(brick_gif);

    let client = reqwest::Client::new();

    let (mut bot, mut rx) = BotBuilder::new().token(config.token.clone()).build()?;
    let bot_task = tokio::spawn(async move { bot.run().await });

    let bot_id = Arc::new(Mutex::new(None));

    let event_handler = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            let cache = Arc::clone(&cache);
            let bot_id = Arc::clone(&bot_id);
            let brick_gif = Arc::clone(&brick_gif);
            let client = client.clone();
            let config = Arc::clone(&config);
            tokio::spawn(async move {
                match event {
                    DiscordEvent::MessageCreate(message) => {
                        event_handlers::on_message_create(message, &config, client, cache, bot_id, brick_gif).await?;
                    }

                    DiscordEvent::Ready(ready) => {
                        *bot_id.lock().await = Some(ready.user.id);
                    }
                }
                Ok::<(), BotError>(())
            });
        }
    });

    let res = tokio::try_join!(bot_task, event_handler);

    // TODO
    match res {
        Err(_e) => eprintln!("Shutting down"),
        _ => {}
    }

    Ok(())
}

async fn prepare_config() -> Result<Arc<Config>> {
    let config = tokio::fs::read_to_string("bot.toml").await.context("Error loading bot configuration.")?;
    let config: Config = toml::from_str(&config).context("Could not parse settings")?;

    let config = Arc::new(config);
    Ok(config)
}

pub fn log_message<T: AsRef<str> + Display>(message: T) {
    let time = Utc::now();

    let formated_time = time.format("%F %T");

    println!("[{}] - {}", formated_time, message);
}
