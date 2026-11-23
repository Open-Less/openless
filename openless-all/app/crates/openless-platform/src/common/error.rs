//! 平台层共享错误。
//!
//! 文本写入与按键注入都要表达"投递了一部分才失败"，所以共用同一个类型；
//! 其它能力（剪贴板、选区）用各自的错误枚举，避免把"部分失败"的语义硬塞进去。

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    /// 已经投递了一部分字符才失败；`typed_chars` 是已投递的源字符数。
    #[error("{source} after {typed_chars} chars were sent")]
    Partial {
        typed_chars: usize,
        #[source]
        source: Box<PlatformError>,
    },
    /// 合成输入被系统拒绝。
    #[error("synthetic input failed: {0}")]
    SendFailed(String),
    /// 本平台没有这条通道。
    #[error("this capability is not available on this platform")]
    Unsupported,
}

impl PlatformError {
    /// 已成功投递的源字符数；非部分失败返回 0。
    pub fn typed_chars(&self) -> usize {
        match self {
            Self::Partial { typed_chars, .. } => *typed_chars,
            _ => 0,
        }
    }

    /// 把"已投递 N 个字符"包装成部分失败；N 为 0 时原样返回。
    pub fn partial(typed_chars: usize, source: PlatformError) -> PlatformError {
        if typed_chars == 0 {
            source
        } else {
            Self::Partial {
                typed_chars,
                source: Box::new(source),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_wrapping_keeps_the_consumed_prefix() {
        let error = PlatformError::partial(7, PlatformError::SendFailed("fixture".into()));
        assert_eq!(error.typed_chars(), 7);
        assert!(error.to_string().contains("after 7 chars"));
    }

    #[test]
    fn wrapping_zero_typed_chars_does_not_change_the_error() {
        let error = PlatformError::partial(0, PlatformError::Unsupported);
        assert!(matches!(error, PlatformError::Unsupported));
    }
}
