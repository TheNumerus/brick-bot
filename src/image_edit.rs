use std::time::Instant;

use bytes::{Buf, BufMut, Bytes, BytesMut};
use image::{codecs::gif::*, AnimationDecoder, GenericImageView, ImageFormat};

use crate::{config::Config, BotError, BRICK_GIF};

pub async fn brickify_gif(avatar: &Bytes, config: &Config) -> Result<Bytes, BotError> {
    let source = &BRICK_GIF;

    let avatar = image::load_from_memory_with_format(avatar.as_ref(), ImageFormat::Png)?;

    let (max_x, max_y) = avatar.dimensions();

    let timer = Instant::now();

    let decoder = GifDecoder::new(source.get().unwrap().reader()).unwrap();

    let mut frames = decoder.into_frames().collect_frames()?;

    for frame in &mut frames {
        for (x, y, pixel) in frame.buffer_mut().enumerate_pixels_mut() {
            if (x < max_x) && (y < max_y) {
                if config.use_avatar_alpha.unwrap_or_else(|| true) {
                    let new_pixel = avatar.get_pixel(x, y).0;
                    if new_pixel[3] > 128 {
                        pixel.0[0] = new_pixel[0];
                        pixel.0[1] = new_pixel[1];
                        pixel.0[2] = new_pixel[2];
                    }
                } else {
                    pixel.0 = avatar.get_pixel(x, y).0;
                }
            }
        }
    }

    let dest = BytesMut::with_capacity(500 * 1024 * 1024);
    let mut writer = dest.writer();

    {
        let mut encoder = GifEncoder::new(&mut writer);
        encoder.set_repeat(Repeat::Infinite)?;
        encoder.encode_frames(frames.into_iter())?;
    }

    println!("Re-encode time: {}", timer.elapsed().as_secs_f32());

    Ok(writer.into_inner().freeze())
}
