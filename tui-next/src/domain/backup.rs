//! Nginx 配置备份与还原领域模型。
//!
//! 管理范围：
//! - `/etc/nginx` 根目录下的普通文件与符号链接（含 `nginx.conf`、`mime.types` 等）
//! - `sites-available/`
//! - `sites-enabled/`
//! - `conf.d/`
//! - `snippets/`
//! - `stream-conf.d/`
//! - `modules-enabled/`

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::NgToolError;
use crate::infra::archive::{create_tar_gz, read_tar_gz, sha256_hex};
use crate::infra::audit::AuditResult;
use crate::infra::AppContext;

/// schema 2 扩大到完整的 Nginx 配置范围，并记录文件模式与符号链接。
pub const MANIFEST_SCHEMA: u32 = 2;

const MANAGED_DIRS: &[&str] = &[
    "sites-available",
    "sites-enabled",
    "conf.d",
    "snippets",
    "stream-conf.d",
    "modules-enabled",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackupSource {
    Manual,
    PreRestore,
}

impl BackupSource {
    pub fn label(&self) -> &'static str {
        match self {
            BackupSource::Manual => "手动",
            BackupSource::PreRestore => "还原前",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestFile {
    pub path: String,
    pub mode: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestSymlink {
    pub path: String,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManifestScope {
    #[serde(default)]
    pub nginx_conf: bool,
    #[serde(default)]
    pub managed_directories: Vec<String>,
    #[serde(default)]
    pub directories: Vec<String>,
    #[serde(default)]
    pub files: Vec<ManifestFile>,
    #[serde(default)]
    pub symlinks: Vec<ManifestSymlink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub created_at: String,
    pub hostname: String,
    pub nginx_version: String,
    pub ngtool_version: String,
    pub source: BackupSource,
    #[serde(default)]
    pub scope: ManifestScope,
    /// 文件相对路径 → `sha256:<hex>`。
    #[serde(default)]
    pub checksums: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Backup {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub mtime: SystemTime,
    pub manifest: Option<Manifest>,
}

impl Backup {
    pub fn restorable(&self) -> bool {
        self.manifest
            .as_ref()
            .map(|m| m.schema_version == MANIFEST_SCHEMA)
            .unwrap_or(false)
    }

    pub fn source_label(&self) -> &'static str {
        match self.manifest.as_ref().map(|m| m.source) {
            Some(BackupSource::Manual) => "手动",
            Some(BackupSource::PreRestore) => "还原前",
            None => "外部",
        }
    }

    pub fn created_at_label(&self) -> String {
        self.manifest
            .as_ref()
            .map(|m| {
                DateTime::parse_from_rfc3339(&m.created_at)
                    .map(|dt| {
                        dt.with_timezone(&Local)
                            .format("%Y-%m-%d %H:%M:%S")
                            .to_string()
                    })
                    .unwrap_or_else(|_| m.created_at.clone())
            })
            .unwrap_or_else(|| {
                let dt: DateTime<Local> = self.mtime.into();
                dt.format("%Y-%m-%d %H:%M:%S").to_string()
            })
    }
}

#[derive(Debug, Clone)]
pub struct CreateBackupInput {
    pub source: BackupSource,
}

#[derive(Debug, Clone)]
pub struct RestoreImpact {
    pub will_overwrite: Vec<String>,
    pub will_enable: Vec<String>,
    pub will_disable: Vec<String>,
    pub missing_in_backup: Vec<String>,
}

#[derive(Debug, Clone)]
struct Snapshot {
    manifest: Manifest,
    files: BTreeMap<String, Vec<u8>>,
}

pub fn list_backups(ctx: &AppContext) -> Result<Vec<Backup>, NgToolError> {
    let dir = ctx.paths.backups.clone();
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| NgToolError::FileOperationFailed {
        path: dir.clone(),
        message: e.to_string(),
    })? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("gz") {
            continue;
        }
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mtime = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        let manifest = read_manifest(&path).ok();
        out.push(Backup {
            path,
            name,
            size: meta.len(),
            mtime,
            manifest,
        });
    }
    out.sort_by_key(|b| std::cmp::Reverse(b.mtime));
    Ok(out)
}

fn read_manifest(archive: &Path) -> Result<Manifest, String> {
    let entries = read_tar_gz(archive).map_err(|e| e.to_string())?;
    let bytes = entries
        .into_iter()
        .find(|(path, _)| path == Path::new("manifest.toml"))
        .map(|(_, bytes)| bytes)
        .ok_or_else(|| "manifest.toml 缺失".to_string())?;
    let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    toml::from_str(text).map_err(|e| e.to_string())
}

pub async fn create_backup(
    ctx: Arc<AppContext>,
    input: CreateBackupInput,
) -> Result<PathBuf, NgToolError> {
    let started = Instant::now();
    let nginx_root = nginx_root(&ctx);
    let backups_dir = ctx.paths.backups.clone();
    let nginx_version = ctx
        .nginx
        .version()
        .await
        .unwrap_or_else(|_| "unknown".into());
    let hostname = read_hostname();
    let now = Local::now();
    let timestamp = now.format("%Y%m%d-%H%M%S-%3f").to_string();
    let prefix = match input.source {
        BackupSource::Manual => "nginx-config",
        BackupSource::PreRestore => "pre-restore",
    };
    let stem = format!("{}-{}", prefix, timestamp);
    let final_path = backups_dir.join(format!("{}.tar.gz", stem));
    let tmp_path = ctx
        .paths
        .tmp
        .join(format!("{}.{}.tar.gz.tmp", stem, std::process::id()));
    let source = input.source;

    let result = tokio::task::spawn_blocking(move || -> Result<PathBuf, NgToolError> {
        let mut scope = ManifestScope {
            managed_directories: MANAGED_DIRS.iter().map(|s| (*s).to_string()).collect(),
            ..ManifestScope::default()
        };
        let mut entries = Vec::new();
        let mut checksums = BTreeMap::new();

        let conf = nginx_root.join("nginx.conf");
        collect_root_files(&nginx_root, &mut scope, &mut entries, &mut checksums)?;
        scope.nginx_conf = conf.is_file();

        for dir in MANAGED_DIRS {
            let path = nginx_root.join(dir);
            if path.is_dir() {
                collect_directory(&nginx_root, &path, &mut scope, &mut entries, &mut checksums)?;
            }
        }

        scope.directories.sort();
        scope.directories.dedup();
        scope.files.sort_by(|a, b| a.path.cmp(&b.path));
        scope.symlinks.sort_by(|a, b| a.path.cmp(&b.path));

        let manifest = Manifest {
            schema_version: MANIFEST_SCHEMA,
            created_at: now.to_rfc3339(),
            hostname,
            nginx_version,
            ngtool_version: crate::version::APP_VERSION.to_string(),
            source,
            scope,
            checksums,
        };
        let manifest_bytes =
            toml::to_string_pretty(&manifest).map_err(|e| NgToolError::FileOperationFailed {
                path: tmp_path.clone(),
                message: format!("manifest 序列化失败：{}", e),
            })?;
        entries.insert(0, (PathBuf::from("manifest.toml"), manifest_bytes.into()));

        if let Some(parent) = tmp_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| NgToolError::FileOperationFailed {
                path: parent.to_path_buf(),
                message: e.to_string(),
            })?;
        }
        if let Some(parent) = final_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| NgToolError::FileOperationFailed {
                path: parent.to_path_buf(),
                message: e.to_string(),
            })?;
        }
        let write_result = (|| {
            create_tar_gz(&tmp_path, &entries).map_err(|e| NgToolError::FileOperationFailed {
                path: tmp_path.clone(),
                message: e.to_string(),
            })?;
            let snapshot = load_snapshot_sync(&tmp_path)?;
            validate_snapshot(&snapshot)?;
            std::fs::rename(&tmp_path, &final_path).map_err(|e| {
                NgToolError::FileOperationFailed {
                    path: final_path.clone(),
                    message: e.to_string(),
                }
            })?;
            Ok(final_path.clone())
        })();
        if write_result.is_err() {
            let _ = std::fs::remove_file(&tmp_path);
        }
        write_result
    })
    .await;

    match result {
        Ok(Ok(path)) => {
            ctx.audit.log(
                "backup.create",
                path.file_name().and_then(|s| s.to_str()).unwrap_or(""),
                AuditResult::Success,
                started.elapsed().as_millis() as u64,
                json!({"source": source.label(), "schema": MANIFEST_SCHEMA}),
            );
            Ok(path)
        }
        Ok(Err(e)) => {
            ctx.audit.log(
                "backup.create",
                "(failed)",
                AuditResult::Failure,
                started.elapsed().as_millis() as u64,
                json!({"error": e.to_string()}),
            );
            Err(e)
        }
        Err(e) => Err(NgToolError::FileOperationFailed {
            path: PathBuf::new(),
            message: format!("任务异常：{}", e),
        }),
    }
}

fn collect_root_files(
    nginx_root: &Path,
    scope: &mut ManifestScope,
    entries: &mut Vec<(PathBuf, Vec<u8>)>,
    checksums: &mut BTreeMap<String, String>,
) -> Result<(), NgToolError> {
    let mut children: Vec<_> = std::fs::read_dir(nginx_root)
        .map_err(|e| NgToolError::FileOperationFailed {
            path: nginx_root.to_path_buf(),
            message: e.to_string(),
        })?
        .filter_map(Result::ok)
        .collect();
    children.sort_by_key(|entry| entry.file_name());
    for child in children {
        let path = child.path();
        let meta =
            std::fs::symlink_metadata(&path).map_err(|e| NgToolError::FileOperationFailed {
                path: path.clone(),
                message: e.to_string(),
            })?;
        if meta.file_type().is_symlink() {
            let target =
                std::fs::read_link(&path).map_err(|e| NgToolError::FileOperationFailed {
                    path: path.clone(),
                    message: e.to_string(),
                })?;
            scope.symlinks.push(ManifestSymlink {
                path: relative_string(nginx_root, &path)?,
                target: target.to_string_lossy().into_owned(),
            });
        } else if meta.is_file() {
            add_file(nginx_root, &path, scope, entries, checksums)?;
        }
    }
    Ok(())
}

fn collect_directory(
    nginx_root: &Path,
    dir: &Path,
    scope: &mut ManifestScope,
    entries: &mut Vec<(PathBuf, Vec<u8>)>,
    checksums: &mut BTreeMap<String, String>,
) -> Result<(), NgToolError> {
    let rel_dir = relative_string(nginx_root, dir)?;
    scope.directories.push(rel_dir);
    let mut children: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| NgToolError::FileOperationFailed {
            path: dir.to_path_buf(),
            message: e.to_string(),
        })?
        .filter_map(Result::ok)
        .collect();
    children.sort_by_key(|e| e.file_name());

    for child in children {
        let path = child.path();
        let meta =
            std::fs::symlink_metadata(&path).map_err(|e| NgToolError::FileOperationFailed {
                path: path.clone(),
                message: e.to_string(),
            })?;
        if meta.file_type().is_symlink() {
            let target =
                std::fs::read_link(&path).map_err(|e| NgToolError::FileOperationFailed {
                    path: path.clone(),
                    message: e.to_string(),
                })?;
            scope.symlinks.push(ManifestSymlink {
                path: relative_string(nginx_root, &path)?,
                target: target.to_string_lossy().into_owned(),
            });
        } else if meta.is_dir() {
            collect_directory(nginx_root, &path, scope, entries, checksums)?;
        } else if meta.is_file() {
            add_file(nginx_root, &path, scope, entries, checksums)?;
        }
    }
    Ok(())
}

fn add_file(
    nginx_root: &Path,
    path: &Path,
    scope: &mut ManifestScope,
    entries: &mut Vec<(PathBuf, Vec<u8>)>,
    checksums: &mut BTreeMap<String, String>,
) -> Result<(), NgToolError> {
    let bytes = std::fs::read(path).map_err(|e| NgToolError::FileOperationFailed {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    let meta = std::fs::metadata(path).map_err(|e| NgToolError::FileOperationFailed {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    let rel = relative_string(nginx_root, path)?;
    checksums.insert(rel.clone(), format!("sha256:{}", sha256_hex(&bytes)));
    scope.files.push(ManifestFile {
        path: rel.clone(),
        mode: meta.permissions().mode() & 0o7777,
    });
    entries.push((PathBuf::from(rel), bytes));
    Ok(())
}

fn relative_string(root: &Path, path: &Path) -> Result<String, NgToolError> {
    path.strip_prefix(root)
        .ok()
        .and_then(Path::to_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| NgToolError::InvalidInput {
            field: "backup_path".into(),
            message: format!("无法生成安全相对路径：{}", path.display()),
        })
}

fn read_hostname() -> String {
    if let Ok(s) = std::fs::read_to_string("/etc/hostname") {
        let trimmed = s.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }
    std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".into())
}

pub async fn delete_backup(ctx: Arc<AppContext>, path: PathBuf) -> Result<(), NgToolError> {
    let started = Instant::now();
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    if path.parent() != Some(ctx.paths.backups.as_path()) {
        return Err(NgToolError::InvalidInput {
            field: "backup".into(),
            message: "只能删除备份目录中的文件".into(),
        });
    }
    let res = tokio::task::spawn_blocking(move || std::fs::remove_file(&path))
        .await
        .map_err(|e| NgToolError::FileOperationFailed {
            path: PathBuf::new(),
            message: format!("任务异常：{}", e),
        })?;
    match res {
        Ok(()) => {
            ctx.audit.log(
                "backup.delete",
                &name,
                AuditResult::Success,
                started.elapsed().as_millis() as u64,
                json!({}),
            );
            Ok(())
        }
        Err(e) => {
            ctx.audit.log(
                "backup.delete",
                &name,
                AuditResult::Failure,
                started.elapsed().as_millis() as u64,
                json!({"error": e.to_string()}),
            );
            Err(NgToolError::FileOperationFailed {
                path: ctx.paths.backups.join(name),
                message: e.to_string(),
            })
        }
    }
}

pub fn impact_for_restore(ctx: &AppContext, manifest: &Manifest) -> std::io::Result<RestoreImpact> {
    let mut will_overwrite: Vec<String> = manifest
        .scope
        .files
        .iter()
        .map(|f| f.path.clone())
        .chain(manifest.scope.symlinks.iter().map(|s| s.path.clone()))
        .collect();
    will_overwrite.sort();

    let target_enabled: HashSet<String> = manifest
        .scope
        .files
        .iter()
        .map(|f| &f.path)
        .chain(manifest.scope.symlinks.iter().map(|s| &s.path))
        .filter_map(|p| p.strip_prefix("sites-enabled/"))
        .map(str::to_string)
        .collect();
    let current_enabled = collect_relative_entries(&ctx.probe.sites_enabled)?;
    let mut will_enable: Vec<_> = target_enabled
        .difference(&current_enabled)
        .cloned()
        .collect();
    let mut will_disable: Vec<_> = current_enabled
        .difference(&target_enabled)
        .cloned()
        .collect();
    will_enable.sort();
    will_disable.sort();

    let snapshot_paths: HashSet<&str> = manifest
        .scope
        .files
        .iter()
        .map(|f| f.path.as_str())
        .chain(manifest.scope.symlinks.iter().map(|s| s.path.as_str()))
        .collect();
    let mut missing_in_backup = Vec::new();
    for link in &manifest.scope.symlinks {
        if !link.path.starts_with("sites-enabled/") {
            continue;
        }
        let target = Path::new(&link.target);
        let target_rel = if target.is_absolute() {
            target
                .strip_prefix(nginx_root(ctx))
                .ok()
                .and_then(Path::to_str)
                .map(str::to_string)
        } else {
            Path::new(&link.path)
                .parent()
                .and_then(|p| normalize_relative(&p.join(target)))
        };
        if let Some(rel) = target_rel {
            if !snapshot_paths.contains(rel.as_str()) {
                missing_in_backup.push(link.path.clone());
            }
        }
    }
    missing_in_backup.sort();

    Ok(RestoreImpact {
        will_overwrite,
        will_enable,
        will_disable,
        missing_in_backup,
    })
}

fn collect_relative_entries(dir: &Path) -> std::io::Result<HashSet<String>> {
    if !dir.is_dir() {
        return Ok(HashSet::new());
    }
    let mut out = HashSet::new();
    for entry in std::fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        if let Some(name) = entry.file_name().to_str() {
            out.insert(name.to_string());
        }
    }
    Ok(out)
}

pub async fn restore_backup(
    ctx: Arc<AppContext>,
    archive_path: PathBuf,
) -> Result<RestoreOutcome, NgToolError> {
    let started = Instant::now();
    let snapshot = load_snapshot(archive_path.clone()).await?;
    validate_snapshot(&snapshot)?;

    let pre = create_backup(
        ctx.clone(),
        CreateBackupInput {
            source: BackupSource::PreRestore,
        },
    )
    .await?;
    let rollback_snapshot = load_snapshot(pre.clone()).await?;
    validate_snapshot(&rollback_snapshot)?;

    if let Err(e) = apply_snapshot_async(nginx_root(&ctx), snapshot).await {
        return finish_failed_restore(ctx, archive_path, pre, rollback_snapshot, e, started).await;
    }

    if let Err(e) = ctx.nginx.test_config().await {
        return finish_failed_restore(ctx, archive_path, pre, rollback_snapshot, e, started).await;
    }

    if let Err(e) = ctx.systemd.reload("nginx").await {
        return finish_failed_restore(ctx, archive_path, pre, rollback_snapshot, e, started).await;
    }

    ctx.audit.log(
        "backup.restore",
        &archive_path.to_string_lossy(),
        AuditResult::Success,
        started.elapsed().as_millis() as u64,
        json!({"pre_restore": pre.to_string_lossy()}),
    );
    Ok(RestoreOutcome::Ok { pre_restore: pre })
}

async fn finish_failed_restore(
    ctx: Arc<AppContext>,
    archive_path: PathBuf,
    pre: PathBuf,
    rollback_snapshot: Snapshot,
    original_error: NgToolError,
    started: Instant,
) -> Result<RestoreOutcome, NgToolError> {
    let error = original_error.to_string();
    let rollback = rollback_once(ctx.clone(), rollback_snapshot).await;
    let outcome = match rollback {
        Ok(()) => RestoreOutcome::FailedRolledBack {
            error,
            pre_restore: pre,
        },
        Err(rollback_error) => RestoreOutcome::FailedRollbackFailed {
            error,
            rollback_error: rollback_error.to_string(),
            pre_restore: pre,
        },
    };
    ctx.audit.log(
        "backup.restore",
        &archive_path.to_string_lossy(),
        AuditResult::Failure,
        started.elapsed().as_millis() as u64,
        json!({"outcome": format!("{:?}", outcome)}),
    );
    Ok(outcome)
}

async fn rollback_once(ctx: Arc<AppContext>, snapshot: Snapshot) -> Result<(), NgToolError> {
    apply_snapshot_async(nginx_root(&ctx), snapshot).await?;
    ctx.nginx.test_config().await?;
    ctx.systemd.reload("nginx").await
}

async fn load_snapshot(path: PathBuf) -> Result<Snapshot, NgToolError> {
    let error_path = path.clone();
    tokio::task::spawn_blocking(move || load_snapshot_sync(&path))
        .await
        .map_err(|e| NgToolError::FileOperationFailed {
            path: error_path,
            message: format!("任务异常：{}", e),
        })?
}

fn load_snapshot_sync(path: &Path) -> Result<Snapshot, NgToolError> {
    let entries = read_tar_gz(path).map_err(|e| NgToolError::FileOperationFailed {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    let mut unique = BTreeMap::new();
    for (entry_path, bytes) in entries {
        let rel = entry_path
            .to_str()
            .ok_or_else(|| NgToolError::ParseFailed {
                target: path.display().to_string(),
                message: "归档包含非 UTF-8 路径".into(),
            })?;
        if unique.insert(rel.to_string(), bytes).is_some() {
            return Err(NgToolError::ParseFailed {
                target: path.display().to_string(),
                message: format!("归档包含重复路径：{}", rel),
            });
        }
    }
    let manifest_bytes =
        unique
            .remove("manifest.toml")
            .ok_or_else(|| NgToolError::ParseFailed {
                target: path.display().to_string(),
                message: "manifest.toml 缺失".into(),
            })?;
    let manifest_text =
        std::str::from_utf8(&manifest_bytes).map_err(|e| NgToolError::ParseFailed {
            target: path.display().to_string(),
            message: format!("manifest 编码错误：{}", e),
        })?;
    let manifest = toml::from_str(manifest_text).map_err(|e| NgToolError::ParseFailed {
        target: path.display().to_string(),
        message: format!("manifest 解析失败：{}", e),
    })?;
    Ok(Snapshot {
        manifest,
        files: unique,
    })
}

fn validate_snapshot(snapshot: &Snapshot) -> Result<(), NgToolError> {
    let manifest = &snapshot.manifest;
    if manifest.schema_version != MANIFEST_SCHEMA {
        return Err(NgToolError::InvalidInput {
            field: "manifest".into(),
            message: format!(
                "schema 版本 {} 不兼容（当前支持 {}）",
                manifest.schema_version, MANIFEST_SCHEMA
            ),
        });
    }

    let expected_dirs: BTreeSet<String> = MANAGED_DIRS.iter().map(|s| (*s).to_string()).collect();
    let actual_dirs: BTreeSet<String> =
        manifest.scope.managed_directories.iter().cloned().collect();
    if actual_dirs != expected_dirs {
        return Err(parse_error(
            "manifest",
            "managed_directories 范围不完整或不受支持",
        ));
    }

    let mut paths = HashSet::new();
    for dir in &manifest.scope.directories {
        validate_managed_path(dir, false)?;
        if !paths.insert(dir.as_str()) {
            return Err(parse_error(dir, "manifest 路径重复"));
        }
    }
    for file in &manifest.scope.files {
        validate_managed_path(&file.path, true)?;
        if !paths.insert(file.path.as_str()) {
            return Err(parse_error(&file.path, "manifest 路径重复"));
        }
        let bytes = snapshot
            .files
            .get(&file.path)
            .ok_or_else(|| parse_error(&file.path, "manifest 声明的文件在归档中缺失"))?;
        let expected = manifest
            .checksums
            .get(&file.path)
            .ok_or_else(|| parse_error(&file.path, "文件 checksum 缺失"))?;
        let actual = format!("sha256:{}", sha256_hex(bytes));
        if &actual != expected {
            return Err(parse_error(
                &file.path,
                &format!("checksum 不一致：期望 {} 实际 {}", expected, actual),
            ));
        }
    }
    for link in &manifest.scope.symlinks {
        validate_managed_path(&link.path, true)?;
        if link.target.is_empty() {
            return Err(parse_error(&link.path, "符号链接目标为空"));
        }
        if !paths.insert(link.path.as_str()) {
            return Err(parse_error(&link.path, "manifest 路径重复"));
        }
    }

    let symlink_paths: HashSet<&str> = manifest
        .scope
        .symlinks
        .iter()
        .map(|link| link.path.as_str())
        .collect();
    for path in paths.iter().copied() {
        let mut parent = Path::new(path).parent();
        while let Some(candidate) = parent {
            if let Some(candidate) = candidate.to_str() {
                if symlink_paths.contains(candidate) {
                    return Err(parse_error(path, "路径不能位于归档符号链接之下"));
                }
            }
            parent = candidate.parent();
        }
    }

    if manifest.scope.nginx_conf != paths.contains("nginx.conf") {
        return Err(parse_error("nginx.conf", "nginx_conf 标记与文件清单不一致"));
    }
    let file_paths: BTreeSet<&str> = manifest
        .scope
        .files
        .iter()
        .map(|f| f.path.as_str())
        .collect();
    let checksum_paths: BTreeSet<&str> = manifest.checksums.keys().map(String::as_str).collect();
    let archive_paths: BTreeSet<&str> = snapshot.files.keys().map(String::as_str).collect();
    if checksum_paths != file_paths || archive_paths != file_paths {
        return Err(parse_error(
            "manifest",
            "文件、checksum 与归档条目不完全一致",
        ));
    }
    Ok(())
}

fn validate_managed_path(path: &str, allow_nginx_conf: bool) -> Result<(), NgToolError> {
    let p = Path::new(path);
    if p.is_absolute() || p.components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(parse_error(path, "路径必须是安全的相对路径"));
    }
    let mut components = p.components();
    let first = components.next().and_then(|c| match c {
        Component::Normal(s) => s.to_str(),
        _ => None,
    });
    if allow_nginx_conf && components.next().is_none() {
        return Ok(());
    }
    if !first.map(|s| MANAGED_DIRS.contains(&s)).unwrap_or(false) {
        return Err(parse_error(path, "路径不在允许的 Nginx 配置范围内"));
    }
    Ok(())
}

fn parse_error(target: &str, message: &str) -> NgToolError {
    NgToolError::ParseFailed {
        target: target.to_string(),
        message: message.to_string(),
    }
}

async fn apply_snapshot_async(root: PathBuf, snapshot: Snapshot) -> Result<(), NgToolError> {
    tokio::task::spawn_blocking(move || apply_snapshot(&root, &snapshot))
        .await
        .map_err(|e| NgToolError::FileOperationFailed {
            path: PathBuf::new(),
            message: format!("任务异常：{}", e),
        })?
}

fn apply_snapshot(root: &Path, snapshot: &Snapshot) -> Result<(), NgToolError> {
    validate_snapshot(snapshot)?;

    for dir in MANAGED_DIRS {
        let path = root.join(dir);
        remove_path(&path)?;
        std::fs::create_dir_all(&path).map_err(|e| NgToolError::FileOperationFailed {
            path: path.clone(),
            message: e.to_string(),
        })?;
    }

    let target_root_entries: HashSet<&str> = snapshot
        .manifest
        .scope
        .files
        .iter()
        .map(|file| file.path.as_str())
        .chain(
            snapshot
                .manifest
                .scope
                .symlinks
                .iter()
                .map(|link| link.path.as_str()),
        )
        .filter(|path| !path.contains('/'))
        .collect();
    for entry in std::fs::read_dir(root).map_err(|e| NgToolError::FileOperationFailed {
        path: root.to_path_buf(),
        message: e.to_string(),
    })? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let meta =
            std::fs::symlink_metadata(&path).map_err(|e| NgToolError::FileOperationFailed {
                path: path.clone(),
                message: e.to_string(),
            })?;
        if (meta.is_file() || meta.file_type().is_symlink())
            && entry
                .file_name()
                .to_str()
                .map(|name| !target_root_entries.contains(name))
                .unwrap_or(false)
        {
            remove_path(&path)?;
        }
    }

    let mut directories = snapshot.manifest.scope.directories.clone();
    directories.sort_by_key(|p| Path::new(p).components().count());
    for rel in directories {
        let path = root.join(&rel);
        std::fs::create_dir_all(&path).map_err(|e| NgToolError::FileOperationFailed {
            path,
            message: e.to_string(),
        })?;
    }

    for file in &snapshot.manifest.scope.files {
        let target = root.join(&file.path);
        let bytes = snapshot
            .files
            .get(&file.path)
            .ok_or_else(|| parse_error(&file.path, "归档文件缺失"))?;
        atomic_write_replace(&target, bytes, file.mode)?;
    }

    for link in &snapshot.manifest.scope.symlinks {
        let path = root.join(&link.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| NgToolError::FileOperationFailed {
                path: parent.to_path_buf(),
                message: e.to_string(),
            })?;
        }
        remove_path(&path)?;
        std::os::unix::fs::symlink(&link.target, &path).map_err(|e| {
            NgToolError::FileOperationFailed {
                path,
                message: e.to_string(),
            }
        })?;
    }
    Ok(())
}

fn remove_path(path: &Path) -> Result<(), NgToolError> {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return Ok(());
    };
    let result = if meta.is_dir() && !meta.file_type().is_symlink() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    result.map_err(|e| NgToolError::FileOperationFailed {
        path: path.to_path_buf(),
        message: e.to_string(),
    })
}

fn atomic_write_replace(target: &Path, bytes: &[u8], mode: u32) -> Result<(), NgToolError> {
    use std::io::Write as _;
    let parent = target
        .parent()
        .ok_or_else(|| NgToolError::FileOperationFailed {
            path: target.to_path_buf(),
            message: "目标路径缺少父目录".into(),
        })?;
    std::fs::create_dir_all(parent).map_err(|e| NgToolError::FileOperationFailed {
        path: parent.to_path_buf(),
        message: e.to_string(),
    })?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let name = target
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let tmp = parent.join(format!(
        ".{}.ngtool.{}.{}.tmp",
        name,
        std::process::id(),
        nonce
    ));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| NgToolError::FileOperationFailed {
                path: tmp.clone(),
                message: e.to_string(),
            })?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| NgToolError::FileOperationFailed {
                path: tmp.clone(),
                message: e.to_string(),
            })?;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode)).map_err(|e| {
            NgToolError::FileOperationFailed {
                path: tmp.clone(),
                message: e.to_string(),
            }
        })?;
        std::fs::rename(&tmp, target).map_err(|e| NgToolError::FileOperationFailed {
            path: target.to_path_buf(),
            message: e.to_string(),
        })?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

fn nginx_root(ctx: &AppContext) -> PathBuf {
    ctx.probe
        .sites_available
        .parent()
        .unwrap_or(ctx.probe.sites_available.as_path())
        .to_path_buf()
}

fn normalize_relative(path: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(p) => parts.push(p.to_os_string()),
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::CurDir => {}
            _ => return None,
        }
    }
    let mut normalized = PathBuf::new();
    for part in parts {
        normalized.push(part);
    }
    normalized.to_str().map(str::to_string)
}

#[derive(Debug, Clone)]
pub enum RestoreOutcome {
    Ok {
        pre_restore: PathBuf,
    },
    FailedRolledBack {
        error: String,
        pre_restore: PathBuf,
    },
    FailedRollbackFailed {
        error: String,
        rollback_error: String,
        pre_restore: PathBuf,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> Manifest {
        let mut checksums = BTreeMap::new();
        checksums.insert("nginx.conf".into(), format!("sha256:{}", sha256_hex(b"x")));
        Manifest {
            schema_version: MANIFEST_SCHEMA,
            created_at: "2026-07-10T00:00:00Z".into(),
            hostname: "host".into(),
            nginx_version: "nginx/1.24".into(),
            ngtool_version: "1.2.4".into(),
            source: BackupSource::Manual,
            scope: ManifestScope {
                nginx_conf: true,
                managed_directories: MANAGED_DIRS.iter().map(|s| (*s).to_string()).collect(),
                directories: MANAGED_DIRS.iter().map(|s| (*s).to_string()).collect(),
                files: vec![ManifestFile {
                    path: "nginx.conf".into(),
                    mode: 0o644,
                }],
                symlinks: vec![],
            },
            checksums,
        }
    }

    #[test]
    fn manifest_roundtrip() {
        let manifest = sample_manifest();
        let text = toml::to_string_pretty(&manifest).unwrap();
        let back: Manifest = toml::from_str(&text).unwrap();
        assert_eq!(back.schema_version, MANIFEST_SCHEMA);
        assert_eq!(back.scope.files[0].path, "nginx.conf");
    }

    #[test]
    fn validation_rejects_missing_archive_file() {
        let snapshot = Snapshot {
            manifest: sample_manifest(),
            files: BTreeMap::new(),
        };
        assert!(validate_snapshot(&snapshot).is_err());
    }

    #[test]
    fn validation_rejects_path_traversal() {
        assert!(validate_managed_path("sites-available/../../etc/passwd", false).is_err());
        assert!(validate_managed_path("/etc/passwd", false).is_err());
    }

    #[test]
    fn normalize_relative_link_target() {
        assert_eq!(
            normalize_relative(Path::new("sites-enabled/../sites-available/app.conf")),
            Some("sites-available/app.conf".into())
        );
    }

    #[test]
    fn apply_snapshot_restores_files_links_and_removes_extras() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        for dir in MANAGED_DIRS {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        std::fs::write(root.join("conf.d/extra.conf"), b"extra").unwrap();

        let nginx_conf = b"events {}\nhttp {}\n".to_vec();
        let site = b"server { listen 80; }\n".to_vec();
        let stream = b"server { listen 44343; proxy_pass 127.0.0.1:38443; }\n".to_vec();
        let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        files.insert("nginx.conf".into(), nginx_conf.clone());
        files.insert("sites-available/app.conf".into(), site.clone());
        files.insert("stream-conf.d/proxy.conf".into(), stream.clone());

        let listed_files = vec![
            ManifestFile {
                path: "nginx.conf".into(),
                mode: 0o640,
            },
            ManifestFile {
                path: "sites-available/app.conf".into(),
                mode: 0o644,
            },
            ManifestFile {
                path: "stream-conf.d/proxy.conf".into(),
                mode: 0o644,
            },
        ];
        let checksums = files
            .iter()
            .map(|(path, bytes)| (path.clone(), format!("sha256:{}", sha256_hex(bytes))))
            .collect();
        let snapshot = Snapshot {
            manifest: Manifest {
                schema_version: MANIFEST_SCHEMA,
                created_at: "2026-07-10T00:00:00Z".into(),
                hostname: "host".into(),
                nginx_version: "nginx/1.24".into(),
                ngtool_version: "1.2.4".into(),
                source: BackupSource::Manual,
                scope: ManifestScope {
                    nginx_conf: true,
                    managed_directories: MANAGED_DIRS.iter().map(|s| (*s).to_string()).collect(),
                    directories: MANAGED_DIRS.iter().map(|s| (*s).to_string()).collect(),
                    files: listed_files,
                    symlinks: vec![ManifestSymlink {
                        path: "sites-enabled/app.conf".into(),
                        target: "../sites-available/app.conf".into(),
                    }],
                },
                checksums,
            },
            files,
        };

        apply_snapshot(root, &snapshot).unwrap();

        assert_eq!(std::fs::read(root.join("nginx.conf")).unwrap(), nginx_conf);
        assert_eq!(
            std::fs::read(root.join("sites-available/app.conf")).unwrap(),
            site
        );
        assert_eq!(
            std::fs::read(root.join("stream-conf.d/proxy.conf")).unwrap(),
            stream
        );
        assert!(!root.join("conf.d/extra.conf").exists());
        assert_eq!(
            std::fs::read_link(root.join("sites-enabled/app.conf")).unwrap(),
            PathBuf::from("../sites-available/app.conf")
        );
        assert_eq!(
            std::fs::metadata(root.join("nginx.conf"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
    }
}
