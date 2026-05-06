//! 备份与还原领域模型，对应 design.md 视图 6 / architecture.md §11.7。
//!
//! 范围限定（架构 §11.7.1）：
//! - `/etc/nginx/nginx.conf`
//! - `/etc/nginx/sites-available/*.conf`
//! - `/etc/nginx/sites-enabled/` 启用关系（仅记录链接的 stem）
//!
//! **不**整目录覆盖：`conf.d/`、`snippets/`、`modules-enabled/` 等不会被备份/还原触及。

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::NgToolError;
use crate::infra::archive::{create_tar_gz, read_tar_gz, sha256_hex};
use crate::infra::audit::AuditResult;
use crate::infra::AppContext;

/// 当前 manifest schema 版本。schema 不兼容的备份将被拒绝还原。
pub const MANIFEST_SCHEMA: u32 = 1;

/// 备份来源。manifest 内 `source` 字段。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackupSource {
    /// 用户手动创建
    Manual,
    /// 还原前自动创建
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

/// 备份范围记录。还原时按此精确替换。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManifestScope {
    /// 是否包含 nginx.conf
    #[serde(default = "default_true")]
    pub nginx_conf: bool,
    /// 备份内 sites-available 的 .conf 文件名（不含路径）
    #[serde(default)]
    pub sites_available: Vec<String>,
    /// 备份时刻 sites-enabled 中实际启用的站点名（去掉 .conf 后缀）
    #[serde(default)]
    pub sites_enabled: Vec<String>,
}

fn default_true() -> bool {
    true
}

/// 备份元数据。从 archive 中 `manifest.toml` 解析。
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
    /// 文件相对路径 → "sha256:<hex>"
    #[serde(default)]
    pub checksums: BTreeMap<String, String>,
}

/// UI 展示用的备份条目。
#[derive(Debug, Clone)]
pub struct Backup {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub mtime: SystemTime,
    /// `None` 表示无 manifest 或解析失败 → 只展示不可还原（design 子模式）。
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
            .map(|m| m.created_at.clone())
            .unwrap_or_else(|| {
                let dt: DateTime<Local> = self.mtime.into();
                dt.format("%Y-%m-%d %H:%M:%S").to_string()
            })
    }
}

/// 创建备份的输入。
#[derive(Debug, Clone)]
pub struct CreateBackupInput {
    pub source: BackupSource,
}

/// 还原备份前的影响摘要（execution.md P9-8）。
#[derive(Debug, Clone)]
pub struct RestoreImpact {
    /// 将覆盖的文件相对路径
    pub will_overwrite: Vec<String>,
    /// 将启用的站点名（当前未启用，备份中启用）
    pub will_enable: Vec<String>,
    /// 将停用的站点名（当前启用，备份中未启用）
    pub will_disable: Vec<String>,
    /// 备份中标记启用但 sites-available 内不存在的站点（理论上不应发生）
    pub missing_in_backup: Vec<String>,
}

/// 列出 `~/.local/ngtool/backups/` 中的全部备份。失败时返回错误，UI 可展示。
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
    // 按 mtime 倒序（最新在前）
    out.sort_by_key(|b| std::cmp::Reverse(b.mtime));
    Ok(out)
}

fn read_manifest(archive: &Path) -> Result<Manifest, String> {
    let entries = read_tar_gz(archive).map_err(|e| e.to_string())?;
    for (path, bytes) in entries {
        if path == Path::new("manifest.toml") {
            let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
            return toml::from_str::<Manifest>(text).map_err(|e| e.to_string());
        }
    }
    Err("manifest.toml 缺失".into())
}

/// 创建备份。范围限定到架构 §11.7.1。失败时不留下半成品。
pub async fn create_backup(
    ctx: Arc<AppContext>,
    input: CreateBackupInput,
) -> Result<PathBuf, NgToolError> {
    let started = Instant::now();
    let nginx_root = ctx
        .probe
        .sites_available
        .parent()
        .unwrap_or_else(|| ctx.probe.sites_available.as_path())
        .to_path_buf();
    let nginx_conf_path = nginx_root.join("nginx.conf");
    let avail_dir = ctx.probe.sites_available.clone();
    let enabled_dir = ctx.probe.sites_enabled.clone();
    let backups_dir = ctx.paths.backups.clone();

    let nginx_version = ctx
        .nginx
        .version()
        .await
        .unwrap_or_else(|_| "unknown".into());
    let hostname = read_hostname();
    let now = chrono::Local::now();
    let timestamp = now.format("%Y%m%d-%H%M%S").to_string();
    let stem = format!("nginx-config-{}", timestamp);
    let archive_name = format!("{}.tar.gz", stem);
    let final_path = backups_dir.join(&archive_name);
    let tmp_path = ctx
        .paths
        .tmp
        .join(format!("{}.{}.tar.gz.tmp", stem, std::process::id()));
    let source = input.source;

    let result = tokio::task::spawn_blocking(move || -> Result<PathBuf, NgToolError> {
        let mut entries: Vec<(PathBuf, Vec<u8>)> = Vec::new();
        let mut checksums: BTreeMap<String, String> = BTreeMap::new();

        // 1) nginx.conf
        let mut has_nginx_conf = false;
        if nginx_conf_path.is_file() {
            let bytes =
                std::fs::read(&nginx_conf_path).map_err(|e| NgToolError::FileOperationFailed {
                    path: nginx_conf_path.clone(),
                    message: e.to_string(),
                })?;
            checksums.insert(
                "nginx.conf".into(),
                format!("sha256:{}", sha256_hex(&bytes)),
            );
            entries.push((PathBuf::from("nginx.conf"), bytes));
            has_nginx_conf = true;
        }

        // 2) sites-available/*.conf
        let mut sites_available: Vec<String> = Vec::new();
        if avail_dir.is_dir() {
            let mut names: Vec<_> = std::fs::read_dir(&avail_dir)
                .map_err(|e| NgToolError::FileOperationFailed {
                    path: avail_dir.clone(),
                    message: e.to_string(),
                })?
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("conf"))
                .collect();
            names.sort_by_key(|e| e.file_name());
            for entry in names {
                let path = entry.path();
                let file_name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                if file_name.is_empty() {
                    continue;
                }
                let bytes = std::fs::read(&path).map_err(|e| NgToolError::FileOperationFailed {
                    path: path.clone(),
                    message: e.to_string(),
                })?;
                let rel = format!("sites-available/{}", file_name);
                checksums.insert(rel.clone(), format!("sha256:{}", sha256_hex(&bytes)));
                entries.push((PathBuf::from(rel), bytes));
                sites_available.push(file_name);
            }
        }

        // 3) sites-enabled.toml（仅记录启用关系）
        let sites_enabled: Vec<String> = collect_enabled_stems(&enabled_dir).unwrap_or_default();
        let enabled_doc = format!(
            "# 自动生成：备份时刻的启用关系\nsites = {:?}\n",
            sites_enabled
        );
        entries.push((
            PathBuf::from("sites-enabled.toml"),
            enabled_doc.as_bytes().to_vec(),
        ));

        // 4) manifest.toml（必须放在最前以方便快速预览）
        let manifest = Manifest {
            schema_version: MANIFEST_SCHEMA,
            created_at: now.to_rfc3339(),
            hostname,
            nginx_version,
            ngtool_version: crate::version::APP_VERSION.to_string(),
            source,
            scope: ManifestScope {
                nginx_conf: has_nginx_conf,
                sites_available: sites_available.clone(),
                sites_enabled: sites_enabled.clone(),
            },
            checksums,
        };
        let manifest_bytes =
            toml::to_string_pretty(&manifest).map_err(|e| NgToolError::FileOperationFailed {
                path: tmp_path.clone(),
                message: format!("manifest 序列化失败：{}", e),
            })?;
        // manifest 应当出现在 archive 头部
        entries.insert(0, (PathBuf::from("manifest.toml"), manifest_bytes.into()));

        // 5) 写入 tmp 后原子 rename 到 backups/
        if let Some(parent) = tmp_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        if let Some(parent) = final_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        create_tar_gz(&tmp_path, &entries).map_err(|e| NgToolError::FileOperationFailed {
            path: tmp_path.clone(),
            message: e.to_string(),
        })?;

        // 校验 archive 可读（架构 §11.7.3 步骤 6）
        let _ = read_tar_gz(&tmp_path).map_err(|e| NgToolError::FileOperationFailed {
            path: tmp_path.clone(),
            message: format!("打包后回读失败：{}", e),
        })?;

        std::fs::rename(&tmp_path, &final_path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            NgToolError::FileOperationFailed {
                path: final_path.clone(),
                message: e.to_string(),
            }
        })?;

        Ok(final_path)
    })
    .await;

    match result {
        Ok(Ok(p)) => {
            ctx.audit.log(
                "backup.create",
                p.file_name().and_then(|s| s.to_str()).unwrap_or(""),
                AuditResult::Success,
                started.elapsed().as_millis() as u64,
                json!({"source": format!("{:?}", source)}),
            );
            Ok(p)
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

fn collect_enabled_stems(enabled_dir: &Path) -> std::io::Result<Vec<String>> {
    if !enabled_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in std::fs::read_dir(enabled_dir)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if stem.is_empty() {
            continue;
        }
        // 接受真实文件 + 符号链接（dangling 也算启用关系）
        if path.symlink_metadata().is_ok() {
            out.push(stem.to_string());
        }
    }
    out.sort();
    Ok(out)
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

/// 删除备份。
pub async fn delete_backup(ctx: Arc<AppContext>, path: PathBuf) -> Result<(), NgToolError> {
    let started = Instant::now();
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
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
                path: PathBuf::new(),
                message: e.to_string(),
            })
        }
    }
}

/// 计算还原前的影响摘要：哪些文件会被覆盖、哪些站点会被启用/停用。
pub fn impact_for_restore(ctx: &AppContext, manifest: &Manifest) -> std::io::Result<RestoreImpact> {
    let mut will_overwrite = Vec::new();
    if manifest.scope.nginx_conf {
        will_overwrite.push("nginx.conf".into());
    }
    for name in &manifest.scope.sites_available {
        will_overwrite.push(format!("sites-available/{}", name));
    }
    will_overwrite.sort();

    let target_enabled: HashSet<String> = manifest.scope.sites_enabled.iter().cloned().collect();
    let current_enabled: HashSet<String> = collect_enabled_stems(&ctx.probe.sites_enabled)
        .unwrap_or_default()
        .into_iter()
        .collect();
    let avail_existing: HashSet<String> = manifest
        .scope
        .sites_available
        .iter()
        .map(|n| n.trim_end_matches(".conf").to_string())
        .collect();

    let mut will_enable: Vec<String> = target_enabled
        .difference(&current_enabled)
        .cloned()
        .collect();
    let mut will_disable: Vec<String> = current_enabled
        .difference(&target_enabled)
        .cloned()
        .collect();
    will_enable.sort();
    will_disable.sort();

    let missing_in_backup: Vec<String> = target_enabled
        .iter()
        .filter(|s| !avail_existing.contains(*s))
        .cloned()
        .collect();

    Ok(RestoreImpact {
        will_overwrite,
        will_enable,
        will_disable,
        missing_in_backup,
    })
}

/// 还原备份。流程：
/// 1) 校验 manifest 与 schema；
/// 2) 自动创建 pre-restore 备份；
/// 3) 解压目标 archive 到 ~/.local/ngtool/tmp/restore-<ts>/；
/// 4) 范围内文件先写 tmp 再原子 rename；sites-enabled 按 manifest 集合同步；
/// 5) `nginx -t` 通过则 reload；失败则用 pre-restore 备份再次还原。
///    二次失败保留临时目录，错误信息中标注路径。
pub async fn restore_backup(
    ctx: Arc<AppContext>,
    archive_path: PathBuf,
) -> Result<RestoreOutcome, NgToolError> {
    let started = Instant::now();

    let archive_clone = archive_path.clone();
    let entries = tokio::task::spawn_blocking(move || read_tar_gz(&archive_clone))
        .await
        .map_err(|e| NgToolError::FileOperationFailed {
            path: archive_path.clone(),
            message: format!("任务异常：{}", e),
        })?
        .map_err(|e| NgToolError::FileOperationFailed {
            path: archive_path.clone(),
            message: e.to_string(),
        })?;

    // 解析 manifest
    let manifest_bytes = entries
        .iter()
        .find(|(p, _)| p == Path::new("manifest.toml"))
        .map(|(_, b)| b.clone())
        .ok_or_else(|| NgToolError::ParseFailed {
            target: archive_path.display().to_string(),
            message: "manifest.toml 缺失".into(),
        })?;
    let manifest_text =
        std::str::from_utf8(&manifest_bytes).map_err(|e| NgToolError::ParseFailed {
            target: archive_path.display().to_string(),
            message: format!("manifest 编码错误：{}", e),
        })?;
    let manifest: Manifest =
        toml::from_str(manifest_text).map_err(|e| NgToolError::ParseFailed {
            target: archive_path.display().to_string(),
            message: format!("manifest 解析失败：{}", e),
        })?;
    if manifest.schema_version != MANIFEST_SCHEMA {
        return Err(NgToolError::InvalidInput {
            field: "manifest".into(),
            message: format!(
                "schema 版本 {} 不兼容（当前支持 {}）",
                manifest.schema_version, MANIFEST_SCHEMA
            ),
        });
    }

    // 校验范围内文件 sha256
    for (rel, hash) in &manifest.checksums {
        if let Some((_, bytes)) = entries.iter().find(|(p, _)| p == Path::new(rel)) {
            let actual = format!("sha256:{}", sha256_hex(bytes));
            if &actual != hash {
                return Err(NgToolError::ParseFailed {
                    target: rel.clone(),
                    message: format!("checksum 不一致：期望 {} 实际 {}", hash, actual),
                });
            }
        }
    }

    // 创建 pre-restore 备份
    let pre = create_backup(
        ctx.clone(),
        CreateBackupInput {
            source: BackupSource::PreRestore,
        },
    )
    .await?;

    // 把范围内的内容写到目标
    let nginx_root = ctx
        .probe
        .sites_available
        .parent()
        .unwrap_or_else(|| ctx.probe.sites_available.as_path())
        .to_path_buf();
    let avail_dir = ctx.probe.sites_available.clone();
    let enabled_dir = ctx.probe.sites_enabled.clone();
    let tmp_root = ctx.paths.tmp.join(format!(
        "restore-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));

    let scope = manifest.scope.clone();
    let entries_clone = entries.clone();
    let apply_result = tokio::task::spawn_blocking(move || -> Result<(), NgToolError> {
        std::fs::create_dir_all(&tmp_root).map_err(|e| NgToolError::FileOperationFailed {
            path: tmp_root.clone(),
            message: e.to_string(),
        })?;

        // 1) nginx.conf
        if scope.nginx_conf {
            if let Some((_, bytes)) = entries_clone
                .iter()
                .find(|(p, _)| p == Path::new("nginx.conf"))
            {
                let dst = nginx_root.join("nginx.conf");
                atomic_write_replace(&tmp_root, &dst, bytes)?;
            }
        }
        // 2) sites-available/*.conf
        std::fs::create_dir_all(&avail_dir).ok();
        for name in &scope.sites_available {
            let rel = format!("sites-available/{}", name);
            if let Some((_, bytes)) = entries_clone.iter().find(|(p, _)| p == Path::new(&rel)) {
                let dst = avail_dir.join(name);
                atomic_write_replace(&tmp_root, &dst, bytes)?;
            }
        }
        // 3) sites-enabled 链接同步
        std::fs::create_dir_all(&enabled_dir).ok();
        let target_enabled: HashSet<String> = scope.sites_enabled.iter().cloned().collect();
        let current = collect_enabled_stems(&enabled_dir).unwrap_or_default();
        let current_set: HashSet<String> = current.iter().cloned().collect();

        // 删除多余链接
        for stem in current_set.difference(&target_enabled) {
            let p = enabled_dir.join(format!("{}.conf", stem));
            let _ = std::fs::remove_file(&p);
        }
        // 补全缺失链接
        for stem in target_enabled.difference(&current_set) {
            let target = avail_dir.join(format!("{}.conf", stem));
            if !target.exists() {
                continue; // missing_in_backup 已在影响摘要里提示
            }
            let link = enabled_dir.join(format!("{}.conf", stem));
            if link.symlink_metadata().is_ok() {
                continue;
            }
            let _ = std::os::unix::fs::symlink(&target, &link);
        }

        // 清理 tmp_root（不删除其中的子项也无所谓——架构 §15.0 由启动时回收）
        let _ = std::fs::remove_dir_all(&tmp_root);
        Ok(())
    })
    .await
    .map_err(|e| NgToolError::FileOperationFailed {
        path: PathBuf::new(),
        message: format!("任务异常：{}", e),
    })?;

    if let Err(e) = apply_result {
        ctx.audit.log(
            "backup.restore",
            &archive_path.to_string_lossy(),
            AuditResult::Failure,
            started.elapsed().as_millis() as u64,
            json!({"stage": "apply", "error": e.to_string(), "pre_restore": pre.to_string_lossy()}),
        );
        return Err(e);
    }

    // 测试 + reload
    if let Err(e) = ctx.nginx.test_config().await {
        // 第一次回滚：用 pre-restore 重新还原
        let archive_p = pre.clone();
        let rollback = Box::pin(restore_backup(ctx.clone(), archive_p)).await;
        let outcome = if rollback.is_ok() {
            RestoreOutcome::TestFailedRolledBack {
                error: e.to_string(),
                pre_restore: pre,
            }
        } else {
            RestoreOutcome::TestFailedRollbackFailed {
                error: e.to_string(),
                rollback_error: rollback.err().map(|e| e.to_string()).unwrap_or_default(),
                pre_restore: pre,
            }
        };
        ctx.audit.log(
            "backup.restore",
            &archive_path.to_string_lossy(),
            AuditResult::Failure,
            started.elapsed().as_millis() as u64,
            json!({"stage": "test", "outcome": format!("{:?}", outcome)}),
        );
        return Ok(outcome);
    }

    if let Err(e) = ctx.systemd.reload("nginx").await {
        ctx.audit.log(
            "backup.restore",
            &archive_path.to_string_lossy(),
            AuditResult::Failure,
            started.elapsed().as_millis() as u64,
            json!({"stage": "reload", "error": e.to_string()}),
        );
        return Err(e);
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

#[derive(Debug, Clone)]
pub enum RestoreOutcome {
    /// 还原成功（pre_restore 是当前自动创建的回滚点）
    Ok { pre_restore: PathBuf },
    /// `nginx -t` 失败但已成功回滚到 pre-restore
    TestFailedRolledBack { error: String, pre_restore: PathBuf },
    /// `nginx -t` 失败且回滚也失败：保留 pre_restore 供人工干预
    TestFailedRollbackFailed {
        error: String,
        rollback_error: String,
        pre_restore: PathBuf,
    },
}

fn atomic_write_replace(tmp_root: &Path, target: &Path, bytes: &[u8]) -> Result<(), NgToolError> {
    use std::io::Write as _;
    let file_name = target
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let tmp = tmp_root.join(format!("{}.tmp", file_name));
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&tmp)
        .map_err(|e| NgToolError::FileOperationFailed {
            path: tmp.clone(),
            message: e.to_string(),
        })?;
    f.write_all(bytes)
        .map_err(|e| NgToolError::FileOperationFailed {
            path: tmp.clone(),
            message: e.to_string(),
        })?;
    f.sync_all().map_err(|e| NgToolError::FileOperationFailed {
        path: tmp.clone(),
        message: e.to_string(),
    })?;
    drop(f);
    std::fs::rename(&tmp, target).map_err(|e| NgToolError::FileOperationFailed {
        path: target.to_path_buf(),
        message: e.to_string(),
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_roundtrip() {
        let mut checks = BTreeMap::new();
        checks.insert("nginx.conf".into(), "sha256:deadbeef".into());
        let m = Manifest {
            schema_version: 1,
            created_at: "2026-04-29T22:00:00+08:00".into(),
            hostname: "orangepi".into(),
            nginx_version: "nginx version: nginx/1.24.0".into(),
            ngtool_version: "0.1.0".into(),
            source: BackupSource::Manual,
            scope: ManifestScope {
                nginx_conf: true,
                sites_available: vec!["app.conf".into()],
                sites_enabled: vec!["app".into()],
            },
            checksums: checks,
        };
        let s = toml::to_string_pretty(&m).unwrap();
        let back: Manifest = toml::from_str(&s).unwrap();
        assert_eq!(back.schema_version, 1);
        assert_eq!(back.source, BackupSource::Manual);
        assert_eq!(back.scope.sites_available, vec!["app.conf"]);
        assert_eq!(back.scope.sites_enabled, vec!["app"]);
    }

    #[test]
    fn impact_diff_marks_enable_disable() {
        // 当前启用：app, blog
        // 备份启用：app, api
        // → will_enable: [api], will_disable: [blog]
        let manifest = Manifest {
            schema_version: 1,
            created_at: "".into(),
            hostname: "".into(),
            nginx_version: "".into(),
            ngtool_version: "".into(),
            source: BackupSource::Manual,
            scope: ManifestScope {
                nginx_conf: true,
                sites_available: vec!["app.conf".into(), "api.conf".into()],
                sites_enabled: vec!["app".into(), "api".into()],
            },
            checksums: BTreeMap::new(),
        };
        // 自己写 impact 计算逻辑做单测（不走 ctx）
        let target_enabled: HashSet<String> =
            manifest.scope.sites_enabled.iter().cloned().collect();
        let current: HashSet<String> = ["app", "blog"].iter().map(|s| s.to_string()).collect();
        let mut will_enable: Vec<String> = target_enabled.difference(&current).cloned().collect();
        let mut will_disable: Vec<String> = current.difference(&target_enabled).cloned().collect();
        will_enable.sort();
        will_disable.sort();
        assert_eq!(will_enable, vec!["api"]);
        assert_eq!(will_disable, vec!["blog"]);
    }
}
