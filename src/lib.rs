/// API client
pub mod bot;
/// Caches for avatars, and composited gifs
mod cache;
/// Brick bot configuraion
pub mod config;
mod error;
pub mod image_edit;
/// REST API methods
pub mod rest;
/// Discord API objects
pub mod structs;

pub use cache::AvatarCache;
pub use error::BotError;
