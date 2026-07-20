use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::body::{Body, Bytes};
use axum::extract::{Path as AxumPath, State};
use axum::http::header::{
    ACCEPT_RANGES, ACCESS_CONTROL_ALLOW_HEADERS, ACCESS_CONTROL_ALLOW_METHODS,
    ACCESS_CONTROL_ALLOW_ORIGIN, ACCESS_CONTROL_EXPOSE_HEADERS, CACHE_CONTROL, CONTENT_LENGTH,
    CONTENT_RANGE, CONTENT_SECURITY_POLICY, CONTENT_TYPE, ETAG, HOST, HeaderName, ORIGIN, RANGE,
    REFERRER_POLICY, VARY,
};
use axum::http::{HeaderMap, HeaderValue, Method, Response, StatusCode};
use futures::stream;
use psychevo_gateway_protocol as wire;
use psychevo_runtime::Error;
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use super::workspace::{
    normalized_workspace_path_identity, path_from_root, resolve_workspace_relative_path,
    workspace_file_read_result_from_file,
};
use super::{ResolvedScope, WebState};

const PREVIEW_IDLE_TTL_MS: i64 = 30 * 60 * 1_000;
const PREVIEW_ABSOLUTE_TTL_MS: i64 = 8 * 60 * 60 * 1_000;
const PREVIEW_TOMBSTONE_TTL_MS: i64 = 8 * 60 * 60 * 1_000;
const DESKTOP_WORKSPACE_PREVIEW_ORIGINS: &[&str] = &[
    "http://127.0.0.1:5175",
    "http://tauri.localhost",
    "tauri://localhost",
];
const MAX_PREVIEW_LEASES: usize = 4_096;
const PREVIEW_STREAM_CHUNK_BYTES: usize = 64 * 1_024;

type PreviewClock = Arc<dyn Fn() -> i64 + Send + Sync>;
#[cfg(test)]
type PreviewBeforeOpenHook = Box<dyn FnOnce() + Send>;

#[derive(Clone)]
pub(super) struct WorkspacePreviewLeaseStore {
    inner: Arc<Mutex<HashMap<String, WorkspacePreviewLease>>>,
    clock: Arc<Mutex<PreviewClock>>,
    #[cfg(test)]
    before_open: Arc<Mutex<Option<PreviewBeforeOpenHook>>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum WorkspacePreviewFileIdentity {
    #[cfg(unix)]
    Unix { device: u64, inode: u64 },
    #[cfg(windows)]
    Windows {
        volume_serial_number: u64,
        file_id: [u8; 16],
    },
    #[cfg(not(any(unix, windows)))]
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspacePreviewFileChangeMarker {
    #[cfg(unix)]
    Unix {
        ctime_seconds: i64,
        ctime_nanoseconds: i64,
    },
    #[cfg(windows)]
    Windows { change_time: i64 },
    #[cfg(not(any(unix, windows)))]
    Unsupported,
}

#[derive(Debug, Clone)]
pub(super) struct WorkspacePreviewLease {
    pub resource_id: String,
    pub workspace_root: PathBuf,
    pub relative_path: String,
    pub canonical_path: PathBuf,
    file_identity: WorkspacePreviewFileIdentity,
    file_change_marker: WorkspacePreviewFileChangeMarker,
    pub size_bytes: u64,
    pub media_type: String,
    pub modified_ns: u128,
    pub created_at_ms: i64,
    pub last_accessed_at_ms: i64,
    pub released_at_ms: Option<i64>,
}

struct WorkspacePreviewLeaseBinding {
    workspace_root: PathBuf,
    relative_path: String,
    canonical_path: PathBuf,
    file_identity: WorkspacePreviewFileIdentity,
    file_change_marker: WorkspacePreviewFileChangeMarker,
    size_bytes: u64,
    media_type: String,
    modified_ns: u128,
}

impl WorkspacePreviewLease {
    pub(super) fn expires_at_ms(&self) -> i64 {
        (self.last_accessed_at_ms + PREVIEW_IDLE_TTL_MS)
            .min(self.created_at_ms + PREVIEW_ABSOLUTE_TTL_MS)
    }
}

impl WorkspacePreviewLeaseStore {
    pub(super) fn production() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            clock: Arc::new(Mutex::new(Arc::new(system_now_ms))),
            #[cfg(test)]
            before_open: Arc::new(Mutex::new(None)),
        }
    }

    fn now_ms(&self) -> i64 {
        let clock = self
            .clock
            .lock()
            .expect("workspace preview clock poisoned")
            .clone();
        clock()
    }

    #[cfg(test)]
    pub(super) fn set_clock_for_tests(&self, now_ms: Arc<std::sync::atomic::AtomicI64>) {
        *self.clock.lock().expect("workspace preview clock poisoned") =
            Arc::new(move || now_ms.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[cfg(test)]
    pub(super) fn set_before_open_for_tests(&self, hook: impl FnOnce() + Send + 'static) {
        *self
            .before_open
            .lock()
            .expect("workspace preview open hook poisoned") = Some(Box::new(hook));
    }

    fn insert(
        &self,
        binding: WorkspacePreviewLeaseBinding,
    ) -> Result<WorkspacePreviewLease, Error> {
        let mut random = [0_u8; 32];
        getrandom::fill(&mut random)
            .map_err(|error| Error::Message(format!("preview lease randomness failed: {error}")))?;
        let resource_id = random
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let now_ms = self.now_ms();
        let lease = WorkspacePreviewLease {
            resource_id: resource_id.clone(),
            workspace_root: binding.workspace_root,
            relative_path: binding.relative_path,
            canonical_path: binding.canonical_path,
            file_identity: binding.file_identity,
            file_change_marker: binding.file_change_marker,
            size_bytes: binding.size_bytes,
            media_type: binding.media_type,
            modified_ns: binding.modified_ns,
            created_at_ms: now_ms,
            last_accessed_at_ms: now_ms,
            released_at_ms: None,
        };
        let mut leases = self
            .inner
            .lock()
            .expect("workspace preview leases poisoned");
        retain_bounded_leases(&mut leases, now_ms);
        leases.insert(resource_id, lease.clone());
        Ok(lease)
    }

    fn open(&self, resource_id: &str) -> Result<(WorkspacePreviewLease, File), PreviewLeaseError> {
        let lease = self.lookup(resource_id)?;
        #[cfg(test)]
        if let Some(hook) = self
            .before_open
            .lock()
            .expect("workspace preview open hook poisoned")
            .take()
        {
            hook();
        }
        let file = open_workspace_preview_file(&lease)?;
        Ok((lease, file))
    }

    fn release(&self, resource_id: &str) -> bool {
        let now_ms = self.now_ms();
        let mut leases = self
            .inner
            .lock()
            .expect("workspace preview leases poisoned");
        let Some(lease) = leases.get_mut(resource_id) else {
            return false;
        };
        if lease.released_at_ms.is_some() || lease.expires_at_ms() <= now_ms {
            return false;
        }
        lease.released_at_ms = Some(now_ms);
        true
    }

    fn lookup(&self, resource_id: &str) -> Result<WorkspacePreviewLease, PreviewLeaseError> {
        if !valid_resource_id(resource_id) {
            return Err(PreviewLeaseError::Missing);
        }
        let now_ms = self.now_ms();
        let lease = {
            let leases = self
                .inner
                .lock()
                .expect("workspace preview leases poisoned");
            let Some(lease) = leases.get(resource_id) else {
                return Err(PreviewLeaseError::Missing);
            };
            if lease.released_at_ms.is_some() || lease.expires_at_ms() <= now_ms {
                return Err(PreviewLeaseError::Gone);
            }
            lease.clone()
        };
        let canonical =
            resolve_workspace_relative_path(&lease.workspace_root, &lease.relative_path)
                .map_err(|_| PreviewLeaseError::Changed)?;
        if canonical != lease.canonical_path {
            return Err(PreviewLeaseError::Changed);
        }
        let metadata = std::fs::metadata(&canonical).map_err(|_| PreviewLeaseError::Changed)?;
        if !metadata.is_file()
            || metadata.len() != lease.size_bytes
            || modified_ns(&metadata) != lease.modified_ns
        {
            return Err(PreviewLeaseError::Changed);
        }
        Ok(lease)
    }

    fn refresh(&self, resource_id: &str) -> Result<(), PreviewLeaseError> {
        let now_ms = self.now_ms();
        let mut leases = self
            .inner
            .lock()
            .expect("workspace preview leases poisoned");
        let Some(lease) = leases.get_mut(resource_id) else {
            return Err(PreviewLeaseError::Missing);
        };
        if lease.released_at_ms.is_some() || lease.expires_at_ms() <= now_ms {
            return Err(PreviewLeaseError::Gone);
        }
        lease.last_accessed_at_ms = now_ms;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
enum PreviewLeaseError {
    Missing,
    Gone,
    Changed,
    Io,
}

#[derive(Debug, Clone, Copy)]
struct PreviewByteRange {
    start: u64,
    end: u64,
}

fn workspace_preview_file_identity(file: &File) -> std::io::Result<WorkspacePreviewFileIdentity> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let metadata = file.metadata()?;
        if metadata.ino() == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "workspace preview file has no stable inode identity",
            ));
        }
        Ok(WorkspacePreviewFileIdentity::Unix {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    #[cfg(windows)]
    {
        use std::mem::size_of;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
        };

        let mut info = FILE_ID_INFO::default();
        let succeeded = unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle() as _,
                FileIdInfo,
                (&mut info as *mut FILE_ID_INFO).cast(),
                size_of::<FILE_ID_INFO>() as u32,
            )
        };
        if succeeded == 0 {
            return Err(std::io::Error::last_os_error());
        }
        if info.FileId.Identifier == [0; 16] {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "workspace preview file has no stable Windows file identity",
            ));
        }
        Ok(WorkspacePreviewFileIdentity::Windows {
            volume_serial_number: info.VolumeSerialNumber,
            file_id: info.FileId.Identifier,
        })
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = file;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "workspace preview file identity is unsupported on this platform",
        ))
    }
}

fn workspace_preview_file_change_marker(
    file: &File,
) -> std::io::Result<WorkspacePreviewFileChangeMarker> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        let metadata = file.metadata()?;
        Ok(WorkspacePreviewFileChangeMarker::Unix {
            ctime_seconds: metadata.ctime(),
            ctime_nanoseconds: metadata.ctime_nsec(),
        })
    }

    #[cfg(windows)]
    {
        use std::mem::size_of;
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_BASIC_INFO, FileBasicInfo, GetFileInformationByHandleEx,
        };

        let mut info = FILE_BASIC_INFO::default();
        let succeeded = unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle() as _,
                FileBasicInfo,
                (&mut info as *mut FILE_BASIC_INFO).cast(),
                size_of::<FILE_BASIC_INFO>() as u32,
            )
        };
        if succeeded == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(WorkspacePreviewFileChangeMarker::Windows {
            change_time: info.ChangeTime,
        })
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = file;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "workspace preview file change markers are unsupported on this platform",
        ))
    }
}

#[cfg(windows)]
fn workspace_preview_handle_is_contained(
    file: &File,
    workspace_root: &Path,
) -> std::io::Result<bool> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_NAME_NORMALIZED, GetFinalPathNameByHandleW, VOLUME_NAME_DOS,
    };

    let handle = file.as_raw_handle() as _;
    let required = unsafe {
        GetFinalPathNameByHandleW(
            handle,
            std::ptr::null_mut(),
            0,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    };
    if required == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut buffer = vec![0_u16; required as usize + 1];
    let written = unsafe {
        GetFinalPathNameByHandleW(
            handle,
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    };
    if written == 0 {
        return Err(std::io::Error::last_os_error());
    }
    if written as usize >= buffer.len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "workspace preview handle path changed while resolving",
        ));
    }
    let handle_path = PathBuf::from(OsString::from_wide(&buffer[..written as usize]));
    let root = normalized_workspace_path_identity(workspace_root);
    let target = normalized_workspace_path_identity(&handle_path);
    Ok(target.starts_with(root))
}

#[cfg(not(windows))]
fn workspace_preview_handle_is_contained(
    _file: &File,
    _workspace_root: &Path,
) -> std::io::Result<bool> {
    Ok(true)
}

fn open_workspace_preview_file(lease: &WorkspacePreviewLease) -> Result<File, PreviewLeaseError> {
    let file = File::open(&lease.canonical_path).map_err(|_| PreviewLeaseError::Changed)?;
    if !workspace_preview_handle_is_contained(&file, &lease.workspace_root)
        .map_err(|_| PreviewLeaseError::Changed)?
        || workspace_preview_file_identity(&file).map_err(|_| PreviewLeaseError::Changed)?
            != lease.file_identity
    {
        return Err(PreviewLeaseError::Changed);
    }
    let before = file.metadata().map_err(|_| PreviewLeaseError::Changed)?;
    if !before.is_file()
        || before.len() != lease.size_bytes
        || modified_ns(&before) != lease.modified_ns
        || workspace_preview_file_change_marker(&file).map_err(|_| PreviewLeaseError::Changed)?
            != lease.file_change_marker
    {
        return Err(PreviewLeaseError::Changed);
    }
    // Range validation must remain O(1): identity plus the OS change marker
    // detects replacement and in-place writes without scanning media content.
    let after = file.metadata().map_err(|_| PreviewLeaseError::Changed)?;
    if !after.is_file()
        || after.len() != lease.size_bytes
        || modified_ns(&after) != lease.modified_ns
        || workspace_preview_file_identity(&file).map_err(|_| PreviewLeaseError::Changed)?
            != lease.file_identity
        || workspace_preview_file_change_marker(&file).map_err(|_| PreviewLeaseError::Changed)?
            != lease.file_change_marker
    {
        return Err(PreviewLeaseError::Changed);
    }
    Ok(file)
}

pub(super) fn workspace_file_preview_open_value(
    state: &WebState,
    scope: &ResolvedScope,
    path: &str,
) -> psychevo_runtime::Result<Value> {
    let workspace_root = normalized_workspace_path_identity(&std::fs::canonicalize(&scope.cwd)?);
    let canonical_path = resolve_workspace_relative_path(&workspace_root, path)?;
    let mut file = File::open(&canonical_path)?;
    let initial_metadata = file.metadata()?;
    if !initial_metadata.is_file() {
        return Err(Error::Message(
            "workspace preview target must be a regular file".to_string(),
        ));
    }
    if !workspace_preview_handle_is_contained(&file, &workspace_root)? {
        return Err(Error::Message(
            "workspace preview target is outside the workspace".to_string(),
        ));
    }
    let file_identity = workspace_preview_file_identity(&file)?;
    let initial_file_change_marker = workspace_preview_file_change_marker(&file)?;
    let display_path = path_from_root(&workspace_root, &canonical_path).ok_or_else(|| {
        Error::Message("workspace preview target is outside the workspace".to_string())
    })?;
    let read = workspace_file_read_result_from_file(&mut file, display_path);
    if let Some(message) = read.unreadable.as_ref() {
        return Err(Error::Message(message.clone()));
    }
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || !workspace_preview_handle_is_contained(&file, &workspace_root)?
        || workspace_preview_file_identity(&file)? != file_identity
        || workspace_preview_file_change_marker(&file)? != initial_file_change_marker
        || metadata.len() != initial_metadata.len()
        || modified_ns(&metadata) != modified_ns(&initial_metadata)
        || read.size_bytes as u64 != metadata.len()
    {
        return Err(Error::Message(
            "workspace preview target changed while opening".to_string(),
        ));
    }
    let current_path = resolve_workspace_relative_path(&workspace_root, &read.path)?;
    let current_file = File::open(&current_path)?;
    let current_metadata = current_file.metadata()?;
    if current_path != canonical_path
        || !workspace_preview_handle_is_contained(&current_file, &workspace_root)?
        || workspace_preview_file_identity(&current_file)? != file_identity
        || workspace_preview_file_change_marker(&current_file)? != initial_file_change_marker
        || !current_metadata.is_file()
        || current_metadata.len() != metadata.len()
        || modified_ns(&current_metadata) != modified_ns(&metadata)
    {
        return Err(Error::Message(
            "workspace preview target changed while opening".to_string(),
        ));
    }
    let media_type = media_type_for_path(&canonical_path).to_string();
    let lease = state
        .inner
        .workspace_preview
        .insert(WorkspacePreviewLeaseBinding {
            workspace_root,
            relative_path: read.path.clone(),
            canonical_path,
            file_identity,
            file_change_marker: initial_file_change_marker,
            size_bytes: metadata.len(),
            media_type: media_type.clone(),
            modified_ns: modified_ns(&metadata),
        })?;
    Ok(serde_json::to_value(
        wire::WorkspaceFilePreviewOpenResult {
            path: read.path,
            content: read.content,
            truncated: read.truncated,
            binary: read.binary,
            editable: read.editable,
            editable_reason: read.editable_reason,
            size_bytes: read.size_bytes,
            revision: read.revision,
            line_ending: read.line_ending,
            unreadable: read.unreadable,
            media_type,
            resource_id: lease.resource_id.clone(),
            resource_path: format!("/_gateway/workspace-preview/{}", lease.resource_id),
            expires_at_ms: lease.expires_at_ms(),
        },
    )?)
}

pub(super) fn workspace_file_preview_release_value(
    state: &WebState,
    resource_id: &str,
) -> psychevo_runtime::Result<Value> {
    Ok(serde_json::to_value(
        wire::WorkspaceFilePreviewReleaseResult {
            released: state.inner.workspace_preview.release(resource_id),
        },
    )?)
}

pub(super) async fn workspace_preview_resource(
    State(state): State<WebState>,
    method: Method,
    headers: HeaderMap,
    AxumPath(resource_id): AxumPath<String>,
) -> Response<Body> {
    let cors_origin = match preview_cors_origin(&state, &headers) {
        Ok(origin) => origin,
        Err(()) => {
            return with_preview_cors(preview_status_response(StatusCode::FORBIDDEN), None);
        }
    };
    if method == Method::OPTIONS {
        let lookup_store = state.inner.workspace_preview.clone();
        let lookup_resource_id = resource_id.clone();
        match tokio::task::spawn_blocking(move || lookup_store.lookup(&lookup_resource_id)).await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => {
                return with_preview_cors(preview_error_response(error, None), cors_origin);
            }
            Err(_) => {
                return with_preview_cors(
                    preview_error_response(PreviewLeaseError::Io, None),
                    cors_origin,
                );
            }
        }
        return with_preview_cors(preview_options_response(), cors_origin);
    }
    let open_store = state.inner.workspace_preview.clone();
    let open_resource_id = resource_id.clone();
    let (lease, validated_file) =
        match tokio::task::spawn_blocking(move || open_store.open(&open_resource_id)).await {
            Ok(Ok(opened)) => opened,
            Ok(Err(error)) => {
                return with_preview_cors(preview_error_response(error, None), cors_origin);
            }
            Err(_) => {
                return with_preview_cors(
                    preview_error_response(PreviewLeaseError::Io, None),
                    cors_origin,
                );
            }
        };
    let range = match parse_single_range(headers.get(RANGE), lease.size_bytes) {
        Ok(range) => range,
        Err(()) => {
            return with_preview_cors(preview_range_error_response(&lease), cors_origin);
        }
    };
    let (status, start, content_length) = match range {
        Some(range) => (
            StatusCode::PARTIAL_CONTENT,
            range.start,
            range.end - range.start + 1,
        ),
        None => (StatusCode::OK, 0, lease.size_bytes),
    };

    let body = if method == Method::HEAD || content_length == 0 {
        drop(validated_file);
        Body::empty()
    } else {
        let mut file = tokio::fs::File::from_std(validated_file);
        if file.seek(std::io::SeekFrom::Start(start)).await.is_err() {
            return with_preview_cors(
                preview_error_response(PreviewLeaseError::Changed, None),
                cors_origin,
            );
        }
        let byte_stream =
            stream::unfold((file, content_length), |(mut file, remaining)| async move {
                if remaining == 0 {
                    return None;
                }
                let chunk_len = remaining.min(PREVIEW_STREAM_CHUNK_BYTES as u64) as usize;
                let mut chunk = vec![0_u8; chunk_len];
                match file.read(&mut chunk).await {
                    Ok(0) => Some((
                        Err(std::io::Error::new(
                            std::io::ErrorKind::UnexpectedEof,
                            "workspace preview file changed during streaming",
                        )),
                        (file, 0),
                    )),
                    Ok(read) => {
                        chunk.truncate(read);
                        Some((
                            Ok::<Bytes, std::io::Error>(Bytes::from(chunk)),
                            (file, remaining - read as u64),
                        ))
                    }
                    Err(error) => Some((Err(error), (file, 0))),
                }
            });
        Body::from_stream(byte_stream)
    };

    if let Err(error) = state.inner.workspace_preview.refresh(&resource_id) {
        return with_preview_cors(preview_error_response(error, None), cors_origin);
    }
    let mut response = Response::new(body);
    *response.status_mut() = status;
    add_preview_security_headers(response.headers_mut());
    add_preview_representation_headers(response.headers_mut(), &lease, content_length);
    if let Some(range) = range {
        response.headers_mut().insert(
            CONTENT_RANGE,
            HeaderValue::from_str(&format!(
                "bytes {}-{}/{}",
                range.start, range.end, lease.size_bytes
            ))
            .expect("valid content range"),
        );
    }
    with_preview_cors(response, cors_origin)
}

fn parse_single_range(
    value: Option<&HeaderValue>,
    size_bytes: u64,
) -> Result<Option<PreviewByteRange>, ()> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.to_str().map_err(|_| ())?;
    let spec = value.strip_prefix("bytes=").ok_or(())?;
    if spec.contains(',') || spec.is_empty() || size_bytes == 0 {
        return Err(());
    }
    let (start, end) = spec.split_once('-').ok_or(())?;
    if start.is_empty() {
        let suffix = end.parse::<u64>().map_err(|_| ())?;
        if suffix == 0 {
            return Err(());
        }
        let length = suffix.min(size_bytes);
        return Ok(Some(PreviewByteRange {
            start: size_bytes - length,
            end: size_bytes - 1,
        }));
    }
    let start = start.parse::<u64>().map_err(|_| ())?;
    if start >= size_bytes {
        return Err(());
    }
    let end = if end.is_empty() {
        size_bytes - 1
    } else {
        end.parse::<u64>().map_err(|_| ())?.min(size_bytes - 1)
    };
    if end < start {
        return Err(());
    }
    Ok(Some(PreviewByteRange { start, end }))
}

fn add_preview_security_headers(headers: &mut HeaderMap) {
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    headers.insert(CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox"));
}

fn add_preview_representation_headers(
    headers: &mut HeaderMap,
    lease: &WorkspacePreviewLease,
    content_length: u64,
) {
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(&lease.media_type)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(
        CONTENT_LENGTH,
        HeaderValue::from_str(&content_length.to_string()).expect("valid content length"),
    );
    headers.insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(
        ETAG,
        HeaderValue::from_str(&format!("\"preview-{}\"", lease.resource_id))
            .expect("valid preview etag"),
    );
}

fn preview_error_response(error: PreviewLeaseError, size_bytes: Option<u64>) -> Response<Body> {
    let status = match (error, size_bytes) {
        (_, Some(_)) => StatusCode::RANGE_NOT_SATISFIABLE,
        (PreviewLeaseError::Missing, None) => StatusCode::NOT_FOUND,
        (PreviewLeaseError::Gone, None) => StatusCode::GONE,
        (PreviewLeaseError::Changed, None) => StatusCode::CONFLICT,
        (PreviewLeaseError::Io, None) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    let mut response = Response::new(Body::empty());
    *response.status_mut() = status;
    add_preview_security_headers(response.headers_mut());
    if let Some(size_bytes) = size_bytes {
        response.headers_mut().insert(
            CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes */{size_bytes}"))
                .expect("valid unsatisfied content range"),
        );
        response
            .headers_mut()
            .insert(CONTENT_LENGTH, HeaderValue::from_static("0"));
        response
            .headers_mut()
            .insert(ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    }
    response
}

fn preview_range_error_response(lease: &WorkspacePreviewLease) -> Response<Body> {
    let mut response = preview_error_response(PreviewLeaseError::Io, Some(lease.size_bytes));
    add_preview_representation_headers(response.headers_mut(), lease, 0);
    response
}

fn preview_options_response() -> Response<Body> {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::NO_CONTENT;
    add_preview_security_headers(response.headers_mut());
    response.headers_mut().insert(
        ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, HEAD, OPTIONS"),
    );
    response.headers_mut().insert(
        ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Range"),
    );
    response.headers_mut().insert(
        ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static(
            "Accept-Ranges, Content-Length, Content-Range, Content-Type, ETag",
        ),
    );
    response
}

fn preview_status_response(status: StatusCode) -> Response<Body> {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = status;
    add_preview_security_headers(response.headers_mut());
    response
}

fn preview_cors_origin(state: &WebState, headers: &HeaderMap) -> Result<Option<HeaderValue>, ()> {
    let Some(origin) = headers.get(ORIGIN) else {
        return Ok(None);
    };
    let origin = origin.to_str().map_err(|_| ())?;
    let normalized = normalize_workspace_preview_request_origin(origin).ok_or(())?;
    let configured = state.inner.workspace_preview_origins.contains(&normalized);
    let same_origin = headers
        .get(HOST)
        .and_then(|host| host.to_str().ok())
        .and_then(|host| {
            normalized
                .split_once("://")
                .map(|(_, authority)| (host, authority))
        })
        .is_some_and(|(host, authority)| host.eq_ignore_ascii_case(authority));
    if !configured && !same_origin {
        return Err(());
    }
    HeaderValue::from_str(&normalized).map(Some).map_err(|_| ())
}

fn with_preview_cors(mut response: Response<Body>, origin: Option<HeaderValue>) -> Response<Body> {
    let Some(origin) = origin else {
        return response;
    };
    response
        .headers_mut()
        .insert(ACCESS_CONTROL_ALLOW_ORIGIN, origin);
    response
        .headers_mut()
        .insert(VARY, HeaderValue::from_static("Origin"));
    response.headers_mut().insert(
        ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static(
            "Accept-Ranges, Content-Length, Content-Range, Content-Type, ETag",
        ),
    );
    response
}

pub(super) fn configured_workspace_preview_origins(
    inherited_env: &std::collections::BTreeMap<String, String>,
) -> std::collections::BTreeSet<String> {
    let mut origins = DESKTOP_WORKSPACE_PREVIEW_ORIGINS
        .iter()
        .filter_map(|origin| normalize_workspace_preview_request_origin(origin))
        .collect::<std::collections::BTreeSet<_>>();
    origins.extend(
        inherited_env
            .get("PSYCHEVO_WORKBENCH_ORIGINS")
            .into_iter()
            .flat_map(|value| value.split(','))
            .filter_map(normalize_workspace_preview_origin),
    );
    origins
}

fn normalize_workspace_preview_request_origin(value: &str) -> Option<String> {
    normalize_workspace_preview_origin(value).or_else(|| {
        let parsed = reqwest::Url::parse(value.trim()).ok()?;
        if parsed.scheme() == "tauri"
            && parsed
                .host_str()
                .is_some_and(|host| host.eq_ignore_ascii_case("localhost"))
            && parsed.port().is_none()
            && parsed.username().is_empty()
            && parsed.password().is_none()
            && parsed.query().is_none()
            && parsed.fragment().is_none()
            && matches!(parsed.path(), "" | "/")
        {
            Some("tauri://localhost".to_string())
        } else {
            None
        }
    })
}

fn normalize_workspace_preview_origin(value: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(value.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https")
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.path() != "/"
    {
        return None;
    }
    Some(parsed.origin().ascii_serialization())
}

fn valid_resource_id(resource_id: &str) -> bool {
    resource_id.len() == 64
        && resource_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn retain_bounded_leases(leases: &mut HashMap<String, WorkspacePreviewLease>, now_ms: i64) {
    leases.retain(|_, lease| {
        let tombstone_at = lease
            .released_at_ms
            .unwrap_or_else(|| lease.expires_at_ms());
        lease.released_at_ms.is_none() && lease.expires_at_ms() > now_ms
            || tombstone_at + PREVIEW_TOMBSTONE_TTL_MS > now_ms
    });
    while leases.len() >= MAX_PREVIEW_LEASES {
        let Some(oldest_id) = leases
            .values()
            .min_by_key(|lease| {
                (
                    lease.released_at_ms.is_none() && lease.expires_at_ms() > now_ms,
                    lease.created_at_ms,
                )
            })
            .map(|lease| lease.resource_id.clone())
        else {
            break;
        };
        leases.remove(&oldest_id);
    }
}

fn modified_ns(metadata: &std::fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .unwrap_or(Duration::ZERO)
        .as_nanos()
}

fn system_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis()
        .min(i64::MAX as u128) as i64
}

pub(super) fn media_type_for_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("bmp") => "image/bmp",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("heic") => "image/heic",
        Some("heif") => "image/heif",
        Some("pdf") => "application/pdf",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("ogg" | "oga") => "audio/ogg",
        Some("opus") => "audio/opus",
        Some("m4a") => "audio/mp4",
        Some("aac") => "audio/aac",
        Some("flac") => "audio/flac",
        Some("weba") => "audio/webm",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("docm") => "application/vnd.ms-word.document.macroEnabled.12",
        Some("dotx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.template",
        Some("dotm") => "application/vnd.ms-word.template.macroEnabled.12",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("xlsm") => "application/vnd.ms-excel.sheet.macroEnabled.12",
        Some("xlsb") => "application/vnd.ms-excel.sheet.binary.macroEnabled.12",
        Some("xltx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.template",
        Some("xltm") => "application/vnd.ms-excel.template.macroEnabled.12",
        Some("pptx") => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        Some("pptm") => "application/vnd.ms-powerpoint.presentation.macroEnabled.12",
        Some("potx") => "application/vnd.openxmlformats-officedocument.presentationml.template",
        Some("potm") => "application/vnd.ms-powerpoint.template.macroEnabled.12",
        Some("ppsx") => "application/vnd.openxmlformats-officedocument.presentationml.slideshow",
        Some("ppsm") => "application/vnd.ms-powerpoint.slideshow.macroEnabled.12",
        Some("rtf") => "application/rtf",
        Some("odt") => "application/vnd.oasis.opendocument.text",
        Some("ods") => "application/vnd.oasis.opendocument.spreadsheet",
        Some("odp") => "application/vnd.oasis.opendocument.presentation",
        Some("ofd") => "application/ofd",
        Some("csv") => "text/csv",
        Some("tsv") => "text/tab-separated-values",
        Some("excalidraw") => "application/vnd.excalidraw+json",
        Some("zip") => "application/zip",
        Some("txt") => "text/plain",
        Some("md" | "markdown") => "text/markdown",
        Some("html" | "htm") => "text/html",
        Some("json") => "application/json",
        Some("xml") => "application/xml",
        _ => "application/octet-stream",
    }
}
