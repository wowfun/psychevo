use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageFormat};

use super::{JPEG_QUALITIES, MAX_IMAGE_BASE64_BYTES, MAX_IMAGE_DIMENSION, MAX_LOCAL_IMAGE_BYTES};

pub(crate) fn local_image_data_url(path: &str) -> std::result::Result<String, String> {
    let path = Path::new(path);
    let format = LocalImageFormat::from_path(path).ok_or_else(|| {
        "unsupported image type; expected png, jpg, jpeg, webp, gif, bmp, or avif".to_string()
    })?;
    let metadata = fs::metadata(path).map_err(|err| err.to_string())?;
    if !metadata.is_file() {
        return Err("image path is not a file".to_string());
    }
    if metadata.len() > MAX_LOCAL_IMAGE_BYTES {
        return Err(format!(
            "image file exceeds {} bytes",
            MAX_LOCAL_IMAGE_BYTES
        ));
    }
    let data = fs::read(path).map_err(|err| err.to_string())?;
    let original_base64 = BASE64_STANDARD.encode(&data);
    let image = match decode_local_image(format, &data) {
        Ok(image) => image,
        Err(_err)
            if format == LocalImageFormat::Avif
                && original_base64.len() <= MAX_IMAGE_BASE64_BYTES =>
        {
            return Ok(format!("data:image/avif;base64,{original_base64}"));
        }
        Err(err) => return Err(err),
    };
    let (width, height) = image.dimensions();
    if format.preserve_original()
        && width <= MAX_IMAGE_DIMENSION
        && height <= MAX_IMAGE_DIMENSION
        && original_base64.len() <= MAX_IMAGE_BASE64_BYTES
    {
        return Ok(format!(
            "data:{};base64,{original_base64}",
            format.mime_type()
        ));
    }

    normalized_image_data_url(&image).ok_or_else(|| {
        format!(
            "image {width}x{height} exceeds normalized payload limit of {} base64 bytes",
            MAX_IMAGE_BASE64_BYTES
        )
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalImageFormat {
    Png,
    Jpeg,
    Webp,
    Gif,
    Bmp,
    Avif,
}

impl LocalImageFormat {
    pub(crate) fn from_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "webp" => Some(Self::Webp),
            "gif" => Some(Self::Gif),
            "bmp" => Some(Self::Bmp),
            "avif" => Some(Self::Avif),
            _ => None,
        }
    }

    pub(crate) fn image_format(self) -> Option<ImageFormat> {
        match self {
            Self::Png => Some(ImageFormat::Png),
            Self::Jpeg => Some(ImageFormat::Jpeg),
            Self::Webp => Some(ImageFormat::WebP),
            Self::Gif => Some(ImageFormat::Gif),
            Self::Bmp => Some(ImageFormat::Bmp),
            Self::Avif => None,
        }
    }

    pub(crate) fn mime_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Gif => "image/gif",
            Self::Bmp => "image/bmp",
            Self::Avif => "image/avif",
        }
    }

    pub(crate) fn preserve_original(self) -> bool {
        matches!(self, Self::Png | Self::Jpeg | Self::Webp | Self::Gif)
    }
}

pub(crate) fn decode_local_image(
    format: LocalImageFormat,
    data: &[u8],
) -> std::result::Result<DynamicImage, String> {
    if format == LocalImageFormat::Avif {
        return decode_avif_image(data);
    }
    image::load_from_memory_with_format(
        data,
        format
            .image_format()
            .expect("non-AVIF local image format has image crate decoder"),
    )
    .map_err(|err| format!("image could not be decoded: {err}"))
}

pub(crate) fn decode_avif_image(data: &[u8]) -> std::result::Result<DynamicImage, String> {
    let mut child = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            "pipe:0",
            "-frames:v",
            "1",
            "-f",
            "image2pipe",
            "-vcodec",
            "png",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("AVIF decoder unavailable: {err}"))?;
    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| "AVIF decoder stdin unavailable".to_string())?;
        stdin
            .write_all(data)
            .map_err(|err| format!("failed to send AVIF data to decoder: {err}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|err| format!("AVIF decoder failed: {err}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "AVIF image could not be decoded: {}",
            stderr.trim()
        ));
    }
    image::load_from_memory_with_format(&output.stdout, ImageFormat::Png)
        .map_err(|err| format!("AVIF image could not be decoded: {err}"))
}

pub(crate) fn normalized_image_data_url(image: &DynamicImage) -> Option<String> {
    let (width, height) = image.dimensions();
    for (candidate_width, candidate_height) in candidate_image_sizes(width, height) {
        let candidate = if candidate_width == width && candidate_height == height {
            image.clone()
        } else {
            image.resize_exact(candidate_width, candidate_height, FilterType::Lanczos3)
        };
        if let Ok(png) = encode_png(&candidate)
            && let Some(data_url) = bounded_data_url("image/png", &png)
        {
            return Some(data_url);
        }
        for quality in JPEG_QUALITIES {
            if let Ok(jpeg) = encode_jpeg(&candidate, quality)
                && let Some(data_url) = bounded_data_url("image/jpeg", &jpeg)
            {
                return Some(data_url);
            }
        }
    }
    None
}

pub(crate) fn candidate_image_sizes(width: u32, height: u32) -> Vec<(u32, u32)> {
    let scale = (MAX_IMAGE_DIMENSION as f64 / width.max(1) as f64)
        .min(MAX_IMAGE_DIMENSION as f64 / height.max(1) as f64)
        .min(1.0);
    let mut current = (
        ((width as f64 * scale).round() as u32).max(1),
        ((height as f64 * scale).round() as u32).max(1),
    );
    let mut sizes = Vec::new();
    for _ in 0..32 {
        if !sizes.contains(&current) {
            sizes.push(current);
        }
        if current == (1, 1) {
            break;
        }
        current = (
            if current.0 == 1 {
                1
            } else {
                ((current.0 as f64) * 0.75).floor().max(1.0) as u32
            },
            if current.1 == 1 {
                1
            } else {
                ((current.1 as f64) * 0.75).floor().max(1.0) as u32
            },
        );
    }
    sizes
}

pub(crate) fn encode_png(image: &DynamicImage) -> std::result::Result<Vec<u8>, image::ImageError> {
    let mut cursor = Cursor::new(Vec::new());
    image.write_to(&mut cursor, ImageFormat::Png)?;
    Ok(cursor.into_inner())
}

pub(crate) fn encode_jpeg(
    image: &DynamicImage,
    quality: u8,
) -> std::result::Result<Vec<u8>, image::ImageError> {
    let rgb = image.to_rgb8();
    let (width, height) = rgb.dimensions();
    let mut output = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut output, quality);
    encoder.encode(&rgb, width, height, image::ColorType::Rgb8.into())?;
    Ok(output)
}

pub(crate) fn bounded_data_url(mime_type: &str, data: &[u8]) -> Option<String> {
    let encoded = BASE64_STANDARD.encode(data);
    (encoded.len() <= MAX_IMAGE_BASE64_BYTES).then(|| format!("data:{mime_type};base64,{encoded}"))
}
