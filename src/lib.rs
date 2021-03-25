/// API client
pub mod bot;
/// Caches for avatars, and composited gifs
mod cache;
mod error;
/// REST API methods
pub mod rest;
/// Discord API objects
pub mod structs;

pub use cache::AvatarCache;
pub use error::BotError;
