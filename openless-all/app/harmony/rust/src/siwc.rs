//! Native Asset Store access is synchronous and shared by the app and IME processes.
//! It bypasses the WebView credential write queue so rotated tokens are durable before use.
use openless_core::{
    siwc::{error, SiwcError, SiwcService, SiwcVault},
    SecretValue,
};
use std::sync::Arc;
pub fn service() -> Arc<SiwcService> {
    SiwcService::new(Arc::new(AssetVault))
}
struct AssetVault;
#[cfg(not(target_env = "ohos"))]
impl SiwcVault for AssetVault {
    fn read(&self, _: &str) -> Result<Option<SecretValue>, SiwcError> {
        Err(error("storage_unavailable"))
    }
    fn write(&self, _: &str, _: &str) -> Result<(), SiwcError> {
        Err(error("storage_unavailable"))
    }
    fn remove(&self, _: &str) -> Result<(), SiwcError> {
        Err(error("storage_unavailable"))
    }
    fn compare_head(&self, _: Option<&str>, _: &str) -> Result<bool, SiwcError> {
        Err(error("storage_unavailable"))
    }
}
#[cfg(target_env = "ohos")]
mod native {
    use super::*;
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Blob {
        size: u32,
        data: *mut u8,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    union Value {
        boolean: bool,
        number: u32,
        blob: Blob,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Attr {
        tag: u32,
        value: Value,
    }
    #[repr(C)]
    struct ResultRow {
        count: u32,
        attrs: *mut Attr,
    }
    #[repr(C)]
    struct ResultSet {
        count: u32,
        results: *mut ResultRow,
    }
    // AssetStoreKit API 11/12 declarations; no dependency on ArkTS or an active WebView.
    #[link(name = "asset_ndk.z")]
    extern "C" {
        fn OH_Asset_Add(attrs: *const Attr, count: u32) -> i32;
        fn OH_Asset_Update(query: *const Attr, count: u32, attrs: *const Attr, updates: u32)
            -> i32;
        fn OH_Asset_Remove(query: *const Attr, count: u32) -> i32;
        fn OH_Asset_Query(query: *const Attr, count: u32, result: *mut ResultSet) -> i32;
        fn OH_Asset_ParseAttr(row: *const ResultRow, tag: u32) -> *mut Attr;
        fn OH_Asset_FreeResultSet(result: *mut ResultSet);
    }
    const SECRET: u32 = 0x30000001;
    const ALIAS: u32 = 0x30000002;
    const ACCESSIBILITY: u32 = 0x20000003;
    const SYNC: u32 = 0x20000010;
    const LABEL: u32 = 0x30000030;
    const VERSION: u32 = 0x30000034;
    const RETURN: u32 = 0x20000040;
    const CONFLICT: u32 = 0x20000044;
    const MISSING: i32 = 24000002;
    const DUPLICATE: i32 = 24000003;
    fn blob(tag: u32, value: &str) -> Attr {
        Attr {
            tag,
            value: Value {
                blob: Blob {
                    size: value.len() as u32,
                    data: value.as_ptr() as *mut u8,
                },
            },
        }
    }
    fn number(tag: u32, value: u32) -> Attr {
        Attr {
            tag,
            value: Value { number: value },
        }
    }
    fn alias(key: &str) -> Result<String, SiwcError> {
        if key.is_empty()
            || key.len() > 100
            || !key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
        {
            return Err(error("storage_failed"));
        }
        Ok(format!("openless.siwc.{key}"))
    }
    impl Drop for ResultSet {
        fn drop(&mut self) {
            unsafe {
                OH_Asset_FreeResultSet(self);
            }
        }
    }
    impl SiwcVault for AssetVault {
        fn read(&self, key: &str) -> Result<Option<SecretValue>, SiwcError> {
            let alias = alias(key)?;
            let query = [blob(ALIAS, &alias), number(RETURN, 0)];
            let mut result = ResultSet {
                count: 0,
                results: std::ptr::null_mut(),
            };
            let code = unsafe { OH_Asset_Query(query.as_ptr(), query.len() as u32, &mut result) };
            if code == MISSING {
                return Ok(None);
            }
            if code != 0 {
                return Err(error("storage_unavailable"));
            }
            if result.count != 1 || result.results.is_null() {
                return Err(error("storage_corrupt"));
            }
            let secret = unsafe { OH_Asset_ParseAttr(result.results, SECRET).as_ref() }
                .ok_or_else(|| error("storage_corrupt"))?;
            let value = unsafe { secret.value.blob };
            if value.size > 1024 || value.data.is_null() {
                return Err(error("storage_corrupt"));
            }
            let bytes = unsafe { std::slice::from_raw_parts(value.data, value.size as usize) };
            Ok(Some(SecretValue::new(
                std::str::from_utf8(bytes).map_err(|_| error("storage_corrupt"))?,
            )))
        }
        fn write(&self, key: &str, value: &str) -> Result<(), SiwcError> {
            if value.len() > 1024 {
                return Err(error("storage_full"));
            }
            let alias = alias(key)?;
            let attrs = [
                blob(ALIAS, &alias),
                blob(SECRET, value),
                blob(LABEL, "openless.siwc"),
                number(ACCESSIBILITY, 2),
                number(SYNC, 0),
                number(CONFLICT, 1),
            ];
            if unsafe { OH_Asset_Add(attrs.as_ptr(), attrs.len() as u32) } != 0 {
                return Err(error("storage_failed"));
            }
            Ok(())
        }
        fn remove(&self, key: &str) -> Result<(), SiwcError> {
            let alias = alias(key)?;
            let query = [blob(ALIAS, &alias)];
            match unsafe { OH_Asset_Remove(query.as_ptr(), 1) } {
                0 | MISSING => Ok(()),
                _ => Err(error("storage_failed")),
            }
        }
        fn compare_head(&self, expected: Option<&str>, next: &str) -> Result<bool, SiwcError> {
            let alias = alias("head")?;
            // The revision is a non-secret manifest. The DB condition and head update are one operation.
            let code = if let Some(previous) = expected {
                let query = [blob(ALIAS, &alias), blob(VERSION, previous)];
                let attrs = [blob(SECRET, next), blob(VERSION, next)];
                unsafe { OH_Asset_Update(query.as_ptr(), 2, attrs.as_ptr(), 2) }
            } else {
                let attrs = [
                    blob(ALIAS, &alias),
                    blob(SECRET, next),
                    blob(VERSION, next),
                    blob(LABEL, "openless.siwc"),
                    number(ACCESSIBILITY, 2),
                    number(SYNC, 0),
                    number(CONFLICT, 1),
                ];
                unsafe { OH_Asset_Add(attrs.as_ptr(), attrs.len() as u32) }
            };
            match code {
                0 => Ok(true),
                MISSING | DUPLICATE => Ok(false),
                _ => Err(error("storage_failed")),
            }
        }
    }
}
