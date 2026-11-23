//! macOS Keychain 通用密码项原语。
//!
//! 关键优化：**复用已被授权读取返回的 item 引用**。`SecKeychainItem` 保存时如果
//! 重新 `add_generic_password`，系统会对新条目再弹一次授权框；持有旧引用直接
//! `set_password` 就不会。系统仍然拥有访问控制权，这里不改任何 ACL 或信任列表。
//!
//! 缓存按 `(service, account)` 键控，因为不同服务的 item 引用不能混用。

use std::collections::HashMap;
use std::sync::OnceLock;

use parking_lot::Mutex;
use security_framework::os::macos::{
    keychain::{SecKeychain, SecPreferencesDomain},
    keychain_item::SecKeychainItem,
    passwords::find_generic_password,
};

/// 一条已知 item 的引用状态。
#[derive(Default)]
enum ItemReference {
    /// 还没读过，不能判断是"不存在"还是"未授权"。
    #[default]
    Unloaded,
    /// 系统明确答复"没有这一项"。
    Absent,
    /// 持有可用引用。
    Known(SecKeychainItem),
}

static REFERENCES: OnceLock<Mutex<HashMap<(String, String), ItemReference>>> = OnceLock::new();

fn references() -> &'static Mutex<HashMap<(String, String), ItemReference>> {
    REFERENCES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cache_key(service: &str, account: &str) -> (String, String) {
    (service.to_string(), account.to_string())
}

fn set_reference(service: &str, account: &str, reference: ItemReference) {
    references()
        .lock()
        .insert(cache_key(service, account), reference);
}

fn platform_error(error: security_framework::base::Error) -> anyhow::Error {
    anyhow::anyhow!(error)
}

fn user_keychain() -> anyhow::Result<SecKeychain> {
    SecKeychain::default_for_domain(SecPreferencesDomain::User)
        .map_err(platform_error)
        .map_err(|error| error.context("open macOS user keychain"))
}

/// 读取一条通用密码。
///
/// - `Ok(Some(_))`：读到了内容，同时缓存 item 引用供后续写入复用；
/// - `Ok(None)`：系统明确答复该项不存在（`errSecItemNotFound`）；
/// - `Err(_)`：读失败（包括未授权），缓存被清空，后续写入不会盲目覆盖。
pub fn read(service: &str, account: &str) -> anyhow::Result<Option<String>> {
    let keychain = user_keychain()?;
    match find_generic_password(Some(&[keychain]), service, account) {
        Ok((bytes, item)) => {
            let bytes = zeroize::Zeroizing::new(bytes.to_owned());
            let raw = std::str::from_utf8(&bytes)
                .map_err(|_| anyhow::anyhow!("invalid macOS credential encoding"))?
                .to_owned();
            set_reference(service, account, ItemReference::Known(item));
            Ok(Some(raw))
        }
        // errSecItemNotFound
        Err(error) if error.code() == -25300 => {
            set_reference(service, account, ItemReference::Absent);
            Ok(None)
        }
        Err(error) => {
            set_reference(service, account, ItemReference::Unloaded);
            Err(platform_error(error)).map_err(|error| error.context("read macOS credential item"))
        }
    }
}

/// 写入一条通用密码。
///
/// 有缓存的 item 引用时直接更新它；否则先尝试读一次以取得引用（或确认不存在），
/// 再决定是"更新"还是"新建"。读失败时不会新建，避免覆盖一个无权访问的已有项。
pub fn write(service: &str, account: &str, raw: &str) -> anyhow::Result<()> {
    let unloaded = matches!(
        references().lock().get(&cache_key(service, account)),
        None | Some(ItemReference::Unloaded)
    );
    if unloaded {
        // 只有"新建的项"在首次成功保存后才没有引用；绝不能因为读不到就覆盖已有项。
        read(service, account)?;
    }

    let existing = references()
        .lock()
        .get(&cache_key(service, account))
        .map(|reference| match reference {
            ItemReference::Known(item) => Some(item.clone()),
            ItemReference::Absent => None,
            ItemReference::Unloaded => None,
        });
    let had_reference = matches!(
        references().lock().get(&cache_key(service, account)),
        Some(ItemReference::Known(_))
    );

    let mut item = existing.unwrap_or(None);
    let result = if let Some(item) = item.as_mut() {
        item.set_password(raw.as_bytes()).map_err(platform_error)
    } else {
        user_keychain()?
            .add_generic_password(service, account, raw.as_bytes())
            .map_err(platform_error)
    };

    if result.is_err() || !had_reference {
        set_reference(service, account, ItemReference::Unloaded);
    }
    result.map_err(|error| error.context("write macOS credential item"))
}

/// 丢弃缓存（例如切换账号或测试之间）。
pub fn forget(service: &str, account: &str) {
    references().lock().remove(&cache_key(service, account));
}

/// 删除一条通用密码。幂等：本来就不存在时视为成功。
pub fn remove(service: &str, account: &str) -> anyhow::Result<()> {
    forget(service, account);
    let keychain = user_keychain()?;
    match keychain.delete_generic_password(service, account) {
        Ok(()) => Ok(()),
        // errSecItemNotFound：已经不在了。
        Err(error) if error.code() == -25300 => Ok(()),
        Err(error) => Err(platform_error(error))
            .map_err(|error| error.context("remove macOS credential item")),
    }
}
