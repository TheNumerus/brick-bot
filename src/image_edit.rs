use bytes::{Buf, BufMut, Bytes, BytesMut};
use image::{codecs::gif::*, AnimationDecoder, GenericImageView, ImageFormat};

use crate::config::Config;

use crate::error::BotError;

pub async fn brickify_gif(source: &[u8], avatar: &Bytes, config: &Config) -> Result<Bytes, BotError> {
    let avatar = image::load_from_memory_with_format(avatar.as_ref(), ImageFormat::Png)?;

    let (max_x, max_y) = avatar.dimensions();

    let decoder = GifDecoder::new(source.clone().reader()).unwrap();

    let mut frames = decoder.into_frames().collect_frames()?;

    for (frame_num, frame) in &mut frames.iter_mut().enumerate() {
        let keyframe = config.keyframes.get(&frame_num.to_string());
        let (offset_x, offset_y) = if let Some(k) = keyframe { (k.x, k.y) } else { (0, 0) };
        let visible = if let Some(k) = keyframe { k.visible.unwrap_or(true) } else { true };
        let scale = if let Some(k) = keyframe { k.scale.unwrap_or(1.0) } else { 1.0 };
        if !visible {
            continue;
        }
        for (x, y, pixel) in frame.buffer_mut().enumerate_pixels_mut() {
            let mapped_x = ((x - offset_x) as f32 / scale) as u32;
            let mapped_y = ((y - offset_y) as f32 / scale) as u32;

            if (mapped_x > 0) && (mapped_x < max_x) {
                if (mapped_y > 0) && (mapped_y < max_y) {
                    if config.use_avatar_alpha {
                        let new_pixel = avatar.get_pixel(mapped_x, mapped_y).0;
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
    }

    let dest = BytesMut::with_capacity(500 * 1024 * 1024);
    let mut writer = dest.writer();

    {
        let mut encoder = GifEncoder::new(&mut writer);
        encoder.set_repeat(Repeat::Infinite)?;
        encoder.encode_frames(frames.into_iter())?;
    }

    Ok(writer.into_inner().freeze())
}
