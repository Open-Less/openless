//! 系统凭据保险库：按 `(service, account)` 读写一条不透明秘文。
//!
//! 平台后端：
//! - **Windows**：凭据管理器（`keyring` crate → `CredWrite` / `CredRead`）；
//! - **macOS**：钥匙串（复用已授权 item 引用，见 [`super::keychain`]）；
//! - **Android**：由宿主提供 Keystore 信封，本层返回明确错误；
//! - 其他平台：明确不可用。
//!
//! 上层的数据模型（信封编码、分块、generation、迁移）不属于平台代码，
//! 留在内核与宿主；这里只负责"把这段字节安全地放进去/取出来"。

#[allow(unused_imports)]
use anyhow::Result;

#[cfg(target_os = "windows")]
mod imp {
    use anyhow::{Context, Result};

    /// Windows 凭据管理器。别名来自 Cargo.toml，与宿主使用同一版本同一 native 后端。
    fn entry(service: &str, account: &str) -> Result<keyring_windows::Entry> {
        keyring_windows::Entry::new(service, account)
            .map_err(|error| anyhow::anyhow!(error))
            .context("open Windows credential manager")
    }

    pub fn read(service: &str, account: &str) -> Result<Option<String>> {
        match entry(service, account)?.get_password() {
            Ok(value) => Ok(Some(value)),
            // NoEntry 是"没存过"的确定性答复，不是失败。
            Err(keyring_windows::Error::NoEntry) => Ok(None),
            Err(error) => Err(anyhow::anyhow!(error)).context("read Windows credential"),
        }
    }

    pub fn write(service: &str, account: &str, value: &str) -> Result<()> {
        entry(service, account)?
            .set_password(value)
            .map_err(|error| anyhow::anyhow!(error))
            .context("write Windows credential")
    }

    pub fn remove(service: &str, account: &str) -> Result<()> {
        match entry(service, account)?.delete_credential() {
            Ok(()) => Ok(()),
            // 删除是幂等的。
            Err(keyring_windows::Error::NoEntry) => Ok(()),
            Err(error) => Err(anyhow::anyhow!(error)).context("remove Windows credential"),
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use anyhow::Result;

    pub fn read(service: &str, account: &str) -> Result<Option<String>> {
        super::super::keychain::read(service, account)
    }

    pub fn write(service: &str, account: &str, value: &str) -> Result<()> {
        super::super::keychain::write(service, account, value)
    }

    pub fn remove(service: &str, account: &str) -> Result<()> {
        super::super::keychain::remove(service, account)
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod imp {
    use anyhow::Result;

    fn unavailable() -> anyhow::Error {
        anyhow::anyhow!(
            "system credential vault is not available on this platform; the host owns it \
             (Android: Keystore envelope, Linux: unsupported)"
        )
    }

    pub fn read(_service: &str, _account: &str) -> Result<Option<String>> {
        Err(unavailable())
    }

    pub fn write(_service: &str, _account: &str, _value: &str) -> Result<()> {
        Err(unavailable())
    }

    pub fn remove(_service: &str, _account: &str) -> Result<()> {
        Err(unavailable())
    }
}

pub use imp::{read, remove, write};
