use std::collections::HashMap;

use bytes::{Buf, BufMut, Bytes, BytesMut};
use image::{codecs::gif::*, AnimationDecoder, GenericImageView, ImageFormat};

use crate::config::{Command, Config, Keyframe};

use brick_bot::BotError;

pub fn brickify_gif(source: &[u8], avatar: &Bytes, config: &Config, command: &Command) -> Result<Bytes, BotError> {
    let avatar = image::load_from_memory_with_format(avatar.as_ref(), ImageFormat::Png)?;

    let (max_x, max_y) = avatar.dimensions();

    let decoder = GifDecoder::new(source.clone().reader()).unwrap();

    let mut frames = decoder.into_frames().collect_frames()?;

    for (frame_num, frame) in &mut frames.iter_mut().enumerate() {
        let interpolated = InterpolatedKeyframe::from_keyframes(&command.keyframes, frame_num);

        if !interpolated.visible {
            continue;
        }

        for (x, y, pixel) in frame.buffer_mut().enumerate_pixels_mut() {
            let mapped_x = ((x - interpolated.x) as f32 / interpolated.scale) as u32;
            let mapped_y = ((y - interpolated.y) as f32 / interpolated.scale) as u32;

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

    let dest = BytesMut::with_capacity(2 * 1024 * 1024);
    let mut writer = dest.writer();

    {
        let mut encoder = GifEncoder::new(&mut writer);
        encoder.set_repeat(Repeat::Infinite)?;
        encoder.encode_frames(frames.into_iter())?;
    }

    Ok(writer.into_inner().freeze())
}

struct InterpolatedKeyframe {
    x: u32,
    y: u32,
    scale: f32,
    visible: bool,
}

impl InterpolatedKeyframe {
    fn from_keyframes(keyframes: &HashMap<String, Keyframe>, frame: usize) -> Self {
        // first try to find exact keyframe
        if let Some(k) = keyframes.get(&frame.to_string()) {
            return k.into();
        }

        let keys = keyframes.keys().map(|a| a.parse::<usize>().unwrap()).collect::<Vec<_>>();

        let before = keys.iter().filter(|a| **a < frame).max();
        let after = keys.iter().filter(|a| **a > frame).min();

        match (before, after) {
            (None, None) => InterpolatedKeyframe::default(),
            (None, Some(a)) => keyframes.get(&a.to_string()).unwrap().into(),
            (Some(b), None) => keyframes.get(&b.to_string()).unwrap().into(),
            (Some(b), Some(a)) => {
                let before = keyframes.get(&b.to_string()).unwrap();
                let after = keyframes.get(&a.to_string()).unwrap();
                let blend_factor = (frame as f32 - *b as f32) / (*a as f32 - *b as f32);

                let blend = |a: f32, b: f32| (a as f32 + (blend_factor * (b as f32 - a as f32)));

                let blend_x = blend(before.x as f32, after.x as f32) as u32;
                let blend_y = blend(before.y as f32, after.y as f32) as u32;
                let blend_scale = blend(before.scale.unwrap_or(1.0), after.scale.unwrap_or(1.0));

                Self {
                    x: blend_x,
                    y: blend_y,
                    scale: blend_scale,
                    visible: before.visible.unwrap_or(true),
                }
            }
        }
    }
}

impl From<&Keyframe> for InterpolatedKeyframe {
    fn from(keyframe: &Keyframe) -> Self {
        Self {
            x: keyframe.x,
            y: keyframe.y,
            scale: keyframe.scale.unwrap_or(1.0),
            visible: keyframe.visible.unwrap_or(true),
        }
    }
}

impl Default for InterpolatedKeyframe {
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            scale: 1.0,
            visible: true,
        }
    }
}
