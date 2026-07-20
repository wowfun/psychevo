use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};

use psychevo_runtime::HostPlatform;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub(crate) const CODEX_ACP_BACKEND_ID: &str = "codex";
pub(crate) const CODEX_ACP_PACKAGE: &str = "@agentclientprotocol/codex-acp";
pub(crate) const CODEX_ACP_VERSION: &str = "1.1.2";
const CODEX_ACP_LOCK_SHA256: &str =
    "27c7dcc8cd23a8500828e6a503aae79a7c5857a72fc393e70e52b685cb451e4a";
const CODEX_ACP_PACKAGE_LOCK: &[u8] = include_bytes!("../assets/codex-acp-package-lock.json");
const MANAGED_TREE_SEAL_FILE: &str = ".psychevo-tree-seal.json";
const MANAGED_TREE_SEAL_SCHEMA_VERSION: u32 = 1;
const CODEX_ACP_PACKAGE_JSON: &str = r#"{
  "name": "psychevo-codex-acp-runtime",
  "version": "1.1.2",
  "private": true,
  "dependencies": {
    "@agentclientprotocol/codex-acp": "1.1.2"
  }
}
"#;
const MANAGED_NPM_CI_ARGS: &[&str] = &[
    "ci",
    "--omit=dev",
    "--ignore-scripts",
    "--no-audit",
    "--no-fund",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedCodexAcpPaths {
    pub(crate) root: PathBuf,
    pub(crate) package_lock: PathBuf,
    pub(crate) package_json: PathBuf,
    pub(crate) executable: PathBuf,
    pub(crate) tree_seal: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ManagedCodexAcpStatus {
    Ready(ManagedCodexAcpPaths),
    Missing {
        paths: ManagedCodexAcpPaths,
    },
    Invalid {
        paths: ManagedCodexAcpPaths,
        reason: String,
    },
}

#[derive(Debug, Deserialize)]
struct ManagedPackageManifest {
    name: String,
    version: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManagedTreeSeal {
    schema_version: u32,
    tree_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagedTreeVerificationPurpose {
    Ordinary,
    Launch,
    Explicit,
}

#[derive(Debug, Clone)]
struct ManagedTreeVerificationCacheEntry {
    seal_sha256: String,
    metadata_sha256: String,
    launch_verified: bool,
}

type ManagedTreeVerificationCache = Mutex<BTreeMap<PathBuf, ManagedTreeVerificationCacheEntry>>;

fn managed_tree_verification_cache() -> &'static ManagedTreeVerificationCache {
    static CACHE: OnceLock<ManagedTreeVerificationCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn invalidate_managed_tree_verification_cache(root: &Path) {
    let Ok(cache_key) = root.canonicalize() else {
        return;
    };
    if let Ok(mut cache) = managed_tree_verification_cache().lock() {
        cache.remove(&cache_key);
    }
}

pub(crate) fn managed_codex_acp_paths(home: &Path, platform: HostPlatform) -> ManagedCodexAcpPaths {
    let root = home
        .join("runtime-adapters")
        .join("codex-acp")
        .join(CODEX_ACP_VERSION);
    let package_root = root
        .join("node_modules")
        .join("@agentclientprotocol")
        .join("codex-acp");
    let executable_name = match platform {
        HostPlatform::Posix => "codex-acp",
        HostPlatform::Windows => "codex-acp.cmd",
    };
    ManagedCodexAcpPaths {
        package_lock: root.join("package-lock.json"),
        package_json: package_root.join("package.json"),
        executable: root.join("node_modules").join(".bin").join(executable_name),
        tree_seal: root.join(MANAGED_TREE_SEAL_FILE),
        root,
    }
}

pub(crate) fn inspect_managed_codex_acp(
    home: &Path,
    platform: HostPlatform,
) -> ManagedCodexAcpStatus {
    inspect_managed_codex_acp_for_purpose(home, platform, ManagedTreeVerificationPurpose::Ordinary)
}

pub(crate) fn inspect_managed_codex_acp_full(
    home: &Path,
    platform: HostPlatform,
) -> ManagedCodexAcpStatus {
    inspect_managed_codex_acp_for_purpose(home, platform, ManagedTreeVerificationPurpose::Explicit)
}

fn inspect_managed_codex_acp_for_purpose(
    home: &Path,
    platform: HostPlatform,
    purpose: ManagedTreeVerificationPurpose,
) -> ManagedCodexAcpStatus {
    let paths = managed_codex_acp_paths(home, platform);
    if !paths.root.is_dir() {
        return ManagedCodexAcpStatus::Missing { paths };
    }
    let missing_files = [
        ("package-lock.json", &paths.package_lock),
        ("package manifest", &paths.package_json),
        ("launcher", &paths.executable),
        ("tree seal", &paths.tree_seal),
    ]
    .into_iter()
    .filter_map(|(label, path)| (!path.is_file()).then_some(label))
    .collect::<Vec<_>>();
    if !missing_files.is_empty() {
        return ManagedCodexAcpStatus::Invalid {
            paths,
            reason: format!(
                "managed Codex ACP install is incomplete: missing {}",
                missing_files.join(", ")
            ),
        };
    }
    match inspect_managed_codex_acp_paths(paths.clone(), platform, purpose) {
        Ok(paths) => ManagedCodexAcpStatus::Ready(paths),
        Err(error) => ManagedCodexAcpStatus::Invalid {
            paths,
            reason: error.to_string(),
        },
    }
}

pub(crate) async fn install_managed_codex_acp(
    home: &Path,
    npm_program: &Path,
    platform: HostPlatform,
    inherited_env: &BTreeMap<String, String>,
) -> psychevo_runtime::Result<ManagedCodexAcpPaths> {
    install_managed_codex_acp_with_lock(
        home,
        npm_program,
        platform,
        inherited_env,
        CODEX_ACP_PACKAGE_LOCK,
    )
    .await
}

async fn install_managed_codex_acp_with_lock(
    home: &Path,
    npm_program: &Path,
    platform: HostPlatform,
    inherited_env: &BTreeMap<String, String>,
    package_lock: &[u8],
) -> psychevo_runtime::Result<ManagedCodexAcpPaths> {
    let actual_lock_sha = format!("{:x}", Sha256::digest(package_lock));
    if actual_lock_sha != CODEX_ACP_LOCK_SHA256 {
        return Err(psychevo_runtime::Error::Message(format!(
            "managed Codex ACP dependency lock integrity mismatch: expected {CODEX_ACP_LOCK_SHA256}, got {actual_lock_sha}"
        )));
    }

    let target = managed_codex_acp_paths(home, platform);
    let parent = target.root.parent().ok_or_else(|| {
        psychevo_runtime::Error::Message("managed Codex ACP path has no parent".to_string())
    })?;
    std::fs::create_dir_all(parent)?;
    let nonce = Uuid::now_v7();
    let staging_root = parent.join(format!(".{}.install-{nonce}", CODEX_ACP_VERSION));
    let backup_root = parent.join(format!(".{}.backup-{nonce}", CODEX_ACP_VERSION));
    std::fs::create_dir(&staging_root)?;
    let install_result = async {
        std::fs::write(staging_root.join("package.json"), CODEX_ACP_PACKAGE_JSON)?;
        std::fs::write(staging_root.join("package-lock.json"), package_lock)?;
        let mut command = managed_npm_command(npm_program, platform, inherited_env)?;
        let output = command
            .current_dir(&staging_root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|error| {
                psychevo_runtime::Error::Message(format!(
                    "failed to launch npm for managed Codex ACP: {error}"
                ))
            })?;
        if !output.status.success() {
            let stderr = psychevo_runtime::decode_process_output_for_platform(
                &output.stderr,
                platform == HostPlatform::Windows,
            );
            return Err(psychevo_runtime::Error::Message(format!(
                "managed Codex ACP npm install failed: {}",
                stderr.trim()
            )));
        }
        let staging_paths = managed_paths_for_root(&staging_root, platform);
        verify_managed_codex_acp_payload(&staging_paths, platform)?;
        write_managed_tree_seal(&staging_paths)?;
        inspect_managed_codex_acp_paths(
            staging_paths.clone(),
            platform,
            ManagedTreeVerificationPurpose::Explicit,
        )?;
        invalidate_managed_tree_verification_cache(&staging_paths.root);
        promote_managed_install(&staging_root, &backup_root, &target, platform)?;
        Ok(target.clone())
    }
    .await;
    if staging_root.exists() {
        let _ = std::fs::remove_dir_all(&staging_root);
    }
    if backup_root.exists()
        && !target.root.exists()
        && let Err(restore_error) = std::fs::rename(&backup_root, &target.root)
    {
        return Err(psychevo_runtime::Error::Message(format!(
            "{}; failed to restore the previous managed Codex ACP install: {restore_error}",
            install_result
                .as_ref()
                .err()
                .map(ToString::to_string)
                .unwrap_or_else(|| "managed install cleanup failed".to_string())
        )));
    }
    install_result
}

fn managed_npm_command(
    npm_program: &Path,
    platform: HostPlatform,
    env: &BTreeMap<String, String>,
) -> psychevo_runtime::Result<tokio::process::Command> {
    let args = MANAGED_NPM_CI_ARGS
        .iter()
        .map(OsString::from)
        .collect::<Vec<_>>();
    let mut command =
        psychevo_runtime::tokio_host_process_command(npm_program, &args, platform, env)?;
    command.env_clear();
    psychevo_runtime::apply_tokio_process_env(
        &mut command,
        env,
        psychevo_runtime::ProcessEnvOptions::new(&[])
            .with_windows_utf8_defaults(platform == HostPlatform::Windows),
    )?;
    Ok(command)
}

fn promote_managed_install(
    staging_root: &Path,
    backup_root: &Path,
    target: &ManagedCodexAcpPaths,
    platform: HostPlatform,
) -> psychevo_runtime::Result<()> {
    let had_existing = target.root.exists();
    if had_existing {
        std::fs::rename(&target.root, backup_root)?;
    }
    if let Err(promote_error) = std::fs::rename(staging_root, &target.root) {
        return Err(restore_managed_install_after_failure(
            target,
            backup_root,
            had_existing,
            promote_error.into(),
        ));
    }
    if let Err(verification_error) = inspect_managed_codex_acp_paths(
        target.clone(),
        platform,
        ManagedTreeVerificationPurpose::Explicit,
    ) {
        return Err(restore_managed_install_after_failure(
            target,
            backup_root,
            had_existing,
            verification_error,
        ));
    }
    if had_existing {
        let _ = std::fs::remove_dir_all(backup_root);
    }
    Ok(())
}

fn restore_managed_install_after_failure(
    target: &ManagedCodexAcpPaths,
    backup_root: &Path,
    had_existing: bool,
    original_error: psychevo_runtime::Error,
) -> psychevo_runtime::Error {
    if target.root.exists()
        && let Err(remove_error) = std::fs::remove_dir_all(&target.root)
    {
        return psychevo_runtime::Error::Message(format!(
            "{original_error}; failed to remove invalid managed install before restore: {remove_error}"
        ));
    }
    if had_existing && let Err(restore_error) = std::fs::rename(backup_root, &target.root) {
        return psychevo_runtime::Error::Message(format!(
            "{original_error}; failed to restore previous managed install: {restore_error}"
        ));
    }
    original_error
}

fn managed_paths_for_root(root: &Path, platform: HostPlatform) -> ManagedCodexAcpPaths {
    let executable = root.join("node_modules").join(".bin").join(match platform {
        HostPlatform::Posix => "codex-acp",
        HostPlatform::Windows => "codex-acp.cmd",
    });
    ManagedCodexAcpPaths {
        root: root.to_path_buf(),
        package_lock: root.join("package-lock.json"),
        package_json: root
            .join("node_modules")
            .join("@agentclientprotocol")
            .join("codex-acp")
            .join("package.json"),
        executable,
        tree_seal: root.join(MANAGED_TREE_SEAL_FILE),
    }
}

fn inspect_managed_codex_acp_paths(
    paths: ManagedCodexAcpPaths,
    platform: HostPlatform,
    purpose: ManagedTreeVerificationPurpose,
) -> psychevo_runtime::Result<ManagedCodexAcpPaths> {
    verify_managed_codex_acp_payload(&paths, platform)?;
    verify_managed_tree_seal(&paths, purpose, managed_tree_verification_cache())?;
    Ok(paths)
}

fn verify_managed_codex_acp_payload(
    paths: &ManagedCodexAcpPaths,
    platform: HostPlatform,
) -> psychevo_runtime::Result<()> {
    let manifest = std::fs::read_to_string(&paths.package_json)
        .ok()
        .and_then(|raw| serde_json::from_str::<ManagedPackageManifest>(&raw).ok())
        .ok_or_else(|| {
            psychevo_runtime::Error::Message(
                "managed Codex ACP package manifest is unreadable or invalid".to_string(),
            )
        })?;
    if manifest.name != CODEX_ACP_PACKAGE || manifest.version != CODEX_ACP_VERSION {
        return Err(psychevo_runtime::Error::Message(format!(
            "managed Codex ACP package must be {CODEX_ACP_PACKAGE}@{CODEX_ACP_VERSION}, found {}@{}",
            manifest.name, manifest.version
        )));
    }
    verify_managed_package_lock(paths)?;
    if !managed_executable_is_usable(&paths.executable, platform) {
        return Err(psychevo_runtime::Error::Message(
            "managed Codex ACP launcher is not executable".to_string(),
        ));
    }
    Ok(())
}

fn verify_managed_package_lock(paths: &ManagedCodexAcpPaths) -> psychevo_runtime::Result<()> {
    let lock = std::fs::read(&paths.package_lock).map_err(|error| {
        psychevo_runtime::Error::Message(format!(
            "managed Codex ACP dependency lock is missing or unreadable: {error}"
        ))
    })?;
    let actual = format!("{:x}", Sha256::digest(&lock));
    if actual != CODEX_ACP_LOCK_SHA256 || lock != CODEX_ACP_PACKAGE_LOCK {
        return Err(psychevo_runtime::Error::Message(format!(
            "managed Codex ACP dependency lock integrity mismatch: expected {CODEX_ACP_LOCK_SHA256}, got {actual}"
        )));
    }
    Ok(())
}

fn write_managed_tree_seal(paths: &ManagedCodexAcpPaths) -> psychevo_runtime::Result<()> {
    if std::fs::symlink_metadata(&paths.tree_seal).is_ok() {
        return Err(psychevo_runtime::Error::Message(format!(
            "managed Codex ACP payload contains reserved seal path `{MANAGED_TREE_SEAL_FILE}`"
        )));
    }
    let seal = ManagedTreeSeal {
        schema_version: MANAGED_TREE_SEAL_SCHEMA_VERSION,
        tree_sha256: managed_tree_sha256(&paths.root)?,
    };
    let mut serialized = serde_json::to_vec_pretty(&seal)?;
    serialized.push(b'\n');
    std::fs::write(&paths.tree_seal, serialized)?;
    Ok(())
}

/// Returns whether this call performed a full payload-content hash. Repeated
/// ordinary inspection still walks entry metadata to invalidate the cache, but
/// does not re-read package binaries when the sealed tree is unchanged.
fn verify_managed_tree_seal(
    paths: &ManagedCodexAcpPaths,
    purpose: ManagedTreeVerificationPurpose,
    cache: &ManagedTreeVerificationCache,
) -> psychevo_runtime::Result<bool> {
    let metadata = std::fs::symlink_metadata(&paths.tree_seal).map_err(|error| {
        psychevo_runtime::Error::Message(format!(
            "managed Codex ACP tree seal is missing or unreadable: {error}"
        ))
    })?;
    if !metadata.file_type().is_file() {
        return Err(psychevo_runtime::Error::Message(
            "managed Codex ACP tree seal must be a regular file".to_string(),
        ));
    }
    let raw = std::fs::read(&paths.tree_seal).map_err(|error| {
        psychevo_runtime::Error::Message(format!(
            "managed Codex ACP tree seal is unreadable: {error}"
        ))
    })?;
    let seal: ManagedTreeSeal = serde_json::from_slice(&raw).map_err(|_| {
        psychevo_runtime::Error::Message("managed Codex ACP tree seal is invalid".to_string())
    })?;
    if seal.schema_version != MANAGED_TREE_SEAL_SCHEMA_VERSION {
        return Err(psychevo_runtime::Error::Message(format!(
            "managed Codex ACP tree seal schema must be {MANAGED_TREE_SEAL_SCHEMA_VERSION}, found {}",
            seal.schema_version
        )));
    }
    let cache_key = paths.root.canonicalize().map_err(|error| {
        psychevo_runtime::Error::Message(format!(
            "managed Codex ACP install root cannot be canonicalized: {error}"
        ))
    })?;
    let seal_sha256 = format!("{:x}", Sha256::digest(&raw));
    let metadata_sha256 = managed_tree_metadata_sha256(&paths.root)?;
    let cache_hit = cache
        .lock()
        .map_err(|_| {
            psychevo_runtime::Error::Message(
                "managed Codex ACP verification cache is unavailable".to_string(),
            )
        })?
        .get(&cache_key)
        .is_some_and(|cached| {
            cached.seal_sha256 == seal_sha256
                && cached.metadata_sha256 == metadata_sha256
                && match purpose {
                    ManagedTreeVerificationPurpose::Ordinary => true,
                    ManagedTreeVerificationPurpose::Launch => cached.launch_verified,
                    ManagedTreeVerificationPurpose::Explicit => false,
                }
        });
    if cache_hit {
        return Ok(false);
    }

    let actual = managed_tree_sha256(&paths.root)?;
    if seal.tree_sha256 != actual {
        if let Ok(mut cache) = cache.lock() {
            cache.remove(&cache_key);
        }
        return Err(psychevo_runtime::Error::Message(format!(
            "managed Codex ACP installed payload integrity mismatch: expected {}, got {actual}",
            seal.tree_sha256
        )));
    }
    let verified_metadata_sha256 = managed_tree_metadata_sha256(&paths.root)?;
    if verified_metadata_sha256 != metadata_sha256 {
        if let Ok(mut cache) = cache.lock() {
            cache.remove(&cache_key);
        }
        return Err(psychevo_runtime::Error::Message(
            "managed Codex ACP payload changed during integrity verification".to_string(),
        ));
    }
    let mut cache = cache.lock().map_err(|_| {
        psychevo_runtime::Error::Message(
            "managed Codex ACP verification cache is unavailable".to_string(),
        )
    })?;
    let launch_verified = purpose == ManagedTreeVerificationPurpose::Launch
        || cache.get(&cache_key).is_some_and(|cached| {
            cached.seal_sha256 == seal_sha256
                && cached.metadata_sha256 == verified_metadata_sha256
                && cached.launch_verified
        });
    cache.insert(
        cache_key,
        ManagedTreeVerificationCacheEntry {
            seal_sha256,
            metadata_sha256: verified_metadata_sha256,
            launch_verified,
        },
    );
    Ok(true)
}

fn managed_tree_sha256(root: &Path) -> psychevo_runtime::Result<String> {
    let root_metadata = std::fs::symlink_metadata(root).map_err(|error| {
        psychevo_runtime::Error::Message(format!(
            "managed Codex ACP install root is unreadable: {error}"
        ))
    })?;
    if root_metadata.file_type().is_symlink() || !root_metadata.file_type().is_dir() {
        return Err(psychevo_runtime::Error::Message(
            "managed Codex ACP install root must be a real directory".to_string(),
        ));
    }
    let canonical_root = root.canonicalize().map_err(|error| {
        psychevo_runtime::Error::Message(format!(
            "managed Codex ACP install root cannot be canonicalized: {error}"
        ))
    })?;
    let mut entries = Vec::new();
    collect_managed_tree_entries(root, root, &mut entries)?;
    entries.sort_by_cached_key(|path| managed_os_bytes(path.as_os_str()));

    let mut digest = Sha256::new();
    digest.update(b"psychevo-managed-tree-v1\0");
    for relative in entries {
        let absolute = root.join(&relative);
        let metadata = std::fs::symlink_metadata(&absolute).map_err(|error| {
            psychevo_runtime::Error::Message(format!(
                "managed Codex ACP payload entry `{}` is unreadable: {error}",
                relative.display()
            ))
        })?;
        let path_bytes = managed_os_bytes(relative.as_os_str());
        update_managed_tree_digest_field(&mut digest, &path_bytes);
        let file_type = metadata.file_type();
        if file_type.is_dir() {
            digest.update(b"directory\0");
            digest.update(managed_permission_bits(&metadata).to_le_bytes());
        } else if file_type.is_file() {
            digest.update(b"file\0");
            digest.update(managed_permission_bits(&metadata).to_le_bytes());
            digest.update(metadata.len().to_le_bytes());
            let mut file = std::fs::File::open(&absolute)?;
            let mut buffer = [0_u8; 64 * 1024];
            loop {
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                digest.update(&buffer[..read]);
            }
        } else if file_type.is_symlink() {
            digest.update(b"symlink\0");
            let resolved = absolute.canonicalize().map_err(|error| {
                psychevo_runtime::Error::Message(format!(
                    "managed Codex ACP symlink `{}` cannot be resolved: {error}",
                    relative.display()
                ))
            })?;
            if !resolved.starts_with(&canonical_root) {
                return Err(psychevo_runtime::Error::Message(format!(
                    "managed Codex ACP symlink `{}` escapes the managed install",
                    relative.display()
                )));
            }
            let target = std::fs::read_link(&absolute)?;
            update_managed_tree_digest_field(&mut digest, &managed_os_bytes(target.as_os_str()));
        } else {
            return Err(psychevo_runtime::Error::Message(format!(
                "managed Codex ACP payload entry `{}` has an unsupported file type",
                relative.display()
            )));
        }
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn managed_tree_metadata_sha256(root: &Path) -> psychevo_runtime::Result<String> {
    let root_metadata = std::fs::symlink_metadata(root)?;
    if root_metadata.file_type().is_symlink() || !root_metadata.file_type().is_dir() {
        return Err(psychevo_runtime::Error::Message(
            "managed Codex ACP install root must be a real directory".to_string(),
        ));
    }
    let canonical_root = root.canonicalize()?;
    let mut entries = Vec::new();
    collect_managed_tree_entries(root, root, &mut entries)?;
    entries.sort_by_cached_key(|path| managed_os_bytes(path.as_os_str()));

    let mut digest = Sha256::new();
    digest.update(b"psychevo-managed-tree-metadata-v1\0");
    for relative in entries {
        let absolute = root.join(&relative);
        let metadata = std::fs::symlink_metadata(&absolute)?;
        update_managed_tree_digest_field(&mut digest, &managed_os_bytes(relative.as_os_str()));
        let file_type = metadata.file_type();
        if file_type.is_dir() {
            digest.update(b"directory\0");
        } else if file_type.is_file() {
            digest.update(b"file\0");
        } else if file_type.is_symlink() {
            digest.update(b"symlink\0");
            let resolved = absolute.canonicalize().map_err(|error| {
                psychevo_runtime::Error::Message(format!(
                    "managed Codex ACP symlink `{}` cannot be resolved: {error}",
                    relative.display()
                ))
            })?;
            if !resolved.starts_with(&canonical_root) {
                return Err(psychevo_runtime::Error::Message(format!(
                    "managed Codex ACP symlink `{}` escapes the managed install",
                    relative.display()
                )));
            }
            let target = std::fs::read_link(&absolute)?;
            update_managed_tree_digest_field(&mut digest, &managed_os_bytes(target.as_os_str()));
        } else {
            return Err(psychevo_runtime::Error::Message(format!(
                "managed Codex ACP payload entry `{}` has an unsupported file type",
                relative.display()
            )));
        }
        digest.update(managed_permission_bits(&metadata).to_le_bytes());
        digest.update(metadata.len().to_le_bytes());
        update_managed_modified_time(&mut digest, &metadata);
        update_managed_platform_metadata(&mut digest, &metadata);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn update_managed_modified_time(digest: &mut Sha256, metadata: &std::fs::Metadata) {
    match metadata.modified() {
        Ok(modified) => match modified.duration_since(std::time::UNIX_EPOCH) {
            Ok(duration) => {
                digest.update([1]);
                digest.update(duration.as_secs().to_le_bytes());
                digest.update(duration.subsec_nanos().to_le_bytes());
            }
            Err(error) => {
                let duration = error.duration();
                digest.update([2]);
                digest.update(duration.as_secs().to_le_bytes());
                digest.update(duration.subsec_nanos().to_le_bytes());
            }
        },
        Err(_) => digest.update([0]),
    }
}

#[cfg(unix)]
fn update_managed_platform_metadata(digest: &mut Sha256, metadata: &std::fs::Metadata) {
    use std::os::unix::fs::MetadataExt as _;
    digest.update(metadata.dev().to_le_bytes());
    digest.update(metadata.ino().to_le_bytes());
    digest.update(metadata.ctime().to_le_bytes());
    digest.update(metadata.ctime_nsec().to_le_bytes());
}

#[cfg(not(unix))]
fn update_managed_platform_metadata(_digest: &mut Sha256, _metadata: &std::fs::Metadata) {}

fn collect_managed_tree_entries(
    root: &Path,
    directory: &Path,
    entries: &mut Vec<PathBuf>,
) -> psychevo_runtime::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let absolute = entry.path();
        let relative = absolute
            .strip_prefix(root)
            .map_err(|_| {
                psychevo_runtime::Error::Message(
                    "managed Codex ACP payload escaped its install root".to_string(),
                )
            })?
            .to_path_buf();
        if relative == Path::new(MANAGED_TREE_SEAL_FILE) {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&absolute)?;
        if managed_metadata_is_unsupported_reparse_point(&metadata) {
            return Err(psychevo_runtime::Error::Message(format!(
                "managed Codex ACP payload entry `{}` is an unsupported Windows reparse point",
                relative.display()
            )));
        }
        entries.push(relative);
        if metadata.file_type().is_dir() {
            collect_managed_tree_entries(root, &absolute, entries)?;
        }
    }
    Ok(())
}

fn update_managed_tree_digest_field(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_le_bytes());
    digest.update(value);
}

#[cfg(unix)]
fn managed_os_bytes(value: &OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt as _;
    value.as_bytes().to_vec()
}

#[cfg(windows)]
fn managed_os_bytes(value: &OsStr) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt as _;
    value
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>()
}

#[cfg(not(any(unix, windows)))]
fn managed_os_bytes(value: &OsStr) -> Vec<u8> {
    value.to_string_lossy().into_owned().into_bytes()
}

#[cfg(unix)]
fn managed_permission_bits(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    metadata.permissions().mode() & 0o7777
}

#[cfg(not(unix))]
fn managed_permission_bits(metadata: &std::fs::Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}

#[cfg(windows)]
fn managed_metadata_is_unsupported_reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        && !metadata.file_type().is_symlink()
}

#[cfg(not(windows))]
fn managed_metadata_is_unsupported_reparse_point(_metadata: &std::fs::Metadata) -> bool {
    false
}

pub(crate) fn verified_managed_codex_acp_command(
    home: &Path,
    configured_command: &Path,
    platform: HostPlatform,
) -> psychevo_runtime::Result<PathBuf> {
    match inspect_managed_codex_acp_for_purpose(
        home,
        platform,
        ManagedTreeVerificationPurpose::Launch,
    ) {
        ManagedCodexAcpStatus::Ready(paths) => {
            let configured = psychevo_runtime::normalized_native_path(configured_command);
            let expected = psychevo_runtime::normalized_native_path(&paths.executable);
            if !paths.executable.is_absolute() || configured != expected {
                return Err(psychevo_runtime::Error::Message(format!(
                    "managed Codex ACP backend command must be the verified executable `{}`; run backend/repair",
                    paths.executable.display()
                )));
            }
            Ok(paths.executable)
        }
        ManagedCodexAcpStatus::Missing { .. } => Err(psychevo_runtime::Error::Message(
            "managed Codex ACP is not installed; run backend/install before starting a turn"
                .to_string(),
        )),
        ManagedCodexAcpStatus::Invalid { reason, .. } => Err(psychevo_runtime::Error::Message(
            format!("{reason}; run backend/repair before starting a turn"),
        )),
    }
}

fn managed_executable_is_usable(path: &Path, platform: HostPlatform) -> bool {
    if platform == HostPlatform::Windows {
        path.is_file()
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            path.metadata()
                .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
                .unwrap_or(false)
        }
        #[cfg(not(unix))]
        {
            path.is_file()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_fake_install(home: &Path, version: &str, platform: HostPlatform) {
        let paths = managed_codex_acp_paths(home, platform);
        write_fake_install_paths(&paths, version, platform);
    }

    fn write_fake_install_paths(
        paths: &ManagedCodexAcpPaths,
        version: &str,
        platform: HostPlatform,
    ) {
        std::fs::create_dir_all(paths.package_json.parent().expect("package parent"))
            .expect("package dir");
        std::fs::create_dir_all(paths.executable.parent().expect("bin parent")).expect("bin dir");
        std::fs::write(&paths.package_lock, CODEX_ACP_PACKAGE_LOCK).expect("package lock");
        std::fs::write(
            &paths.package_json,
            serde_json::json!({
                "name": CODEX_ACP_PACKAGE,
                "version": version,
            })
            .to_string(),
        )
        .expect("package manifest");
        let payload = paths
            .package_json
            .parent()
            .expect("package parent")
            .join("dist/cli.js");
        std::fs::create_dir_all(payload.parent().expect("payload parent")).expect("payload dir");
        std::fs::write(&payload, "#!/bin/sh\nexit 0\n").expect("payload");
        if platform == HostPlatform::Windows {
            std::fs::write(&paths.executable, "@echo off\r\nexit /b 0\r\n").expect("launcher");
        } else {
            #[cfg(unix)]
            std::os::unix::fs::symlink(
                Path::new("../@agentclientprotocol/codex-acp/dist/cli.js"),
                &paths.executable,
            )
            .expect("launcher symlink");
            #[cfg(not(unix))]
            std::fs::write(&paths.executable, "#!/bin/sh\nexit 0\n").expect("launcher");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&payload)
                .expect("payload metadata")
                .permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&payload, permissions).expect("payload mode");
            if platform == HostPlatform::Windows {
                let mut permissions = std::fs::metadata(&paths.executable)
                    .expect("launcher metadata")
                    .permissions();
                permissions.set_mode(0o755);
                std::fs::set_permissions(&paths.executable, permissions).expect("launcher mode");
            }
        }
        write_managed_tree_seal(paths).expect("tree seal");
    }

    fn assert_no_transaction_directories(home: &Path) {
        let parent = home.join("runtime-adapters/codex-acp");
        let leftovers = std::fs::read_dir(parent)
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.starts_with('.'))
            .collect::<Vec<_>>();
        assert!(leftovers.is_empty(), "transaction leftovers: {leftovers:?}");
    }

    fn test_install_env() -> BTreeMap<String, String> {
        let mut env = std::env::var("PATH")
            .ok()
            .map(|path| BTreeMap::from([("PATH".to_string(), path)]))
            .unwrap_or_default();
        env.insert(
            "PSYCHEVO_TEST_CODEX_ACP_PACKAGE".to_string(),
            CODEX_ACP_PACKAGE.to_string(),
        );
        env.insert(
            "PSYCHEVO_TEST_CODEX_ACP_VERSION".to_string(),
            CODEX_ACP_VERSION.to_string(),
        );
        env
    }

    #[cfg(unix)]
    fn fake_npm(temp: &Path, succeeds: bool) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;

        let path = temp.join(if succeeds { "fake-npm" } else { "failing-npm" });
        let body = if succeeds {
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/fake_managed_codex_npm.sh"
            ))
            .to_string()
        } else {
            "#!/bin/sh\nexit 17\n".to_string()
        };
        std::fs::write(&path, body).expect("fake npm");
        let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).expect("mode");
        path
    }

    #[cfg(unix)]
    fn fake_npm_requiring_captured_env(temp: &Path) -> PathBuf {
        let path = fake_npm(temp, true);
        std::fs::write(path.with_extension("require-captured-env"), "required\n")
            .expect("captured-env marker");
        path
    }

    #[test]
    fn managed_codex_acp_uses_versioned_private_path() {
        let home = Path::new("/tmp/psychevo-home");
        let paths = managed_codex_acp_paths(home, HostPlatform::Posix);

        assert_eq!(
            paths.executable,
            home.join("runtime-adapters/codex-acp/1.1.2/node_modules/.bin/codex-acp")
        );
        assert_eq!(
            managed_codex_acp_paths(home, HostPlatform::Windows).executable,
            home.join("runtime-adapters/codex-acp/1.1.2/node_modules/.bin/codex-acp.cmd")
        );
    }

    #[test]
    fn managed_codex_acp_requires_exact_package_identity() {
        let temp = tempfile::tempdir().expect("temp");
        write_fake_install(temp.path(), "9.9.9", HostPlatform::Posix);

        let status = inspect_managed_codex_acp(temp.path(), HostPlatform::Posix);

        assert!(matches!(
            status,
            ManagedCodexAcpStatus::Invalid { reason, .. }
                if reason.contains("@1.1.2") && reason.contains("@9.9.9")
        ));
    }

    #[test]
    fn managed_codex_acp_accepts_verified_offline_install() {
        let temp = tempfile::tempdir().expect("temp");
        write_fake_install(temp.path(), CODEX_ACP_VERSION, HostPlatform::Posix);

        let status = inspect_managed_codex_acp(temp.path(), HostPlatform::Posix);

        assert!(matches!(status, ManagedCodexAcpStatus::Ready(_)));
    }

    #[test]
    fn managed_codex_acp_accepts_verified_windows_command_shim() {
        let temp = tempfile::tempdir().expect("temp");
        write_fake_install(temp.path(), CODEX_ACP_VERSION, HostPlatform::Windows);

        assert!(matches!(
            inspect_managed_codex_acp(temp.path(), HostPlatform::Windows),
            ManagedCodexAcpStatus::Ready(_)
        ));
    }

    #[test]
    fn managed_codex_acp_rejects_modified_installed_lock() {
        let temp = tempfile::tempdir().expect("temp");
        write_fake_install(temp.path(), CODEX_ACP_VERSION, HostPlatform::Posix);
        let paths = managed_codex_acp_paths(temp.path(), HostPlatform::Posix);
        std::fs::write(&paths.package_lock, b"{}\n").expect("mutated lock");

        assert!(matches!(
            inspect_managed_codex_acp(temp.path(), HostPlatform::Posix),
            ManagedCodexAcpStatus::Invalid { reason, .. }
                if reason.contains("lock integrity mismatch")
        ));
    }

    #[test]
    fn managed_codex_acp_tree_cache_avoids_rehash_and_invalidates_on_payload_change() {
        let temp = tempfile::tempdir().expect("temp");
        write_fake_install(temp.path(), CODEX_ACP_VERSION, HostPlatform::Posix);
        let paths = managed_codex_acp_paths(temp.path(), HostPlatform::Posix);
        let cache = Mutex::new(BTreeMap::new());

        assert!(
            verify_managed_tree_seal(&paths, ManagedTreeVerificationPurpose::Ordinary, &cache,)
                .expect("first full verification")
        );
        assert!(
            !verify_managed_tree_seal(&paths, ManagedTreeVerificationPurpose::Ordinary, &cache,)
                .expect("cached verification"),
            "unchanged inspection must not re-read the payload tree"
        );
        assert!(
            verify_managed_tree_seal(&paths, ManagedTreeVerificationPurpose::Launch, &cache)
                .expect("first launch verification"),
            "ordinary readiness does not substitute for first-launch verification"
        );
        assert!(
            !verify_managed_tree_seal(&paths, ManagedTreeVerificationPurpose::Launch, &cache)
                .expect("cached launch verification"),
            "the same sealed launch must reuse its successful verification"
        );
        assert!(
            verify_managed_tree_seal(&paths, ManagedTreeVerificationPurpose::Explicit, &cache)
                .expect("explicit Doctor verification"),
            "explicit verification must always hash payload contents"
        );

        let payload = paths
            .package_json
            .parent()
            .expect("package parent")
            .join("dist/cli.js");
        std::fs::write(&payload, "#!/bin/sh\nexit 17\n").expect("tampered payload");
        let error =
            verify_managed_tree_seal(&paths, ManagedTreeVerificationPurpose::Ordinary, &cache)
                .expect_err("tampered payload");
        assert!(
            error.to_string().contains("payload integrity mismatch"),
            "{error}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn managed_codex_acp_tree_seal_rejects_out_of_tree_symlink() {
        let temp = tempfile::tempdir().expect("temp");
        write_fake_install(temp.path(), CODEX_ACP_VERSION, HostPlatform::Posix);
        let paths = managed_codex_acp_paths(temp.path(), HostPlatform::Posix);
        let outside = temp.path().join("outside");
        std::fs::write(&outside, "#!/bin/sh\n").expect("outside payload");
        std::fs::remove_file(&paths.executable).expect("remove launcher");
        std::os::unix::fs::symlink(&outside, &paths.executable).expect("escaping launcher");
        std::fs::remove_file(&paths.tree_seal).expect("remove old seal");

        let error = write_managed_tree_seal(&paths).expect_err("escaping symlink");

        assert!(error.to_string().contains("escapes the managed install"));
        assert!(!paths.tree_seal.exists());
    }

    #[test]
    fn managed_codex_acp_launch_fails_closed_for_missing_invalid_and_wrong_command() {
        let temp = tempfile::tempdir().expect("temp");
        let expected = managed_codex_acp_paths(temp.path(), HostPlatform::Posix);
        let missing = verified_managed_codex_acp_command(
            temp.path(),
            &expected.executable,
            HostPlatform::Posix,
        )
        .expect_err("missing install");
        assert!(missing.to_string().contains("backend/install"));

        write_fake_install(temp.path(), CODEX_ACP_VERSION, HostPlatform::Posix);
        let wrong = verified_managed_codex_acp_command(
            temp.path(),
            &temp.path().join("other-codex-acp"),
            HostPlatform::Posix,
        )
        .expect_err("wrong executable");
        assert!(wrong.to_string().contains("backend/repair"));

        let payload = expected
            .package_json
            .parent()
            .expect("package parent")
            .join("dist/cli.js");
        std::fs::write(payload, "tampered payload").expect("tamper payload");
        let invalid = verified_managed_codex_acp_command(
            temp.path(),
            &expected.executable,
            HostPlatform::Posix,
        )
        .expect_err("invalid install");
        assert!(invalid.to_string().contains("backend/repair"));
    }

    #[test]
    fn partial_managed_install_is_invalid_and_repairable_not_missing() {
        let temp = tempfile::tempdir().expect("temp");
        write_fake_install(temp.path(), CODEX_ACP_VERSION, HostPlatform::Windows);
        let paths = managed_codex_acp_paths(temp.path(), HostPlatform::Windows);
        std::fs::remove_file(paths.package_lock).expect("remove lock");

        assert!(matches!(
            inspect_managed_codex_acp(temp.path(), HostPlatform::Windows),
            ManagedCodexAcpStatus::Invalid { reason, .. }
                if reason.contains("missing package-lock.json")
        ));
    }

    #[test]
    fn managed_codex_acp_inspection_is_offline_and_side_effect_free() {
        let temp = tempfile::tempdir().expect("temp");

        assert!(matches!(
            inspect_managed_codex_acp(temp.path(), HostPlatform::Windows),
            ManagedCodexAcpStatus::Missing { .. }
        ));
        assert!(!temp.path().join("runtime-adapters").exists());
    }

    #[test]
    fn managed_npm_cmd_uses_windows_command_processor() {
        let env = BTreeMap::from([(
            "COMSPEC".to_string(),
            r"C:\Windows\System32\cmd.exe".to_string(),
        )]);
        let command = managed_npm_command(
            Path::new(r"C:\Program Files\nodejs\npm.cmd"),
            HostPlatform::Windows,
            &env,
        )
        .expect("managed npm command");
        let command = command.as_std();
        assert_eq!(
            command.get_program(),
            std::ffi::OsStr::new(r"C:\Windows\System32\cmd.exe")
        );
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(&args[..4], ["/D", "/S", "/V:OFF", "/C"]);
        #[cfg(not(windows))]
        assert_eq!(
            args[4],
            r#"""C:\Program Files\nodejs\npm.cmd" "ci" "--omit=dev" "--ignore-scripts" "--no-audit" "--no-fund"""#
        );
    }

    #[test]
    fn failed_final_verification_atomically_restores_previous_install() {
        let temp = tempfile::tempdir().expect("temp");
        write_fake_install(temp.path(), CODEX_ACP_VERSION, HostPlatform::Posix);
        let target = managed_codex_acp_paths(temp.path(), HostPlatform::Posix);
        let parent = target.root.parent().expect("target parent");
        let staging = parent.join(".test-invalid-staging");
        let backup = parent.join(".test-backup");
        let staging_paths = managed_paths_for_root(&staging, HostPlatform::Posix);
        write_fake_install_paths(&staging_paths, "9.9.9", HostPlatform::Posix);

        let error = promote_managed_install(&staging, &backup, &target, HostPlatform::Posix)
            .expect_err("invalid promoted install");

        assert!(error.to_string().contains("@9.9.9"), "{error}");
        assert!(matches!(
            inspect_managed_codex_acp(temp.path(), HostPlatform::Posix),
            ManagedCodexAcpStatus::Ready(_)
        ));
        assert!(!backup.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn managed_codex_acp_installs_to_verified_versioned_path() {
        let temp = tempfile::tempdir().expect("temp");
        let npm = fake_npm(temp.path(), true);
        let inherited_env = test_install_env();

        let installed =
            install_managed_codex_acp(temp.path(), &npm, HostPlatform::Posix, &inherited_env)
                .await
                .expect("managed install");

        assert_eq!(
            installed.root,
            temp.path().join("runtime-adapters/codex-acp/1.1.2")
        );
        assert!(matches!(
            inspect_managed_codex_acp(temp.path(), HostPlatform::Posix),
            ManagedCodexAcpStatus::Ready(_)
        ));
        assert_eq!(
            std::fs::read(installed.root.join("package-lock.json")).expect("installed lock"),
            CODEX_ACP_PACKAGE_LOCK
        );
        assert_no_transaction_directories(temp.path());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn managed_codex_acp_install_uses_only_captured_environment() {
        let temp = tempfile::tempdir().expect("temp");
        let npm = fake_npm_requiring_captured_env(temp.path());
        let mut inherited_env = test_install_env();
        inherited_env.insert(
            "PSYCHEVO_CAPTURED_INSTALL_ENV".to_string(),
            "captured".to_string(),
        );
        inherited_env.remove("HOME");

        install_managed_codex_acp(temp.path(), &npm, HostPlatform::Posix, &inherited_env)
            .await
            .expect("captured environment install");

        assert!(matches!(
            inspect_managed_codex_acp(temp.path(), HostPlatform::Posix),
            ManagedCodexAcpStatus::Ready(_)
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn managed_codex_acp_rejects_changed_dependency_lock_before_npm() {
        let temp = tempfile::tempdir().expect("temp");
        let npm = fake_npm(temp.path(), true);
        let inherited_env = test_install_env();

        let error = install_managed_codex_acp_with_lock(
            temp.path(),
            &npm,
            HostPlatform::Posix,
            &inherited_env,
            b"{}",
        )
        .await
        .expect_err("lock mismatch");

        assert!(
            error
                .to_string()
                .contains("dependency lock integrity mismatch")
        );
        assert!(!temp.path().join("runtime-adapters").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn failed_repair_preserves_existing_verified_install() {
        let temp = tempfile::tempdir().expect("temp");
        write_fake_install(temp.path(), CODEX_ACP_VERSION, HostPlatform::Posix);
        let original = managed_codex_acp_paths(temp.path(), HostPlatform::Posix);
        let npm = fake_npm(temp.path(), false);
        let inherited_env = test_install_env();

        install_managed_codex_acp(temp.path(), &npm, HostPlatform::Posix, &inherited_env)
            .await
            .expect_err("failing npm");

        assert!(original.executable.is_file());
        assert!(matches!(
            inspect_managed_codex_acp(temp.path(), HostPlatform::Posix),
            ManagedCodexAcpStatus::Ready(_)
        ));
        assert_no_transaction_directories(temp.path());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn successful_repair_replaces_unsealed_legacy_install() {
        let temp = tempfile::tempdir().expect("temp");
        write_fake_install(temp.path(), CODEX_ACP_VERSION, HostPlatform::Posix);
        let paths = managed_codex_acp_paths(temp.path(), HostPlatform::Posix);
        std::fs::remove_file(&paths.tree_seal).expect("remove legacy seal");
        assert!(matches!(
            inspect_managed_codex_acp(temp.path(), HostPlatform::Posix),
            ManagedCodexAcpStatus::Invalid { reason, .. } if reason.contains("tree seal")
        ));
        let npm = fake_npm(temp.path(), true);
        let inherited_env = test_install_env();

        install_managed_codex_acp(temp.path(), &npm, HostPlatform::Posix, &inherited_env)
            .await
            .expect("repair legacy install");

        assert!(matches!(
            inspect_managed_codex_acp(temp.path(), HostPlatform::Posix),
            ManagedCodexAcpStatus::Ready(_)
        ));
        assert_no_transaction_directories(temp.path());
    }
}
