//! Linux 的光标上下文与「自动学词」的观察器。
//!
//! 两者共用同一个底座：焦点输入上下文自报的 **surrounding text**（见
//! [`crate::fcitx5::surrounding_text`]）。宿主本来就在这条输入链路里提交文字，
//! 应用自报的光标位置比遍历无障碍树（AT-SPI）可靠，也不需要额外权限。
//!
//! - 光标上下文：听写/润色开始时读一次「光标前后各一段文字」，交给 Core 拼进
//!   system prompt（`prompts::cursor_context_input`），模型就能接着用户正在写的
//!   内容润色，而不是凭空猜。
//! - 自动学词：插入完成后观察同一段文字，若用户在观察窗口内改了刚插入的文本，
//!   把「原文 → 改后」这一对交给 Core（`EditObservationSink::publish`），由 Core
//!   决定要不要进词库。
//!
//! 隐私：密码管理器与终端一律不读。这条门禁放在**读取之前**，和 Tauri 的
//! `BLOCKED_PROCESS_NAME_FRAGMENTS` 同一思路 —— 只要有一条路径没设门禁，门禁就
//! 等于不存在。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::future::BoxFuture;
use openless_core::host_document::{edit_is_within_typed_text, minimal_edit, window_from_split};
use openless_core::ports::{EditObservationAdapter, EditObservationSink, HostContextAdapter};
use openless_core::{BackendError, BackendErrorCode};

use crate::fcitx5::SurroundingText;

/// 光标上下文读取预算（字符）。与 Tauri 的 `DEFAULT_BUDGET_CHARS` 保持一致，
/// 两侧给模型的上下文量相同，提示词行为不会因平台而变。
pub const CURSOR_CONTEXT_BUDGET_CHARS: usize = 600;

/// 自动学词的采样间隔。250 ms 足够捕捉用户改字，又不会把 DBus 打满。
const OBSERVE_INTERVAL: Duration = Duration::from_millis(250);

/// 敏感应用（小写子串匹配）。命中时既不读光标上下文也不观察编辑。
///
/// 名单只做「明显是密码/终端」的判断：误伤一个编辑器可以靠用户关掉开关解决，
/// 但漏掉一个密码管理器就是把主密码写进日志/请求。
const SENSITIVE_PROGRAMS: &[&str] = &[
    "keepass",
    "1password",
    "bitwarden",
    "lastpass",
    "dashlane",
    "terminal",
    "konsole",
    "alacritty",
    "kitty",
    "wezterm",
    "xterm",
    "terminator",
    "tilix",
    "guake",
    "yakuake",
    "putty",
    "tmux",
    "screen",
];

/// `program` 是否是敏感应用（大小写不敏感的子串匹配）。
pub fn is_sensitive_program(program: &str) -> bool {
    let lowered = program.to_lowercase();
    SENSITIVE_PROGRAMS
        .iter()
        .any(|blocked| lowered.contains(blocked))
}

/// 读取 surrounding text 的抽象。生产实现走 fcitx5；测试用假实现。
pub trait SurroundingReader: Send + Sync {
    fn read(&self) -> Result<Option<SurroundingText>, BackendError>;
}

/// 生产实现：走 fcitx5 插件的 `GetSurroundingText`。
#[derive(Default)]
pub struct FcitxSurrounding;

impl SurroundingReader for FcitxSurrounding {
    fn read(&self) -> Result<Option<SurroundingText>, BackendError> {
        crate::fcitx5::surrounding_text()
    }
}

/// 把 surrounding text 折成 prompt 里的光标上下文。
///
/// 光标之后的部分先截断到预算，再交给 Core 的 `window_from_split`（它负责
/// 「两侧都空就不给上下文」的判定）。应用把光标偏移报成**字符**数；若报得比
/// 文本还长（少见但对不齐时会致命），夹到文本长度，绝不让切片 panic。
pub fn cursor_context_from(surrounding: &SurroundingText) -> Option<String> {
    let chars: Vec<char> = surrounding.text.chars().collect();
    let cursor = surrounding.cursor.min(chars.len());
    let before: String = chars[..cursor].iter().collect();
    let after: String = chars[cursor..].iter().collect();
    let window = window_from_split(&before, &after, CURSOR_CONTEXT_BUDGET_CHARS)?;
    let split = window.cursor.min(window.text.chars().count());
    let before: String = window.text.chars().take(split).collect();
    let after: String = window.text.chars().skip(split).collect();
    Some(openless_core::prompts::cursor_context_input(&before, &after))
}

/// 注入式宿主上下文适配器。
pub struct LinuxHostContextAdapter {
    reader: Arc<dyn SurroundingReader>,
}

impl Default for LinuxHostContextAdapter {
    fn default() -> Self {
        Self {
            reader: Arc::new(FcitxSurrounding),
        }
    }
}

impl LinuxHostContextAdapter {
    pub fn new(reader: Arc<dyn SurroundingReader>) -> Self {
        Self { reader }
    }
}

impl HostContextAdapter for LinuxHostContextAdapter {
    fn capture(
        &self,
        include_cursor: bool,
    ) -> BoxFuture<'static, Result<openless_core::HostContextCapture, BackendError>> {
        let reader = Arc::clone(&self.reader);
        Box::pin(async move {
            if !include_cursor {
                // 明确不读文档：连 surrounding text 都不问。
                return Ok(openless_core::HostContextCapture::default());
            }
            // DBus 调用是阻塞的，放到阻塞线程池，别占着 tokio worker。
            let surrounding = tokio::task::spawn_blocking(move || reader.read())
                .await
                .map_err(|error| {
                    BackendError::new(
                        BackendErrorCode::Platform,
                        format!("cursor context task failed: {error}"),
                    )
                })??;
            let Some(surrounding) = surrounding else {
                log::debug!("[cursor-context] status=none (application reports no surrounding text)");
                return Ok(openless_core::HostContextCapture::default());
            };
            if is_sensitive_program(&surrounding.program) {
                log::info!(
                    "[cursor-context] status=blocked program={} (sensitive application)",
                    surrounding.program
                );
                return Ok(openless_core::HostContextCapture {
                    front_app: Some(surrounding.program.clone()),
                    cursor_context: None,
                });
            }
            let context = cursor_context_from(&surrounding);
            match &context {
                Some(text) => log::info!(
                    "[cursor-context] status=ok program={} chars={}",
                    surrounding.program,
                    text.chars().count()
                ),
                None => log::info!(
                    "[cursor-context] status=empty program={}",
                    surrounding.program
                ),
            }
            Ok(openless_core::HostContextCapture {
                front_app: Some(surrounding.program),
                cursor_context: context,
            })
        })
    }
}

/// 自动学词的观察适配器。
///
/// `arm` 记下当前 surrounding text 作为基线，起一个后台线程每 [`OBSERVE_INTERVAL`]
/// 采样一次：内容变了就求最小编辑对，只有「改动落在刚插入的文本里」才上报。
pub struct LinuxEditObservationAdapter {
    reader: Arc<dyn SurroundingReader>,
    armed: Mutex<Option<Arc<AtomicBool>>>,
}

impl Default for LinuxEditObservationAdapter {
    fn default() -> Self {
        Self {
            reader: Arc::new(FcitxSurrounding),
            armed: Mutex::new(None),
        }
    }
}

impl LinuxEditObservationAdapter {
    pub fn new(reader: Arc<dyn SurroundingReader>) -> Self {
        Self {
            reader,
            armed: Mutex::new(None),
        }
    }
}

impl EditObservationAdapter for LinuxEditObservationAdapter {
    fn arm(
        &self,
        typed_text: String,
        sink: Arc<dyn EditObservationSink>,
    ) -> Result<(), BackendError> {
        self.disarm();
        if typed_text.trim().is_empty() {
            return Ok(());
        }
        let reader = Arc::clone(&self.reader);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = Arc::clone(&stop);
        let lifetime = sink.observation_duration();
        let spawned = std::thread::Builder::new()
            .name("openless-vocab-observer".into())
            .spawn(move || {
                observe_loop(reader, typed_text, sink, lifetime, stop_for_thread);
            });
        if let Err(error) = spawned {
            return Err(BackendError::new(
                BackendErrorCode::Platform,
                format!("failed to start vocabulary observer: {error}"),
            ));
        }
        *self.armed.lock().expect("observation slot poisoned") = Some(stop);
        Ok(())
    }

    fn disarm(&self) {
        let previous = self
            .armed
            .lock()
            .expect("observation slot poisoned")
            .take();
        if let Some(stop) = previous {
            stop.store(true, Ordering::Release);
        }
    }
}

fn observe_loop(
    reader: Arc<dyn SurroundingReader>,
    typed_text: String,
    sink: Arc<dyn EditObservationSink>,
    lifetime: Duration,
    stop: Arc<AtomicBool>,
) {
    let started = Instant::now();
    let mut baseline = match reader.read() {
        Ok(Some(surrounding)) if !is_sensitive_program(&surrounding.program) => surrounding.text,
        Ok(_) => return,
        Err(error) => {
            log::warn!("[vocab] observer could not read the baseline: {error}");
            return;
        }
    };
    while !stop.load(Ordering::Acquire) && started.elapsed() < lifetime {
        std::thread::sleep(OBSERVE_INTERVAL);
        if stop.load(Ordering::Acquire) {
            return;
        }
        let current = match reader.read() {
            Ok(Some(surrounding)) if !is_sensitive_program(&surrounding.program) => surrounding.text,
            Ok(_) => return,
            Err(error) => {
                log::debug!("[vocab] observer stopped reading: {error}");
                return;
            }
        };
        if current == baseline {
            continue;
        }
        let Some(edit) = minimal_edit(&baseline, &current) else {
            baseline = current;
            continue;
        };
        if !edit_is_within_typed_text(&edit, &typed_text) {
            // 用户在别处编辑：推进基线，不算「改了刚插入的话」。
            baseline = current;
            continue;
        }
        let accepted = sink.publish(edit);
        if accepted {
            baseline = current;
        } else {
            // Core 拒绝（过期代次 / 去重 / 不安全）：停止观察，别再读文档。
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openless_core::host_document::EditPair;

    fn surrounding(text: &str, cursor: usize) -> SurroundingText {
        SurroundingText {
            program: "gedit".into(),
            text: text.into(),
            cursor,
            anchor: cursor,
        }
    }

    #[test]
    fn sensitive_applications_are_blocked_case_insensitively() {
        assert!(is_sensitive_program("KeePassXC"));
        assert!(is_sensitive_program("org.gnome.Terminal"));
        assert!(is_sensitive_program("Alacritty"));
        assert!(!is_sensitive_program("gedit"));
        assert!(!is_sensitive_program("firefox"));
    }

    #[test]
    fn cursor_context_splits_at_the_reported_character_offset() {
        let text = "before|after";
        let cursor = text.chars().position(|c| c == '|').unwrap();
        let context = cursor_context_from(&surrounding(text, cursor)).expect("context");
        assert!(context.starts_with("before"));
        assert!(context.ends_with("after"));
    }

    #[test]
    fn blank_documents_produce_no_context() {
        assert!(cursor_context_from(&surrounding("   ", 0)).is_none());
    }

    #[test]
    fn an_out_of_range_cursor_is_clamped_instead_of_panicking() {
        // 应用偶尔把字节数当字符数报上来；必须夹住，不能 panic。
        let context = cursor_context_from(&surrounding("short text", 9_999)).expect("context");
        assert!(context.starts_with("short text"), "{context}");
    }

    #[test]
    fn budget_limits_how_much_text_reaches_the_model() {
        let text = "x".repeat(5_000);
        let context = cursor_context_from(&surrounding(&text, 2_500)).expect("context");
        let stripped: String = context
            .chars()
            .filter(|c| !openless_core::prompts::CURSOR_MARKER.contains(*c))
            .collect();
        assert!(
            stripped.chars().count() <= CURSOR_CONTEXT_BUDGET_CHARS + 8,
            "the budget must bound the characters, got {}",
            stripped.chars().count()
        );
    }

    /// 记录调用、按脚本返回的假 reader。
    struct ScriptedReader {
        samples: Mutex<Vec<String>>,
    }

    impl ScriptedReader {
        fn new(samples: &[&str]) -> Self {
            Self {
                samples: Mutex::new(samples.iter().map(|s| s.to_string()).collect()),
            }
        }
    }

    impl SurroundingReader for ScriptedReader {
        fn read(&self) -> Result<Option<SurroundingText>, BackendError> {
            let mut samples = self.samples.lock().unwrap();
            if samples.is_empty() {
                return Ok(None);
            }
            let text = samples.remove(0);
            let cursor = text.chars().count();
            Ok(Some(SurroundingText {
                program: "gedit".into(),
                text,
                cursor,
                anchor: cursor,
            }))
        }
    }

    struct RecordingSink {
        published: Mutex<Vec<EditPair>>,
        accept: bool,
    }

    impl EditObservationSink for RecordingSink {
        fn observation_duration(&self) -> Duration {
            Duration::from_secs(5)
        }

        fn publish(&self, edit: EditPair) -> bool {
            self.published.lock().unwrap().push(edit);
            self.accept
        }
    }

    #[test]
    fn an_edit_inside_the_inserted_text_is_published_once() {
        let reader = Arc::new(ScriptedReader::new(&[
            "the weather is nice today",
            "the weather is lovely today",
            "the weather is lovely today",
        ]));
        let sink = Arc::new(RecordingSink {
            published: Mutex::new(Vec::new()),
            accept: true,
        });
        observe_loop(
            reader as Arc<dyn SurroundingReader>,
            "weather is nice".into(),
            sink.clone() as Arc<dyn EditObservationSink>,
            Duration::from_secs(3),
            Arc::new(AtomicBool::new(false)),
        );
        let published = sink.published.lock().unwrap();
        assert_eq!(published.len(), 1, "one edit should be reported once");
        assert_eq!(published[0].source, "nice");
        assert_eq!(published[0].target, "lovely");
    }

    #[test]
    fn edits_outside_the_inserted_text_are_ignored() {
        let reader = Arc::new(ScriptedReader::new(&["start A end", "start B end"]));
        let sink = Arc::new(RecordingSink {
            published: Mutex::new(Vec::new()),
            accept: true,
        });
        observe_loop(
            reader as Arc<dyn SurroundingReader>,
            "inserted sentence".into(),
            sink.clone() as Arc<dyn EditObservationSink>,
            Duration::from_secs(1),
            Arc::new(AtomicBool::new(false)),
        );
        assert!(
            sink.published.lock().unwrap().is_empty(),
            "an edit outside the inserted text is not vocabulary"
        );
    }

    #[test]
    fn sensitive_application_stops_the_observer_before_reading() {
        let reader = Arc::new(ScriptedReader::new(&["password 123456"]));
        let sink = Arc::new(RecordingSink {
            published: Mutex::new(Vec::new()),
            accept: true,
        });
        observe_loop(
            reader as Arc<dyn SurroundingReader>,
            "password".into(),
            sink.clone() as Arc<dyn EditObservationSink>,
            Duration::from_secs(1),
            Arc::new(AtomicBool::new(false)),
        );
        assert!(sink.published.lock().unwrap().is_empty());
    }
}
