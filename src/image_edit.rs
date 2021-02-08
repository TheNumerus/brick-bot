use anyhow::Context;
use bytes::Bytes;
use image::ImageFormat;

use crate::{BotError, BRICK_GIF};

pub async fn brickify_gif(avatar: &Bytes) -> Result<Bytes, BotError> {
    let source = &BRICK_GIF;
    let source = Bytes::copy_from_slice(source.get().unwrap());

    let background = image::load_from_memory_with_format(source.as_ref(), ImageFormat::Gif)?;

    let avatar = image::load_from_memory_with_format(avatar.as_ref(), ImageFormat::Png)?;

    Ok(source)
}
