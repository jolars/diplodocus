use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Cursor;
use std::sync::Arc;

use image::{DynamicImage, ImageDecoder, ImageEncoder, ImageReader};

use crate::documents::PageLayout;
use crate::ir::AssetReference;
use crate::provenance::fingerprint_bytes;
use crate::site::{Site, SiteError};
use crate::validation::ContentAsset;

use super::RenderedFile;
use super::html::escape;

// These expressions follow the standalone theme's gutters, sidebars, and main width.
const CONTENT_SIZES: &str = "(max-width: 760px) calc(100vw - 2rem), (max-width: 1099px) min(47rem, calc(100vw - 25.2rem)), min(45rem, calc(100vw - 43.4rem))";
const WIDE_CONTENT_SIZES: &str =
    "(max-width: 760px) calc(100vw - 3rem), min(71rem, calc(100vw - 9rem))";
const FULL_CONTENT_SIZES: &str =
    "(max-width: 760px) calc(100vw - 2rem), min(92rem, calc(100vw - 4rem))";

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ImageRole {
    Content,
    Logo,
    Hero,
}

pub(super) fn logo_variants<'a>(
    light: Option<&'a AssetReference>,
    dark: Option<&'a AssetReference>,
) -> impl Iterator<Item = (&'a AssetReference, &'static str)> {
    let light_attribute = if dark.is_some() {
        " data-logo-theme=\"light\""
    } else {
        ""
    };
    light
        .map(|logo| (logo, light_attribute))
        .into_iter()
        .chain(dark.map(|logo| (logo, " data-logo-theme=\"dark\"")))
}

#[derive(Default)]
pub(crate) struct ImageCache(BTreeMap<(String, ImageRole), Arc<Vec<Variant>>>);

#[derive(Default)]
pub(super) struct Images<'a> {
    previous: Option<&'a ImageCache>,
    current: RefCell<ImageCache>,
}

#[derive(Clone)]
struct Variant {
    width: u32,
    path: String,
    file: RenderedFile,
}

impl<'a> Images<'a> {
    pub(super) fn new(previous: Option<&'a ImageCache>) -> Self {
        Self {
            previous,
            current: RefCell::default(),
        }
    }

    pub(super) fn attributes(
        &self,
        site: &Site<'_>,
        route: &str,
        digest: &str,
        role: ImageRole,
        standalone: bool,
    ) -> Result<String, SiteError> {
        self.prepare(site, digest, role)?;
        let asset = &site.assets[digest];
        let cache = self.current.borrow();
        let Some(variants) = cache.0.get(&(digest.to_owned(), role)) else {
            return Ok(String::new());
        };
        if variants.is_empty() {
            return Ok(String::new());
        }
        let mut sources = variants
            .iter()
            .map(|variant| format!("{} {}w", site.url(route, &variant.path), variant.width))
            .collect::<Vec<_>>();
        let (width, _) = ImageReader::new(Cursor::new(&asset.bytes))
            .with_guessed_format()
            .map_err(|_| SiteError::Evidence)?
            .into_dimensions()
            .map_err(|_| SiteError::Evidence)?;
        sources.push(format!(
            "{} {width}w",
            site.url(route, &site.asset_route(digest)?)
        ));
        let sizes = match role {
            ImageRole::Logo => "1.75rem",
            ImageRole::Hero if standalone => "(max-width: 760px) 12rem, 24rem",
            ImageRole::Content if standalone && site.home(route).is_none() => {
                match site.pages.get(route).map(|page| page.layout()) {
                    Some(PageLayout::Wide) => WIDE_CONTENT_SIZES,
                    Some(PageLayout::Full) => FULL_CONTENT_SIZES,
                    _ => CONTENT_SIZES,
                }
            }
            _ => "100vw",
        };
        Ok(format!(
            " srcset=\"{}\" sizes=\"{}\"",
            escape(&sources.join(", ")),
            escape(sizes),
        ))
    }

    pub(super) fn prepare(
        &self,
        site: &Site<'_>,
        digest: &str,
        role: ImageRole,
    ) -> Result<(), SiteError> {
        let asset = site.assets.get(digest).ok_or(SiteError::Evidence)?;
        if !matches!(asset.media_type.as_str(), "image/png" | "image/jpeg") {
            return Ok(());
        }
        let key = (digest.to_owned(), role);
        let mut cache = self.current.borrow_mut();
        if let std::collections::btree_map::Entry::Vacant(entry) = cache.0.entry(key) {
            let prepared = match self.previous.and_then(|cache| cache.0.get(entry.key())) {
                Some(previous) => Arc::clone(previous),
                None => Arc::new(variants(asset, role)?),
            };
            entry.insert(prepared);
        }
        Ok(())
    }

    pub(super) fn into_cache(self) -> ImageCache {
        self.current.into_inner()
    }
}

impl ImageCache {
    pub(super) fn into_files(self) -> impl Iterator<Item = (String, RenderedFile)> {
        self.0
            .into_values()
            .flat_map(Arc::unwrap_or_clone)
            .map(|variant| (variant.path, variant.file))
    }

    pub(super) fn files(&self) -> impl Iterator<Item = (&str, &RenderedFile)> {
        self.0
            .values()
            .flat_map(|variants| variants.iter())
            .map(|variant| (variant.path.as_str(), &variant.file))
    }
}

fn variants(asset: &ContentAsset, role: ImageRole) -> Result<Vec<Variant>, SiteError> {
    if asset.media_type == "image/png" {
        let reader = png::Decoder::new(Cursor::new(&asset.bytes))
            .read_info()
            .map_err(|_| SiteError::Evidence)?;
        // A static derivative would discard animation frames.
        if reader.info().animation_control.is_some() {
            return Ok(Vec::new());
        }
    }
    let reader = ImageReader::new(Cursor::new(&asset.bytes))
        .with_guessed_format()
        .map_err(|_| SiteError::Evidence)?;
    let mut decoder = reader.into_decoder().map_err(|_| SiteError::Evidence)?;
    let widths: &[u32] = match role {
        ImageRole::Content => &[480, 960, 1440, 1920],
        ImageRole::Logo => &[32, 64, 96],
        ImageRole::Hero => &[192, 384, 576, 768, 1152],
    };
    let width = decoder.dimensions().0;
    if width <= widths[0] {
        return Ok(Vec::new());
    }
    // Keep the original's orientation and color profile rather than silently changing its appearance.
    if decoder.orientation().map_err(|_| SiteError::Evidence)?
        != image::metadata::Orientation::NoTransforms
    {
        return Ok(Vec::new());
    }
    let profile = decoder.icc_profile().map_err(|_| SiteError::Evidence)?;
    let image = DynamicImage::from_decoder(decoder).map_err(|_| SiteError::Evidence)?;
    let mut result = Vec::new();
    for &width in widths.iter().filter(|&&width| width < image.width()) {
        let height = ((u64::from(image.height()) * u64::from(width)) / u64::from(image.width()))
            .max(1) as u32;
        let resized = image.resize_exact(width, height, image::imageops::FilterType::Lanczos3);
        let mut bytes = Vec::new();
        let extension = if asset.media_type == "image/png" {
            let mut encoder = image::codecs::png::PngEncoder::new(&mut bytes);
            if let Some(profile) = &profile {
                encoder
                    .set_icc_profile(profile.clone())
                    .map_err(|_| SiteError::Evidence)?;
            }
            resized
                .write_with_encoder(encoder)
                .map_err(|_| SiteError::Evidence)?;
            "png"
        } else {
            let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90);
            if let Some(profile) = &profile {
                encoder
                    .set_icc_profile(profile.clone())
                    .map_err(|_| SiteError::Evidence)?;
            }
            resized
                .to_rgb8()
                .write_with_encoder(encoder)
                .map_err(|_| SiteError::Evidence)?;
            "jpg"
        };
        if bytes.len() >= asset.bytes.len() {
            continue;
        }
        result.push(Variant {
            width,
            path: format!("assets/{}.{}", fingerprint_bytes(&bytes).value, extension),
            file: RenderedFile {
                bytes,
                media_type: asset.media_type.clone(),
            },
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(bytes: Vec<u8>, media_type: &str) -> ContentAsset {
        ContentAsset {
            fingerprint: fingerprint_bytes(&bytes),
            bytes,
            media_type: media_type.into(),
        }
    }

    #[test]
    fn animated_pngs_keep_all_frames_in_the_original() {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 600, 2);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_animated(2, 0).unwrap();
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&vec![0; 600 * 2 * 4]).unwrap();
            writer.write_image_data(&vec![255; 600 * 2 * 4]).unwrap();
            writer.finish().unwrap();
        }
        let asset = asset(bytes, "image/png");
        for role in [ImageRole::Content, ImageRole::Logo, ImageRole::Hero] {
            assert!(variants(&asset, role).unwrap().is_empty());
        }
    }

    #[test]
    fn exif_rotated_jpegs_keep_their_orientation_in_the_original() {
        let mut bytes = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut bytes);
        encoder
            .set_exif_metadata(vec![
                b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0,
                0, 0,
            ])
            .unwrap();
        image::RgbImage::new(600, 300)
            .write_with_encoder(encoder)
            .unwrap();
        let asset = asset(bytes, "image/jpeg");
        let mut decoder = ImageReader::new(Cursor::new(&asset.bytes))
            .with_guessed_format()
            .unwrap()
            .into_decoder()
            .unwrap();
        assert_eq!(
            decoder.orientation().unwrap(),
            image::metadata::Orientation::Rotate90
        );
        for role in [ImageRole::Content, ImageRole::Logo, ImageRole::Hero] {
            assert!(variants(&asset, role).unwrap().is_empty());
        }
    }
}
