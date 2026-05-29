#[allow(unused_imports)]
use crate::*;

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use std::io::Read;

const VIEW_TEXT_PREVIEW_BYTES: usize = 1024 * 1024;
const TRAJECTORY_DATA_PREVIEW_CHARS: usize = 2048;
const ATIF_CONTENT_PREVIEW_CHARS: usize = 16 * 1024;
const SMALL_IMAGE_INLINE_BYTES: u64 = 96 * 1024;

mod files;
mod html;
mod matrix;
mod query;
mod redaction;
mod trajectory;
mod trial;

pub(crate) use files::*;
pub(crate) use html::*;
pub(crate) use matrix::*;
pub(crate) use query::*;
pub(crate) use redaction::*;
pub(crate) use trajectory::*;
pub(crate) use trial::*;
