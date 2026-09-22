//! Use the pinned codecs directly: image's convenience JPEG decoder disables
//! strict mode, and its PNG decoder can stop before checking the final chunks.

use std::io::Cursor;

use image::ImageFormat;
use zune_core::{bytestream::ZCursor, options::DecoderOptions};

use super::AssetError;

const MAX_DECODED_BYTES: usize = 512 * 1024 * 1024;

pub(super) fn validate(format: ImageFormat, bytes: &[u8]) -> Result<(), AssetError> {
    match format {
        ImageFormat::Png => png(bytes),
        ImageFormat::Jpeg => jpeg(bytes),
        _ => Err(AssetError::UnsupportedMedia),
    }
}

fn png(bytes: &[u8]) -> Result<(), AssetError> {
    let decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: MAX_DECODED_BYTES,
        },
    );
    let mut reader = decoder.read_info().map_err(|_| AssetError::InvalidMedia)?;
    let size = reader
        .output_buffer_size()
        .filter(|&size| size <= MAX_DECODED_BYTES)
        .ok_or(AssetError::InvalidMedia)?;
    let mut output = vec![0; size];
    let frames = reader.info().animation_control.map_or(1, |control| {
        u64::from(control.num_frames) + u64::from(reader.info().frame_control.is_none())
    });
    for _ in 0..frames {
        reader
            .next_frame(&mut output)
            .map_err(|_| AssetError::InvalidMedia)?;
    }
    reader.finish().map_err(|_| AssetError::InvalidMedia)
}

fn jpeg(bytes: &[u8]) -> Result<(), AssetError> {
    if !jpeg_is_complete(bytes) {
        return Err(AssetError::InvalidMedia);
    }
    let options = DecoderOptions::default()
        .set_strict_mode(true)
        .set_max_width(usize::MAX)
        .set_max_height(usize::MAX);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    decoder
        .decode_headers()
        .map_err(|_| AssetError::InvalidMedia)?;
    if decoder
        .output_buffer_size()
        .is_none_or(|size| size > MAX_DECODED_BYTES)
    {
        return Err(AssetError::InvalidMedia);
    }
    decoder.decode().map_err(|_| AssetError::InvalidMedia)?;
    Ok(())
}

/// The codec can finish its pixel buffer without requiring an end marker, even
/// in strict mode. Walk framing separately so an EOI inside metadata or a
/// stuffed entropy byte cannot masquerade as the actual end of the image.
fn jpeg_is_complete(bytes: &[u8]) -> bool {
    if !bytes.starts_with(&[0xff, 0xd8]) {
        return false;
    }
    let mut position = 2;
    let mut saw_scan = false;
    while position < bytes.len() {
        if bytes[position] != 0xff {
            return false;
        }
        while bytes.get(position) == Some(&0xff) {
            position += 1;
        }
        let Some(&marker) = bytes.get(position) else {
            return false;
        };
        position += 1;
        if marker == 0xd9 {
            return saw_scan && position == bytes.len();
        }
        if matches!(marker, 0 | 0xd0..=0xd8) {
            return false;
        }
        if marker == 1 {
            continue;
        }
        let Some(length) = bytes.get(position..position + 2) else {
            return false;
        };
        let length = usize::from(u16::from_be_bytes([length[0], length[1]]));
        if length < 2 || length > bytes.len() - position {
            return false;
        }
        position += length;
        if marker == 0xda {
            saw_scan = true;
            loop {
                let Some(&byte) = bytes.get(position) else {
                    return false;
                };
                if byte != 0xff {
                    position += 1;
                    continue;
                }
                let start = position;
                while bytes.get(position) == Some(&0xff) {
                    position += 1;
                }
                match bytes.get(position) {
                    Some(0 | 0xd0..=0xd7) => position += 1,
                    Some(_) => {
                        position = start;
                        break;
                    }
                    None => return false,
                }
            }
        }
    }
    false
}
