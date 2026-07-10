//! Nginx 配置健康检查。
//!
//! 提供对站点 conf 的静态扫描与内容级修复（重复 HTTP server 块、
//! 有证书却缺 HTTPS、仅 HTTPS 却缺 HTTP 跳转等）。
//!
//! - [`scan_config_file`]：只读扫描
//! - [`fix_config_file`]：按单条问题生成修复文本（不写盘）
//! - [`apply_all_fixes`]：连续应用全部可修复问题并写回文件

use std::path::{Path, PathBuf};

/// 配置问题类型
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigIssue {
    /// 重复的 HTTP 80 server 块
    DuplicateHttpBlock { file: String, count: usize },
    /// 缺少 HTTPS 443 server 块（但有证书）
    MissingHttpsBlock {
        file: String,
        domain: String,
        cert_path: String,
    },
    /// 只有 HTTPS 块，缺少 HTTP 重定向块
    MissingHttpRedirect { file: String, domain: String },
}

impl ConfigIssue {
    pub fn description(&self) -> String {
        match self {
            ConfigIssue::DuplicateHttpBlock { file, count } => {
                format!("{}: 发现 {} 个重复的 HTTP 80 server 块", file, count)
            }
            ConfigIssue::MissingHttpsBlock { file, domain, .. } => {
                format!("{}: 域名 {} 缺少 HTTPS 443 配置", file, domain)
            }
            ConfigIssue::MissingHttpRedirect { file, domain } => {
                format!("{}: 域名 {} 缺少 HTTP 到 HTTPS 的重定向", file, domain)
            }
        }
    }

    pub fn severity(&self) -> IssueSeverity {
        match self {
            ConfigIssue::DuplicateHttpBlock { .. } => IssueSeverity::Warning,
            ConfigIssue::MissingHttpsBlock { .. } => IssueSeverity::Error,
            ConfigIssue::MissingHttpRedirect { .. } => IssueSeverity::Warning,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueSeverity {
    Warning,
    Error,
}

/// 扫描配置文件，检测问题
pub fn scan_config_file(path: &Path) -> Result<Vec<ConfigIssue>, String> {
    let content = std::fs::read_to_string(path).map_err(|e| format!("读取文件失败：{}", e))?;

    let mut issues = Vec::new();
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string();

    // 检测重复的 HTTP 80 server 块
    let http_80_count = count_server_blocks_with_listen(&content, "listen 80");
    if http_80_count > 1 {
        issues.push(ConfigIssue::DuplicateHttpBlock {
            file: file_name.clone(),
            count: http_80_count,
        });
    }

    // 检测缺失的 HTTPS 配置
    let https_443_count = count_server_blocks_with_listen(&content, "listen 443");
    let has_ssl_cert = content.contains("ssl_certificate ");

    if http_80_count > 0 && https_443_count == 0 && !has_ssl_cert {
        // 尝试提取域名
        if let Some(domain) = extract_server_name(&content) {
            // 检查是否有对应的证书
            let cert_path = format!("/etc/letsencrypt/live/{}/fullchain.pem", domain);
            if Path::new(&cert_path).exists() {
                issues.push(ConfigIssue::MissingHttpsBlock {
                    file: file_name.clone(),
                    domain: domain.clone(),
                    cert_path,
                });
            }
        }
    }

    if https_443_count > 0
        && !has_http_redirect(&content)
        && extract_server_name(&content).is_some()
    {
        if let Some(domain) = extract_server_name(&content) {
            issues.push(ConfigIssue::MissingHttpRedirect {
                file: file_name,
                domain,
            });
        }
    }

    Ok(issues)
}

/// 对单条问题生成修复后的完整配置文本（不写盘）。
pub fn fix_config_file(path: &Path, issue: &ConfigIssue) -> Result<String, String> {
    let content = std::fs::read_to_string(path).map_err(|e| format!("读取文件失败：{}", e))?;
    apply_issue_to_content(&content, issue)
}

/// 一次写盘应用结果。
#[derive(Debug, Clone)]
pub struct FixReport {
    pub path: PathBuf,
    pub applied: Vec<String>,
}

/// 扫描并依次应用全部可修复问题，原子写回原文件。
///
/// 注意：写盘后不会自动执行 `nginx -t`；调用方应在需要时自行测试/重载。
pub fn apply_all_fixes(path: &Path) -> Result<FixReport, String> {
    let issues = scan_config_file(path)?;
    if issues.is_empty() {
        return Ok(FixReport {
            path: path.to_path_buf(),
            applied: Vec::new(),
        });
    }

    let mut content =
        std::fs::read_to_string(path).map_err(|e| format!("读取文件失败：{}", e))?;
    let mut applied = Vec::new();

    for issue in &issues {
        content = apply_issue_to_content(&content, issue)?;
        applied.push(issue.description());
    }

    atomic_write(path, content.as_bytes())?;

    Ok(FixReport {
        path: path.to_path_buf(),
        applied,
    })
}

fn apply_issue_to_content(content: &str, issue: &ConfigIssue) -> Result<String, String> {
    match issue {
        ConfigIssue::DuplicateHttpBlock { .. } => remove_duplicate_http_blocks(content),
        ConfigIssue::MissingHttpsBlock {
            domain, cert_path, ..
        } => add_https_block(content, domain, cert_path),
        ConfigIssue::MissingHttpRedirect { domain, .. } => add_http_redirect(content, domain),
    }
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp = parent.join(format!(
        ".{}.ngtool-health.tmp",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("site.conf")
    ));
    std::fs::write(&tmp, bytes).map_err(|e| format!("写入临时文件失败：{}", e))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("替换配置文件失败：{}", e)
    })?;
    Ok(())
}

/// 统计包含特定 listen 指令的 server 块数量
fn count_server_blocks_with_listen(content: &str, listen_directive: &str) -> usize {
    let mut count = 0;
    let mut in_server_block = false;
    let mut brace_depth = 0;
    let mut found_listen_in_current_block = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("server {") || trimmed == "server{" {
            in_server_block = true;
            brace_depth = 1;
            found_listen_in_current_block = false;
            continue;
        }

        if in_server_block {
            // 统计大括号
            brace_depth += trimmed.matches('{').count();
            brace_depth -= trimmed.matches('}').count();

            // 检查 listen 指令
            if trimmed.starts_with(listen_directive) {
                found_listen_in_current_block = true;
            }

            // server 块结束
            if brace_depth == 0 {
                if found_listen_in_current_block {
                    count += 1;
                }
                in_server_block = false;
            }
        }
    }

    count
}

fn has_http_redirect(content: &str) -> bool {
    content.contains("listen 80") && content.contains("return 301 https://$host$request_uri")
}

/// 提取第一个 server_name 指令的值
fn extract_server_name(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("server_name ") {
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() >= 2 {
                let domain = parts[1].trim_end_matches(';');
                return Some(domain.to_string());
            }
        }
    }
    None
}

/// 移除重复的 HTTP 80 server 块（保留第一个）
fn remove_duplicate_http_blocks(content: &str) -> Result<String, String> {
    let mut result = Vec::new();
    let mut in_server_block = false;
    let mut brace_depth = 0;
    let mut current_block = Vec::new();
    let mut is_http_80_block = false;
    let mut seen_http_80_blocks: Vec<String> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("server {") || trimmed == "server{" {
            in_server_block = true;
            brace_depth = 1;
            current_block.clear();
            current_block.push(line.to_string());
            is_http_80_block = false;
            continue;
        }

        if in_server_block {
            current_block.push(line.to_string());
            brace_depth += trimmed.matches('{').count();
            brace_depth -= trimmed.matches('}').count();

            // 检查是否是 HTTP 80 块
            if trimmed.starts_with("listen 80") {
                is_http_80_block = true;
            }

            // server 块结束
            if brace_depth == 0 {
                if is_http_80_block {
                    let normalized = normalize_block(&current_block);
                    if seen_http_80_blocks.iter().any(|b| b == &normalized) {
                        // 只移除内容完全相同的 HTTP 80 块，避免误删合法 redirect/server。
                    } else {
                        seen_http_80_blocks.push(normalized);
                        result.extend(current_block.clone());
                    }
                } else {
                    // 非 HTTP 80 块，保留
                    result.extend(current_block.clone());
                }
                in_server_block = false;
                current_block.clear();
            }
        } else {
            result.push(line.to_string());
        }
    }

    Ok(result.join("\n"))
}

fn normalize_block(block: &[String]) -> String {
    block
        .iter()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 添加 HTTPS 443 server 块
fn add_https_block(content: &str, domain: &str, cert_path: &str) -> Result<String, String> {
    // 将第一个 HTTP 主 server 块升级为 HTTPS，并追加独立 HTTP redirect 块。
    let key_path = cert_path.replace("fullchain.pem", "privkey.pem");

    let ssl_config = format!(
        r#"
    ssl_certificate {};
    ssl_certificate_key {};
    include /etc/letsencrypt/options-ssl-nginx.conf;
    ssl_dhparam /etc/letsencrypt/ssl-dhparams.pem;"#,
        cert_path, key_path
    );

    let mut result = Vec::new();
    let mut in_first_server = false;
    let mut ssl_added = false;

    for line in content.lines() {
        if !ssl_added && line.trim().starts_with("server {") {
            in_first_server = true;
        }

        if in_first_server && !ssl_added && line.trim().starts_with("listen 80") {
            let indent = line
                .chars()
                .take_while(|ch| ch.is_ascii_whitespace())
                .collect::<String>();
            result.push(format!("{}listen 443 ssl;", indent));
            result.push(ssl_config.clone());
            ssl_added = true;
            in_first_server = false;
            continue;
        }

        result.push(line.to_string());
    }

    if !ssl_added {
        return Err("未找到可升级的 listen 80 server 块".into());
    }

    add_http_redirect(&result.join("\n"), domain)
}

/// 添加 HTTP 到 HTTPS 的重定向块
fn add_http_redirect(content: &str, domain: &str) -> Result<String, String> {
    let redirect_block = format!(
        r#"

server {{
    listen 80;
    server_name {};
    return 301 https://$host$request_uri;
}}
"#,
        domain
    );

    Ok(format!("{}{}", content, redirect_block))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_server_blocks() {
        let content = r#"
server {
    listen 80;
    server_name example.com;
}
server {
    listen 443 ssl;
    server_name example.com;
}
server {
    listen 80;
    server_name example.com;
}
"#;
        assert_eq!(count_server_blocks_with_listen(content, "listen 80"), 2);
        assert_eq!(count_server_blocks_with_listen(content, "listen 443"), 1);
    }

    #[test]
    fn test_extract_server_name() {
        let content = r#"
server {
    listen 80;
    server_name example.com;
}
"#;
        assert_eq!(
            extract_server_name(content),
            Some("example.com".to_string())
        );
    }

    #[test]
    fn test_remove_duplicate_http_blocks_keeps_distinct_redirect() {
        let content = r#"
server {
    listen 80;
    server_name example.com;
}
server {
    listen 80;
    server_name example.com;
}
server {
    listen 80;
    server_name example.com;
    return 301 https://$host$request_uri;
}
"#;
        let fixed = remove_duplicate_http_blocks(content).unwrap();
        assert_eq!(count_server_blocks_with_listen(&fixed, "listen 80"), 2);
        assert!(fixed.contains("return 301 https://$host$request_uri;"));
    }

    #[test]
    fn apply_all_fixes_writes_deduped_http_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.conf");
        let content = r#"
server {
    listen 80;
    server_name example.com;
}
server {
    listen 80;
    server_name example.com;
}
"#;
        std::fs::write(&path, content).unwrap();
        let report = apply_all_fixes(&path).unwrap();
        assert_eq!(report.applied.len(), 1);
        let after = std::fs::read_to_string(&path).unwrap();
        assert_eq!(count_server_blocks_with_listen(&after, "listen 80"), 1);
    }
}
