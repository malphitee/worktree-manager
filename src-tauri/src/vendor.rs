//! PHP vendor 复制判定（requirements.md 判定口径 4，六条件）与 lock 内容 Hash。

use std::fs;
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::copy;
use crate::error::AppError;

pub const COMPOSER_JSON: &str = "composer.json";
pub const COMPOSER_LOCK: &str = "composer.lock";
pub const VENDOR_DIR: &str = "vendor";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VendorDecision {
    /// 六条件全满足 → 应复制
    Copied,
    NotPhp,
    SourceMissing,
    LockMissing,
    LockMismatch,
    TargetExists,
}

/// `composer.lock` 的 SHA-256（十六进制小写）
pub fn lock_hash(path: &Path) -> Result<String, AppError> {
    let mut file = fs::File::open(path)
        .map_err(|error| AppError::Io(format!("读取失败 {}：{error}", path.display())))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| AppError::Io(format!("读取失败 {}：{error}", path.display())))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// 六条件判定（① 新 worktree 有 composer.json；② 源仓库有 vendor；③ 新 worktree 有 composer.lock；
/// ④ 源仓库有 composer.lock；⑤ 两份 lock SHA-256 一致；⑥ 目标没有 vendor）
pub fn assess_vendor(worktree: &Path, source_repo: &Path) -> Result<VendorDecision, AppError> {
    // ① 新 worktree 有 composer.json
    if !worktree.join(COMPOSER_JSON).is_file() {
        return Ok(VendorDecision::NotPhp);
    }
    // ② 源仓库有 vendor 目录
    if !source_repo.join(VENDOR_DIR).is_dir() {
        return Ok(VendorDecision::SourceMissing);
    }
    // ③ 新 worktree 有 composer.lock
    let worktree_lock = worktree.join(COMPOSER_LOCK);
    if !worktree_lock.is_file() {
        return Ok(VendorDecision::LockMissing);
    }
    // ④ 源仓库有 composer.lock
    let source_lock = source_repo.join(COMPOSER_LOCK);
    if !source_lock.is_file() {
        return Ok(VendorDecision::LockMissing);
    }
    // ⑤ 两份 lock 的 SHA-256 一致
    if lock_hash(&worktree_lock)? != lock_hash(&source_lock)? {
        return Ok(VendorDecision::LockMismatch);
    }
    // ⑥ 目标没有 vendor
    if worktree.join(VENDOR_DIR).exists() {
        return Ok(VendorDecision::TargetExists);
    }
    Ok(VendorDecision::Copied)
}

/// 复制源仓库 `vendor` 到新 worktree（调用方必须先通过 `assess_vendor`）
pub fn copy_vendor(worktree: &Path, source_repo: &Path) -> Result<(), AppError> {
    copy::copy_snapshot(&source_repo.join(VENDOR_DIR), &worktree.join(VENDOR_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let worktree = temp.path().join("worktree");
        let source = temp.path().join("source");
        fs::create_dir_all(&worktree).unwrap();
        fs::create_dir_all(&source).unwrap();
        (temp, worktree, source)
    }

    #[test]
    fn lock_hash_matches_known_sha256() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("composer.lock");
        fs::write(&path, "abc").unwrap();
        assert_eq!(
            lock_hash(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn not_php_without_composer_json() {
        let (_temp, worktree, source) = fixture();
        write(&source.join("composer.lock"), "{}");
        fs::create_dir_all(source.join("vendor")).unwrap();
        assert_eq!(
            assess_vendor(&worktree, &source).unwrap(),
            VendorDecision::NotPhp
        );
    }

    #[test]
    fn source_missing_without_vendor_directory() {
        let (_temp, worktree, source) = fixture();
        write(&worktree.join("composer.json"), "{}");
        write(&source.join("composer.lock"), "{}");
        assert_eq!(
            assess_vendor(&worktree, &source).unwrap(),
            VendorDecision::SourceMissing
        );
    }

    #[test]
    fn lock_missing_when_either_lock_absent() {
        let (_temp, worktree, source) = fixture();
        write(&worktree.join("composer.json"), "{}");
        fs::create_dir_all(source.join("vendor")).unwrap();
        // worktree 缺 lock
        assert_eq!(
            assess_vendor(&worktree, &source).unwrap(),
            VendorDecision::LockMissing
        );
        // 源仓库缺 lock
        write(&worktree.join("composer.lock"), "{}");
        assert_eq!(
            assess_vendor(&worktree, &source).unwrap(),
            VendorDecision::LockMissing
        );
    }

    #[test]
    fn lock_mismatch_when_hashes_differ() {
        let (_temp, worktree, source) = fixture();
        write(&worktree.join("composer.json"), "{}");
        write(&worktree.join("composer.lock"), "{\"a\":1}");
        fs::create_dir_all(source.join("vendor")).unwrap();
        write(&source.join("composer.lock"), "{\"a\":2}");
        assert_eq!(
            assess_vendor(&worktree, &source).unwrap(),
            VendorDecision::LockMismatch
        );
    }

    #[test]
    fn target_exists_when_vendor_already_present() {
        let (_temp, worktree, source) = fixture();
        write(&worktree.join("composer.json"), "{}");
        write(&worktree.join("composer.lock"), "{}");
        fs::create_dir_all(source.join("vendor")).unwrap();
        write(&source.join("composer.lock"), "{}");
        fs::create_dir_all(worktree.join("vendor")).unwrap();
        assert_eq!(
            assess_vendor(&worktree, &source).unwrap(),
            VendorDecision::TargetExists
        );
    }

    #[test]
    fn copied_when_all_six_conditions_hold_then_copy_vendor() {
        let (_temp, worktree, source) = fixture();
        write(&worktree.join("composer.json"), "{}");
        write(&worktree.join("composer.lock"), "{\"lock\":true}");
        fs::create_dir_all(source.join("vendor")).unwrap();
        write(&source.join("composer.lock"), "{\"lock\":true}");
        write(&source.join("vendor").join("autoload.php"), "<?php");
        write(&source.join("vendor").join(".git").join("HEAD"), "x");

        assert_eq!(
            assess_vendor(&worktree, &source).unwrap(),
            VendorDecision::Copied
        );
        copy_vendor(&worktree, &source).unwrap();
        assert!(worktree.join("vendor").join("autoload.php").is_file());
        assert!(!worktree.join("vendor").join(".git").exists());
    }
}
