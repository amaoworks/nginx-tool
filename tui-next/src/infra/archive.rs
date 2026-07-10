//! tar.gz 打包/解压适配，对应 architecture.md §11.7。
//!
//! 仅做底层 I/O，不知道 manifest/checksum 等业务概念。备份范围限定由 domain 层负责。

use std::fmt::Write as FmtWrite;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use sha2::{Digest, Sha256};

const MAX_ARCHIVE_ENTRIES: usize = 4096;
const MAX_ENTRY_SIZE: u64 = 16 * 1024 * 1024;
const MAX_ARCHIVE_SIZE: u64 = 64 * 1024 * 1024;

/// 把 `entries` 中的 (虚拟路径, 文件内容字节) 打成 tar.gz 写入 `out_path`。
/// `entries` 的顺序即是 archive 中的顺序；调用方自行决定 manifest 应当先于其他文件出现。
pub fn create_tar_gz(out_path: &Path, entries: &[(PathBuf, Vec<u8>)]) -> std::io::Result<()> {
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = File::create(out_path)?;
    let enc = GzEncoder::new(file, Compression::default());
    let mut tar = tar::Builder::new(enc);
    for (path, bytes) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, path, bytes.as_slice())?;
    }
    tar.into_inner()?.finish()?;
    Ok(())
}

/// 把 tar.gz 中所有条目读到内存，返回 (虚拟路径 → 内容字节) 列表。
/// 调用方自行确认大小是合理的（备份首版总量限定在数百 KB 量级）。
pub fn read_tar_gz(path: &Path) -> std::io::Result<Vec<(PathBuf, Vec<u8>)>> {
    let file = File::open(path)?;
    let dec = GzDecoder::new(file);
    let mut tar = tar::Archive::new(dec);
    let mut out = Vec::new();
    let mut total_size = 0_u64;
    for entry in tar.entries()? {
        let mut entry = entry?;
        if out.len() >= MAX_ARCHIVE_ENTRIES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "archive entry count exceeds limit",
            ));
        }
        let size = entry.size();
        if size > MAX_ENTRY_SIZE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "archive entry size exceeds limit",
            ));
        }
        total_size = total_size.checked_add(size).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "archive size overflow")
        })?;
        if total_size > MAX_ARCHIVE_SIZE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "archive total size exceeds limit",
            ));
        }
        let path = entry.path()?.into_owned();
        let mut bytes = Vec::with_capacity(size as usize);
        entry.read_to_end(&mut bytes)?;
        out.push((path, bytes));
    }
    Ok(out)
}

/// 计算字节切片的 sha256 十六进制摘要。
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest.iter() {
        let _ = write!(&mut s, "{:02x}", b);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_archive() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("test.tar.gz");
        let entries: Vec<(PathBuf, Vec<u8>)> = vec![
            (PathBuf::from("manifest.toml"), b"k = 1\n".to_vec()),
            (
                PathBuf::from("nginx.conf"),
                b"events { worker_connections 1024; }\n".to_vec(),
            ),
            (
                PathBuf::from("sites-available/app.conf"),
                b"server { listen 80; }\n".to_vec(),
            ),
        ];
        create_tar_gz(&archive, &entries).unwrap();

        let parsed = read_tar_gz(&archive).unwrap();
        assert_eq!(parsed.len(), 3);
        let names: Vec<_> = parsed
            .iter()
            .map(|(p, _)| p.to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().any(|n| n == "manifest.toml"));
        assert!(names.iter().any(|n| n == "nginx.conf"));
        assert!(names.iter().any(|n| n == "sites-available/app.conf"));
    }

    #[test]
    fn sha256_known_vector() {
        // SHA-256("abc") = ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
        let h = sha256_hex(b"abc");
        assert_eq!(
            h,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
