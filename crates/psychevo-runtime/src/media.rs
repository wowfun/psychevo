use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use image::GenericImageView;
use reqwest::Url;
use uuid::Uuid;

use crate::error::{Error, Result};

pub const MAX_IMAGE_SOURCE_BYTES: u64 = 50 * 1024 * 1024;
pub const PSYCHEVO_MEDIA_SCHEME: &str = "psychevo-media";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageMimeKind {
    Png,
    Jpeg,
    Webp,
    Gif,
    Bmp,
    Avif,
}

impl ImageMimeKind {
    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Gif => "image/gif",
            Self::Bmp => "image/bmp",
            Self::Avif => "image/avif",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
            Self::Gif => "gif",
            Self::Bmp => "bmp",
            Self::Avif => "avif",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedImageSource {
    pub source: String,
    pub display_source: String,
    pub agent_visible_source: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
    pub data_url: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct GeneratedImageArtifact {
    pub artifact_id: String,
    pub saved_path: PathBuf,
    pub display_url: String,
    pub agent_visible_source: String,
    pub mime_type: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub size_bytes: u64,
}

pub async fn resolve_explicit_image_source(
    source: &str,
    cwd: &Path,
    home: &Path,
) -> Result<ResolvedImageSource> {
    let source = strip_wrapping_quotes(source.trim());
    if source.is_empty() {
        return Err(Error::Message("image source is empty".to_string()));
    }
    if source.starts_with("data:image/") {
        return resolve_data_image_source(source);
    }
    if let Some(artifact_id) = source.strip_prefix("psychevo-media://") {
        return resolve_media_image_source(source, artifact_id, home);
    }
    if source.starts_with("file://") {
        let path = Url::parse(source)
            .map_err(|err| Error::Message(format!("invalid file image URL: {err}")))?
            .to_file_path()
            .map_err(|_| Error::Message(format!("invalid file image URL: {source}")))?;
        return resolve_local_image_source(source, &path);
    }
    if source.starts_with("http://") || source.starts_with("https://") {
        return resolve_remote_image_source(source).await;
    }
    let path = resolve_local_path(source, cwd);
    resolve_local_image_source(source, &path)
}

pub fn media_root(home: &Path) -> PathBuf {
    home.join("media")
}

pub fn generated_media_dir(home: &Path) -> PathBuf {
    media_root(home).join("generated")
}

pub fn media_display_url(artifact_id: &str) -> String {
    format!("/_gateway/media/{artifact_id}")
}

pub fn media_agent_visible_source(artifact_id: &str) -> String {
    format!("psychevo-media://{artifact_id}")
}

pub fn validate_media_artifact_id(artifact_id: &str) -> Result<()> {
    if artifact_id.is_empty()
        || artifact_id.len() > 96
        || !artifact_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return Err(Error::Message("invalid media artifact id".to_string()));
    }
    Ok(())
}

pub fn media_artifact_path(home: &Path, artifact_id: &str) -> Result<PathBuf> {
    validate_media_artifact_id(artifact_id)?;
    let dir = generated_media_dir(home);
    for extension in ["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif"] {
        let path = dir.join(format!("{artifact_id}.{extension}"));
        if path.is_file() {
            return Ok(path);
        }
    }
    Err(Error::Message(format!(
        "media artifact not found: {artifact_id}"
    )))
}

pub fn read_media_artifact(home: &Path, artifact_id: &str) -> Result<ResolvedImageSource> {
    let path = media_artifact_path(home, artifact_id)?;
    resolve_local_image_source(&media_agent_visible_source(artifact_id), &path)
}

pub fn write_generated_image_artifact(
    home: &Path,
    bytes: &[u8],
    requested_mime_type: &str,
) -> Result<GeneratedImageArtifact> {
    let kind = sniff_image_mime(bytes).ok_or_else(|| {
        Error::Message("generated image output did not match a supported image type".to_string())
    })?;
    let mime_type = normalized_image_mime(requested_mime_type)
        .filter(|mime| *mime == kind.mime_type())
        .unwrap_or_else(|| kind.mime_type().to_string());
    if bytes.len() as u64 > MAX_IMAGE_SOURCE_BYTES {
        return Err(Error::Message(format!(
            "generated image exceeds {MAX_IMAGE_SOURCE_BYTES} bytes"
        )));
    }
    let artifact_id = format!("img_{}", Uuid::now_v7().as_simple());
    let dir = generated_media_dir(home);
    fs::create_dir_all(&dir)?;
    let saved_path = dir.join(format!("{artifact_id}.{}", kind.extension()));
    fs::write(&saved_path, bytes)?;
    let (width, height) = image_dimensions(bytes);
    Ok(GeneratedImageArtifact {
        artifact_id: artifact_id.clone(),
        saved_path,
        display_url: media_display_url(&artifact_id),
        agent_visible_source: media_agent_visible_source(&artifact_id),
        mime_type,
        width,
        height,
        size_bytes: bytes.len() as u64,
    })
}

pub fn sniff_image_mime(bytes: &[u8]) -> Option<ImageMimeKind> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(ImageMimeKind::Png);
    }
    if bytes.len() >= 3 && bytes[0..3] == [0xff, 0xd8, 0xff] {
        return Some(ImageMimeKind::Jpeg);
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some(ImageMimeKind::Gif);
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some(ImageMimeKind::Webp);
    }
    if bytes.starts_with(b"BM") {
        return Some(ImageMimeKind::Bmp);
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        let brand = &bytes[8..12];
        if matches!(brand, b"avif" | b"avis") || bytes.windows(4).any(|chunk| chunk == b"avif") {
            return Some(ImageMimeKind::Avif);
        }
    }
    None
}

pub fn normalized_image_mime(value: &str) -> Option<String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "image/png" => Some("image/png".to_string()),
        "image/jpeg" | "image/jpg" => Some("image/jpeg".to_string()),
        "image/webp" => Some("image/webp".to_string()),
        "image/gif" => Some("image/gif".to_string()),
        "image/bmp" => Some("image/bmp".to_string()),
        "image/avif" => Some("image/avif".to_string()),
        _ => None,
    }
}

fn resolve_local_image_source(source: &str, path: &Path) -> Result<ResolvedImageSource> {
    let metadata = fs::metadata(path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            Error::Message(format!("image path does not exist: {}", path.display()))
        } else {
            Error::Message(format!(
                "image path is not readable: {}: {err}",
                path.display()
            ))
        }
    })?;
    if !metadata.is_file() {
        return Err(Error::Message(format!(
            "image path is not a file: {}",
            path.display()
        )));
    }
    if metadata.len() > MAX_IMAGE_SOURCE_BYTES {
        return Err(Error::Message(format!(
            "image file is too large: {} exceeds {} bytes",
            path.display(),
            MAX_IMAGE_SOURCE_BYTES
        )));
    }
    let bytes = fs::read(path)?;
    resolved_from_bytes(
        source,
        &path.display().to_string(),
        &path.display().to_string(),
        bytes,
    )
}

fn resolve_data_image_source(source: &str) -> Result<ResolvedImageSource> {
    let (mime, data) = parse_data_image(source)?;
    let bytes = BASE64_STANDARD
        .decode(data.as_bytes())
        .map_err(|err| Error::Message(format!("invalid data image base64: {err}")))?;
    let resolved = resolved_from_bytes(source, "data:image", source, bytes)?;
    if resolved.mime_type != mime {
        return Err(Error::Message(format!(
            "data image MIME mismatch: declared {mime}, detected {}",
            resolved.mime_type
        )));
    }
    Ok(resolved)
}

fn resolve_media_image_source(
    source: &str,
    artifact_id: &str,
    home: &Path,
) -> Result<ResolvedImageSource> {
    validate_media_artifact_id(artifact_id)?;
    let mut resolved = read_media_artifact(home, artifact_id)?;
    resolved.source = source.to_string();
    resolved.display_source = media_display_url(artifact_id);
    resolved.agent_visible_source = media_agent_visible_source(artifact_id);
    Ok(resolved)
}

async fn resolve_remote_image_source(source: &str) -> Result<ResolvedImageSource> {
    let url =
        Url::parse(source).map_err(|err| Error::Message(format!("invalid image URL: {err}")))?;
    reject_unsafe_remote_image_url(&url)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(3))
        .timeout(Duration::from_secs(10))
        .build()?;
    let response = client.get(url.clone()).send().await?;
    reject_unsafe_remote_image_url(response.url())?;
    if !response.status().is_success() {
        return Err(Error::Message(format!(
            "image URL returned HTTP {}",
            response.status()
        )));
    }
    if let Some(length) = response.content_length()
        && length > MAX_IMAGE_SOURCE_BYTES
    {
        return Err(Error::Message(format!(
            "remote image is too large: {length} exceeds {MAX_IMAGE_SOURCE_BYTES} bytes"
        )));
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .and_then(normalized_image_mime)
        .ok_or_else(|| {
            Error::Message("remote URL did not return an image MIME type".to_string())
        })?;
    let final_url = response.url().to_string();
    let bytes = response.bytes().await?.to_vec();
    if bytes.len() as u64 > MAX_IMAGE_SOURCE_BYTES {
        return Err(Error::Message(format!(
            "remote image exceeds {MAX_IMAGE_SOURCE_BYTES} bytes"
        )));
    }
    let resolved = resolved_from_bytes(source, &final_url, &final_url, bytes)?;
    if resolved.mime_type != content_type {
        return Err(Error::Message(format!(
            "remote image MIME mismatch: declared {content_type}, detected {}",
            resolved.mime_type
        )));
    }
    Ok(resolved)
}

fn resolved_from_bytes(
    source: &str,
    display_source: &str,
    agent_visible_source: &str,
    bytes: Vec<u8>,
) -> Result<ResolvedImageSource> {
    let kind = sniff_image_mime(&bytes).ok_or_else(|| {
        Error::Message("image data did not match a supported image type".to_string())
    })?;
    let mime_type = kind.mime_type().to_string();
    let (width, height) = image_dimensions(&bytes);
    let data_url = format!("data:{mime_type};base64,{}", BASE64_STANDARD.encode(&bytes));
    Ok(ResolvedImageSource {
        source: source.to_string(),
        display_source: display_source.to_string(),
        agent_visible_source: agent_visible_source.to_string(),
        mime_type,
        size_bytes: bytes.len() as u64,
        bytes,
        data_url,
        width,
        height,
    })
}

fn parse_data_image(source: &str) -> Result<(String, &str)> {
    let (header, data) = source
        .split_once(',')
        .ok_or_else(|| Error::Message("data image URL is missing base64 data".to_string()))?;
    if !header.to_ascii_lowercase().ends_with(";base64") {
        return Err(Error::Message(
            "data image URL must use base64 encoding".to_string(),
        ));
    }
    let raw_mime = header
        .strip_prefix("data:")
        .ok_or_else(|| Error::Message("unsupported data image MIME type".to_string()))?;
    let mime = raw_mime
        .get(..raw_mime.len().saturating_sub(";base64".len()))
        .and_then(normalized_image_mime)
        .ok_or_else(|| Error::Message("unsupported data image MIME type".to_string()))?;
    Ok((mime, data))
}

fn reject_unsafe_remote_image_url(url: &Url) -> Result<()> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(Error::Message(
            "remote image URL must use http or https".to_string(),
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| Error::Message("remote image URL is missing a host".to_string()))?;
    if host.eq_ignore_ascii_case("localhost") || host.ends_with(".localhost") {
        return Err(Error::Message(
            "remote image URL host is not allowed".to_string(),
        ));
    }
    if let Ok(ip) = host.parse::<IpAddr>()
        && unsafe_ip_address(ip)
    {
        return Err(Error::Message(
            "remote image URL host is not allowed".to_string(),
        ));
    }
    Ok(())
}

fn unsafe_ip_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified()
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || ip.is_multicast()
        }
    }
}

fn image_dimensions(bytes: &[u8]) -> (Option<u32>, Option<u32>) {
    image::load_from_memory(bytes)
        .ok()
        .map(|image| {
            let (width, height) = image.dimensions();
            (Some(width), Some(height))
        })
        .unwrap_or((None, None))
}

fn resolve_local_path(source: &str, cwd: &Path) -> PathBuf {
    let expanded = expand_home_path(source);
    if expanded.is_absolute() {
        expanded
    } else {
        cwd.join(expanded)
    }
}

fn expand_home_path(source: &str) -> PathBuf {
    if let Some(rest) = source.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(source)
}

fn strip_wrapping_quotes(value: &str) -> &str {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
        {
            return &value[1..value.len() - 1];
        }
    }
    value
}
