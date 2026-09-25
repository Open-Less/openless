//! Rust-native UI localization for the Linux egui host.
//!
//! This layer deliberately mirrors the Tauri UI's language choices
//! (`system`, `zh-CN`, `zh-TW`, `en`, `ja`, `ko`) so both UIs offer the same
//! set. The source of truth / fallback is `zh-CN`, exactly like the Tauri
//! `i18n/index.ts`; all five concrete locales are bundled statically so there
//! is no network fetch and no runtime loading.
//!
//! UI text is looked up through a typed catalog rather than string-typed
//! `format!` splices so the completeness/fallback contracts are enforceable
//! and a locale switch re-renders deterministically.

use std::fmt::Display;

/// The five concrete languages the Linux egui UI supports.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Lang {
    ZhCn,
    ZhTw,
    En,
    Ja,
    Ko,
}

/// The persisted UI-locale preference. `System` means "follow the host OS
/// locale"; `Lang(lang)` is an explicit user choice, matching the Tauri
/// `setLocalePreference` model where only an explicit tag is stored.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LocalePref {
    System,
    Lang(Lang),
}

pub const LANGS: [Lang; 5] = [Lang::ZhCn, Lang::ZhTw, Lang::En, Lang::Ja, Lang::Ko];

/// JSON/wire tags, matching the Tauri `SUPPORTED_LOCALES`.
pub const FOLLOW_SYSTEM: &str = "system";

impl Lang {
    /// Canonical BCP-47-ish tag for a concrete language.
    pub fn tag(self) -> &'static str {
        match self {
            Lang::ZhCn => "zh-CN",
            Lang::ZhTw => "zh-TW",
            Lang::En => "en",
            Lang::Ja => "ja",
            Lang::Ko => "ko",
        }
    }

    /// Parse a BCP-47 tag / locale identifier into a supported language.
    /// Handles region and script suffixes (`zh-Hant-TW`, `zh_TW`, `ja_JP`…).
    pub fn parse(tag: &str) -> Option<Lang> {
        let normalized = tag.replace('-', "_").to_ascii_lowercase();
        if normalized.starts_with("zh") {
            // Traditional markers win regardless of where they appear.
            if normalized.contains("hant")
                || normalized.contains("_tw")
                || normalized.contains("_hk")
                || normalized.contains("_mo")
            {
                return Some(Lang::ZhTw);
            }
            return Some(Lang::ZhCn);
        }
        if normalized.starts_with("ja") {
            return Some(Lang::Ja);
        }
        if normalized.starts_with("ko") {
            return Some(Lang::Ko);
        }
        if normalized.starts_with("en") {
            return Some(Lang::En);
        }
        None
    }
}

impl LocalePref {
    pub fn from_tag(tag: &str) -> LocalePref {
        if tag.eq_ignore_ascii_case(FOLLOW_SYSTEM) {
            LocalePref::System
        } else if let Some(lang) = Lang::parse(tag) {
            LocalePref::Lang(lang)
        } else {
            LocalePref::System
        }
    }

    pub fn to_tag(self) -> String {
        match self {
            LocalePref::System => FOLLOW_SYSTEM.to_string(),
            LocalePref::Lang(lang) => lang.tag().to_string(),
        }
    }

    /// Resolve this preference against the running host into a concrete lang.
    /// `System` falls through to `resolve_system_lang()`, mirroring Tauri's
    /// `detectSystemLocale()`.
    pub fn resolve(self) -> Lang {
        match self {
            LocalePref::System => resolve_system_lang(),
            LocalePref::Lang(lang) => lang,
        }
    }
}

/// Resolve the host OS locale into a supported language without touching any
/// UI. Uses the same precedence as typical Linux tooling: `LC_ALL`, then
/// `LC_MESSAGES`, then `LANG`; an unparseable or unset value falls back to `en`
/// rather than guessing.
pub fn resolve_system_lang() -> Lang {
    for variable in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Ok(value) = std::env::var(variable) {
            if let Some(lang) = Lang::parse(&value) {
                return lang;
            }
        }
    }
    Lang::En
}

/// One catalog row: a stable key plus the text in all five concrete locales.
/// Index `0` is the `zh-CN` source of truth.
pub struct Msg {
    pub key: &'static str,
    pub text: [&'static str; 5],
}

/// Order helper so callers can write rows positionally and stay readable.
/// `[zh, zh_tw, en, ja, ko]` is the single canonical order used everywhere.
#[allow(dead_code)]
const fn row(
    zh: &'static str,
    zh_tw: &'static str,
    en: &'static str,
    ja: &'static str,
    ko: &'static str,
) -> [&'static str; 5] {
    [zh, zh_tw, en, ja, ko]
}

// Global catalog of every UI string the Linux egui host renders.
//
// Convention:
//   * `zh` (zh-CN) is the source of truth and is never empty.
//   * A `[&str;5]` value that is empty for a non-zh locale means "fall back to
//     zh-CN for this key" (`tr` handles that); the completeness test asserts
//     that the zh-CN column is fully populated and that every key actually
//     referenced resolves.
pub const CATALOG: &[Msg] = &[
    // ---- Shell / navigation ------------------------------------------------
    Msg {
        key: "shell.workspace",
        text: row(
            "工作台",
            "工作臺",
            "Workspace",
            "ワークスペース",
            "작업 공간",
        ),
    },
    Msg {
        key: "shell.capabilities",
        text: row("能力", "能力", "Capabilities", "機能", "기능"),
    },
    Msg {
        key: "shell.version",
        text: row(
            "版本 {}",
            "版本 {}",
            "Version {}",
            "バージョン {}",
            "버전 {}",
        ),
    },
    Msg {
        key: "nav.overview",
        text: row("概览", "概覽", "Overview", "概要", "개요"),
    },
    Msg {
        key: "nav.history",
        text: row("历史", "歷史", "History", "履歴", "기록"),
    },
    Msg {
        key: "marketplace.derivativeBadge",
        text: row("衍生自 @{}", "衍生自 @{}", "Derived from @{}", "@{} から派生", "@{}에서 파생"),
    },
    Msg {
        key: "nav.quickNote",
        text: row("速记", "速記", "Quick notes", "速記", "속기"),
    },
    Msg { key: "quickNote.kicker", text: row("速记", "速記", "Quick notes", "速記", "속기") },
    Msg { key: "quickNote.title", text: row("速记", "速記", "Quick notes", "速記", "속기") },
    Msg { key: "quickNote.desc", text: row("永久保留录音，支持回放、导出、重新转录和重新润色。", "永久保留錄音，支援回放、匯出、重新轉錄與重新潤色。", "Permanent audio with playback, export, retranscription, and repolish.", "音声を保持し、再生・書き出し・再文字起こし・再推敲に対応します。", "오디오를 영구 보관하고 재생·내보내기·재전사·다시 다듬기를 지원합니다.") },
    Msg { key: "quickNote.start", text: row("开始录音", "開始錄音", "Start recording", "録音を開始", "녹음 시작") },
    Msg { key: "quickNote.finish", text: row("完成录音", "完成錄音", "Finish recording", "録音を終了", "녹음 종료") },
    Msg { key: "quickNote.noTranscript", text: row("还没有转写内容。", "尚未有轉錄內容。", "No transcript yet.", "まだ文字起こしがありません。", "아직 전사 내용이 없습니다.") },
    Msg { key: "quickNote.recording", text: row("录音中…", "錄音中…", "Recording…", "録音中…", "녹음 중…") },
    Msg { key: "quickNote.shortcutTitle", text: row("速记快捷键", "速記快捷鍵", "Quick note shortcut", "速記ショートカット", "속기 단축키") },
    Msg { key: "quickNote.shortcutDesc", text: row("按一次开始永久录音，再按一次结束并保存。", "按一下開始永久錄音，再按一下結束並保存。", "Press once to start a permanent capture, then press again to finish.", "一度押して録音を開始し、もう一度押して保存します。", "한 번 눌러 녹음하고 다시 눌러 저장합니다.") },
    Msg {
        key: "nav.vocab",
        text: row("词典", "詞典", "Dictionary", "辞書", "사전"),
    },
    Msg {
        key: "nav.styles",
        text: row(
            "风格包",
            "風格包",
            "Style Packs",
            "スタイルパック",
            "스타일 팩",
        ),
    },
    Msg {
        key: "nav.marketplace",
        text: row(
            "风格市场",
            "風格市場",
            "Marketplace",
            "マーケット",
            "마켓",
        ),
    },
    Msg {
        key: "nav.providers",
        text: row(
            "Provider 与设置",
            "Provider 與設定",
            "Providers & Settings",
            "プロバイダーと設定",
            "프로바이더 및 설정",
        ),
    },
    Msg {
        key: "nav.models",
        text: row(
            "本地模型",
            "本機模型",
            "Local Models",
            "ローカルモデル",
            "로컬 모델",
        ),
    },
    Msg {
        key: "nav.assistant",
        text: row(
            "Less Computer",
            "Less Computer",
            "Less Computer",
            "Less Computer",
            "Less Computer",
        ),
    },
    Msg {
        key: "nav.settings",
        text: row("设置", "設定", "Settings", "設定", "설정"),
    },
    Msg {
        key: "nav.group_style",
        text: row("风格", "風格", "Style", "スタイル", "스타일"),
    },
    Msg {
        key: "nav.group_tools",
        text: row("工具", "工具", "Tools", "ツール", "도구"),
    },
    Msg {
        key: "nav.polish_mode",
        text: row(
            "润色模式",
            "潤色模式",
            "Polish mode",
            "推敲モード",
            "다듬기 모드",
        ),
    },
    Msg {
        key: "nav.translation",
        text: row("翻译", "翻譯", "Translation", "翻訳", "번역"),
    },
        Msg {
        key: "nav.selection_ask",
        text: row("划词追问", "劃詞追問", "Ask", "選択追問", "선택 질문"),
    },
    Msg {
        key: "nav.corrections",
        text: row(
            "纠错规则",
            "糾錯規則",
            "Corrections",
            "修正ルール",
            "교정 규칙",
        ),
    },
    // ---- Common controls ----------------------------------------------------
    Msg {
        key: "btn.refresh",
        text: row("刷新", "重新整理", "Refresh", "更新", "새로고침"),
    },
    Msg {
        key: "btn.retry",
        text: row("重试", "重試", "Retry", "再試行", "다시 시도"),
    },
    Msg {
        key: "btn.start",
        text: row("开始", "開始", "Start", "開始", "시작"),
    },
    Msg {
        key: "btn.stop",
        text: row("停止", "停止", "Stop", "停止", "중지"),
    },
    Msg {
        key: "btn.cancel",
        text: row("取消", "取消", "Cancel", "キャンセル", "취소"),
    },
    Msg {
        key: "btn.close",
        text: row("关闭", "關閉", "Close", "閉じる", "닫기"),
    },
    Msg {
        key: "btn.send",
        text: row("发送", "傳送", "Send", "送信", "보내기"),
    },
    Msg {
        key: "btn.insert",
        text: row("插入", "插入", "Insert", "挿入", "삽입"),
    },
    Msg {
        key: "btn.confirm_replace",
        text: row(
            "确认替换",
            "確認替換",
            "Confirm replace",
            "置換を確定",
            "바꾸기 확인",
        ),
    },
    Msg {
        key: "btn.undo",
        text: row("撤销", "復原", "Undo", "元に戻す", "실행 취소"),
    },
    Msg {
        key: "btn.run",
        text: row("运行", "執行", "Run", "実行", "실행"),
    },
    Msg {
        key: "btn.allow",
        text: row("允许", "允許", "Allow", "許可", "허용"),
    },
    Msg {
        key: "btn.deny",
        text: row("拒绝", "拒絕", "Deny", "拒否", "거부"),
    },
    Msg {
        key: "btn.end_recording",
        text: row(
            "结束录音",
            "結束錄音",
            "Stop recording",
            "録音終了",
            "녹음 종료",
        ),
    },
    Msg {
        key: "btn.stop_recording",
        text: row(
            "停止录音",
            "停止錄音",
            "Stop recording",
            "録音停止",
            "녹음 중지",
        ),
    },
    Msg {
        key: "btn.voice_ask",
        text: row(
            "语音提问",
            "語音提問",
            "Voice ask",
            "音声で質問",
            "음성 질문",
        ),
    },
    Msg {
        key: "heading.dictation",
        text: row("听写", "聽寫", "Dictation", "ディクテーション", "받아쓰기"),
    },
    Msg {
        key: "heading.qa",
        text: row("问答", "問答", "Q&A", "Q&A", "Q&A"),
    },
    Msg {
        key: "heading.overview",
        text: row("概览", "概覽", "Overview", "概要", "개요"),
    },
    Msg {
        key: "heading.selection_preview",
        text: row(
            "选区预览",
            "選區預覽",
            "Selection preview",
            "選択範囲プレビュー",
            "선택 영역 미리보기",
        ),
    },
    Msg {
        key: "heading.insert_preview",
        text: row(
            "插入预览",
            "插入預覽",
            "Insert preview",
            "挿入プレビュー",
            "삽입 미리보기",
        ),
    },
    Msg {
        key: "heading.qa_preview",
        text: row(
            "划词追问",
            "劃詞追問",
            "Ask on selection",
            "選択範囲で質問",
            "선택어 질문",
        ),
    },
    Msg {
        key: "heading.recent",
        text: row("最近识别", "最近辨識", "Recent", "最近の認識", "최근 기록"),
    },
    Msg {
        key: "heading.local_models",
        text: row(
            "本地模型",
            "本機模型",
            "Local Models",
            "ローカルモデル",
            "로컬 모델",
        ),
    },
    // ---- Dictation / empty states ------------------------------------------
    Msg {
        key: "dictation.recording",
        text: row("正在录音", "正在錄音", "Recording…", "録音中…", "녹음 중…"),
    },
    Msg {
        key: "dictation.no_transcript",
        text: row(
            "尚无转写结果",
            "尚無轉寫結果",
            "No transcription yet",
            "まだ文字起こしはありません",
            "아직 받아쓰기 결과가 없습니다",
        ),
    },
    Msg {
        key: "less_computer.done",
        text: row(
            "Less Computer 已完成",
            "Less Computer 已完成",
            "Less Computer finished",
            "Less Computer が完了しました",
            "Less Computer 완료",
        ),
    },
    Msg {
        key: "less_computer.cancelled",
        text: row(
            "Less Computer 已取消",
            "Less Computer 已取消",
            "Less Computer cancelled",
            "Less Computer をキャンセルしました",
            "Less Computer 취소됨",
        ),
    },
    Msg {
        key: "less_computer.no_output",
        text: row(
            "尚无 Agent 输出",
            "尚無 Agent 輸出",
            "No agent output yet",
            "まだエージェントの出力はありません",
            "아직 에이전트 출력이 없습니다",
        ),
    },
    Msg {
        key: "approval.submitted",
        text: row(
            "审批已提交",
            "審批已提交",
            "Approval submitted",
            "承認を送信しました",
            "승인이 제출되었습니다",
        ),
    },
    Msg {
        key: "approval.request_run",
        text: row(
            "请求执行：{}",
            "請求執行：{}",
            "Requested execution: {}",
            "実行リクエスト: {}",
            "실행 요청: {}",
        ),
    },
    Msg {
        key: "selection.replace_completed",
        text: row(
            "最近一次选区替换已完成",
            "最近一次選區替換已完成",
            "Last selection replace completed",
            "最後の選択範囲置換が完了しました",
            "마지막 선택 영역 바꾸기가 완료되었습니다",
        ),
    },
    Msg {
        key: "qa.submitted",
        text: row(
            "问答已提交",
            "問答已提交",
            "Question submitted",
            "質問を送信しました",
            "질문이 제출되었습니다",
        ),
    },
    Msg {
        key: "qa.closed",
        text: row(
            "问答已关闭",
            "問答已關閉",
            "Q&A closed",
            "Q&A を閉じました",
            "Q&A가 닫혔습니다",
        ),
    },
    Msg {
        key: "qa.recording_updated",
        text: row(
            "问答录音状态已更新",
            "問答錄音狀態已更新",
            "Q&A recording updated",
            "Q&A の録音状態を更新しました",
            "Q&A 녹음 상태가 업데이트되었습니다",
        ),
    },
    Msg {
        key: "selection.replaced",
        text: row(
            "选区替换已确认",
            "選區替換已確認",
            "Selection replace confirmed",
            "選択範囲の置換を確認しました",
            "선택 영역 바꾸기가 확인되었습니다",
        ),
    },
    Msg {
        key: "selection.cancelled",
        text: row(
            "选区替换已取消",
            "選區替換已取消",
            "Selection replace cancelled",
            "選択範囲の置換をキャンセルしました",
            "선택 영역 바꾸기가 취소되었습니다",
        ),
    },
    Msg {
        key: "selection.reverted",
        text: row(
            "选区替换已撤销",
            "選區替換已復原",
            "Selection replace reverted",
            "選択範囲の置換を元に戻しました",
            "선택 영역 바꾸기가 취소되었습니다",
        ),
    },
    Msg {
        key: "voice.cancelled",
        text: row(
            "语音会话已取消",
            "語音工作階段已取消",
            "Voice session cancelled",
            "音声セッションをキャンセルしました",
            "음성 세션이 취소되었습니다",
        ),
    },
    // ---- Overview metrics ---------------------------------------------------
    Msg {
        key: "metric.chars_today",
        text: row(
            "今日字数",
            "今日字數",
            "Chars today",
            "今日の文字数",
            "오늘 문자 수",
        ),
    },
    Msg {
        key: "metric.duration_today",
        text: row(
            "今日时长",
            "今日時長",
            "Time today",
            "今日の時間",
            "오늘 시간",
        ),
    },
    Msg {
        key: "metric.avg_latency",
        text: row(
            "平均延迟",
            "平均延遲",
            "Avg latency",
            "平均遅延",
            "평균 지연",
        ),
    },
    Msg {
        key: "metric.total",
        text: row(
            "累计记录",
            "累計記錄",
            "Total records",
            "累計記録",
            "누적 기록",
        ),
    },
    Msg {
        key: "metric.no_data_today",
        text: row(
            "今日暂无",
            "今日暫無",
            "None today",
            "今日はありません",
            "오늘 없음",
        ),
    },
    Msg {
        key: "metric.near7",
        text: row(
            "近7天 {} 段 · 近30天 {} 段",
            "近7天 {} 段 · 近30天 {} 段",
            "{} in 7d · {} in 30d",
            "直近7日 {} 件 · 30日 {} 件",
            "7일 {}건 · 30일 {}건",
        ),
    },
    Msg {
        key: "metric.total_segments",
        text: row(
            "共 {} 段",
            "共 {} 段",
            "{} segments",
            "合計 {} 件",
            "총 {}건",
        ),
    },
    Msg {
        key: "loading.overview",
        text: row(
            "正在加载概览数据…",
            "正在載入概覽資料…",
            "Loading overview…",
            "概要を読み込み中…",
            "개요를 불러오는 중…",
        ),
    },
    Msg {
        key: "overview.load_failed",
        text: row(
            "概览加载失败",
            "概覽載入失敗",
            "Failed to load overview",
            "概要の読み込みに失敗しました",
            "개요를 불러오지 못했습니다",
        ),
    },
    Msg {
        key: "overview.provider_cards",
        text: row(
            "ASR 语音识别",
            "ASR 語音辨識",
            "ASR speech",
            "ASR 音声認識",
            "ASR 음성 인식",
        ),
    },
    Msg {
        key: "overview.provider_cards_llm",
        text: row(
            "LLM 大模型",
            "LLM 大模型",
            "LLM model",
            "LLM モデル",
            "LLM 모델",
        ),
    },
    Msg {
        key: "overview.not_set",
        text: row(
            "(未设置)",
            "(未設定)",
            "(not set)",
            "(未設定)",
            "(설정 안 됨)",
        ),
    },
    Msg {
        key: "overview.configured",
        text: row("已配置", "已設定", "Configured", "設定済み", "설정됨"),
    },
    Msg {
        key: "overview.configured_dot",
        text: row(
            "● 已配置",
            "● 已設定",
            "● Configured",
            "● 設定済み",
            "● 설정됨",
        ),
    },
    Msg {
        key: "overview.unconfigured",
        text: row("未配置", "未設定", "Not configured", "未設定", "미설정"),
    },
    Msg {
        key: "overview.recent_empty",
        text: row(
            "暂无识别记录，点击上方「开始」说第一句吧。",
            "暫無辨識紀錄，點按上方「開始」說第一句吧。",
            "No recent dictation yet — hit Start above to begin.",
            "まだ認識記録はありません。上の「開始」を押してください。",
            "아직 받아쓰기 기록이 없습니다. 위의 시작을 눌러주세요.",
        ),
    },
    Msg {
        key: "overview.no_text",
        text: row(
            "(无文本)",
            "(無文字)",
            "(no text)",
            "(テキストなし)",
            "(텍스트 없음)",
        ),
    },
    Msg {
        key: "overview.heatmap_title",
        text: row(
            "近一年每日活动次数",
            "近一年每日活動次數",
            "Daily activity · past year",
            "過去1年の日別活動回数",
            "지난 1년 일별 활동 횟수",
        ),
    },
    Msg {
        key: "overview.heatmap_empty",
        text: row(
            "暂无活动数据",
            "暫無活動資料",
            "No activity data yet",
            "まだ活動データはありません",
            "아직 활동 데이터가 없습니다",
        ),
    },
    Msg {
        key: "overview.heatmap_less",
        text: row("少", "少", "Less", "少", "적음"),
    },
    Msg {
        key: "overview.heatmap_more",
        text: row("多", "多", "More", "多", "많음"),
    },
    Msg {
        key: "overview.heatmap_footnote",
        text: row(
            "（近 {} 天 · {} 天有记录）",
            "（近 {} 天 · {} 天有記錄）",
            "({} days · {} active)",
            "（{} 日間・{} 日記録あり）",
            "({}일 · {}일 기록)",
        ),
    },
    // ---- Overview page (2.0 dashboard) ------------------------------------
        Msg {
        key: "overview.title",
        text: row(
            "今日概览",
            "今日概覽",
            "Today's overview",
            "本日の概要",
            "오늘 개요",
        ),
    },
    Msg {
        key: "overview.mode_raw",
        text: row("原文", "原文", "Verbatim", "原文", "원문"),
    },
    Msg {
        key: "overview.mode_light",
        text: row(
            "轻度润色",
            "輕度潤色",
            "Light polish",
            "軽い推敲",
            "가벼운 다듬기",
        ),
    },
    Msg {
        key: "overview.mode_structured",
        text: row(
            "清晰结构",
            "清晰結構",
            "Structured",
            "明確な構造",
            "명확한 구조",
        ),
    },
    Msg {
        key: "overview.mode_formal",
        text: row(
            "正式表达",
            "正式表達",
            "Formal",
            "フォーマル",
            "격식체",
        ),
    },
        Msg {
        key: "overview.refresh",
        text: row(
            "刷新状态",
            "重新整理狀態",
            "Refresh status",
            "状態を更新",
            "상태 새로고침",
        ),
    },
        Msg {
        key: "overview.stats_title",
        text: row(
            "使用记录",
            "使用紀錄",
            "Your activity",
            "利用記録",
            "사용 기록",
        ),
    },
        Msg {
        key: "overview.metric_chars",
        text: row(
            "今日字数",
            "今日字數",
            "Characters today",
            "本日の文字数",
            "오늘 글자 수",
        ),
    },
        Msg {
        key: "overview.metric_segments",
        text: row("{} 段", "{} 段", "{} segments", "{} セグメント", "{} 세그먼트"),
    },
        Msg {
        key: "overview.metric_duration",
        text: row(
            "今日总时长",
            "今日總時長",
            "Total duration today",
            "本日の合計時間",
            "오늘 총 시간",
        ),
    },
        Msg {
        key: "overview.metric_avg",
        text: row(
            "平均段落",
            "平均段落",
            "Avg per segment",
            "平均セグメント",
            "평균 세그먼트",
        ),
    },
        Msg {
        key: "overview.metric_avg_trend",
        text: row(
            "今日均值",
            "今日均值",
            "Today's average",
            "本日の平均",
            "오늘 평균",
        ),
    },
        Msg {
        key: "overview.metric_no_data",
        text: row("暂无数据", "暫無數據", "No data", "データなし", "데이터 없음"),
    },
    Msg {
        key: "overview.history_error",
        text: row(
            "历史读取失败",
            "歷史讀取失敗",
            "Failed to load history",
            "履歴の読み込みに失敗",
            "기록을 불러오지 못함",
        ),
    },
    Msg {
        key: "overview.metric_total",
        text: row(
            "累计记录",
            "累計記錄",
            "Total records",
            "累計記録",
            "누적 기록",
        ),
    },
    Msg {
        key: "overview.metric_total_trend",
        text: row(
            "本机存档（上限 {}）",
            "本機存檔（上限 {}）",
            "Stored locally (max {})",
            "ローカル保存（上限 {}）",
            "로컬 저장(최대 {})",
        ),
    },
    Msg {
        key: "overview.period_last7",
        text: row("近 7 天", "近 7 天", "Last 7 days", "直近 7 日", "최근 7일"),
    },
    Msg {
        key: "overview.period_last30",
        text: row(
            "近 30 天",
            "近 30 天",
            "Last 30 days",
            "直近 30 日",
            "최근 30일",
        ),
    },
    Msg {
        key: "overview.daily_avg",
        text: row("日均 {}", "日均 {}", "{} / day", "1日平均 {}", "일평균 {}"),
    },
    Msg {
        key: "overview.metric_count",
        text: row("条数", "條數", "Count", "件数", "건수"),
    },
    Msg {
        key: "overview.metric_chars_name",
        text: row("字数", "字數", "Characters", "文字数", "글자 수"),
    },
    Msg {
        key: "overview.metric_duration_name",
        text: row("时长", "時長", "Duration", "時間", "시간"),
    },
        Msg {
        key: "overview.recent_title",
        text: row(
            "最近识别",
            "最近識別",
            "Recent transcripts",
            "最近の認識",
            "최근 인식",
        ),
    },
        Msg {
        key: "overview.recent_all",
        text: row("全部记录 →", "全部記錄 →", "View all →", "すべて表示 →", "전체 보기 →"),
    },
        Msg {
        key: "overview.recent_empty_hint",
        text: row(
            "还没有听写记录。跟着上方的引导试一次，结果会显示在这里。",
            "還沒有聽寫紀錄。依照上方引導試一次，結果就會顯示在這裡。",
            "No dictations yet. Follow the guide above to try one; your result will appear here.",
            "まだ音声入力の記録がありません。上の案内に沿って試すと、ここに結果が表示されます。",
            "아직 받아쓰기 기록이 없습니다. 위 안내에 따라 사용해 보면 결과가 여기에 표시됩니다.",
        ),
    },
    Msg {
        key: "overview.recent_failed",
        text: row(
            "无法读取最近识别，请重试。",
            "無法讀取最近辨識，請重試。",
            "Could not load recent items — retry.",
            "最近の認識を読み込めません。再試行してください。",
            "최근 인식을 불러오지 못했습니다. 다시 시도하세요.",
        ),
    },
    Msg {
        key: "overview.retry",
        text: row("重试", "重試", "Retry", "再試行", "다시 시도"),
    },
        Msg {
        key: "overview.activity_title",
        text: row(
            "年度活动",
            "年度活動",
            "Annual activity",
            "年間アクティビティ",
            "연간 활동",
        ),
    },
        Msg {
        key: "overview.activity_count",
        text: row(
            "{} 次听写",
            "{} 次聽寫",
            "{} dictation(s)",
            "{} 回の入力",
            "{}회 받아쓰기",
        ),
    },
    Msg {
        key: "overview.activity_error",
        text: row(
            "活动数据读取失败",
            "活動資料讀取失敗",
            "Failed to load activity",
            "アクティビティの読み込みに失敗",
            "활동 데이터를 불러오지 못함",
        ),
    },
        Msg {
        key: "overview.services_title",
        text: row(
            "当前语音服务",
            "目前的語音服務",
            "Current voice services",
            "使用中の音声サービス",
            "현재 음성 서비스",
        ),
    },
    Msg {
        key: "overview.asr_kind",
        text: row(
            "语音识别",
            "語音辨識",
            "Speech recognition",
            "音声認識",
            "음성 인식",
        ),
    },
    Msg {
        key: "overview.llm_kind",
        text: row(
            "文字处理",
            "文字處理",
            "Text processing",
            "テキスト処理",
            "텍스트 처리",
        ),
    },
        Msg {
        key: "overview.provider_help_asr",
        text: row(
            "将语音转成文字。",
            "將語音轉成文字。",
            "Turns your speech into text.",
            "音声をテキストに変換します。",
            "음성을 텍스트로 변환합니다.",
        ),
    },
        Msg {
        key: "overview.provider_help_llm",
        text: row(
            "按你的风格整理和润色文字。",
            "依照你的風格整理和潤飾文字。",
            "Organizes and polishes text in your style.",
            "あなたのスタイルに合わせて文章を整えます。",
            "내 스타일에 맞게 글을 정리하고 다듬습니다.",
        ),
    },
    Msg {
        key: "overview.configure_provider",
        text: row("去配置", "前往設定", "Configure", "設定する", "설정하기"),
    },
        Msg {
        key: "overview.manage_provider",
        text: row(
            "管理服务",
            "管理服務",
            "Manage service",
            "サービスを管理",
            "서비스 관리",
        ),
    },
    Msg {
        key: "overview.status_loading",
        text: row(
            "正在读取服务配置…",
            "正在讀取服務設定…",
            "Reading service configuration…",
            "サービス設定を読み込み中…",
            "서비스 설정을 불러오는 중…",
        ),
    },
    Msg {
        key: "overview.credentials_error",
        text: row(
            "无法读取凭据状态",
            "無法讀取憑證狀態",
            "Could not read credential status",
            "資格情報の状態を読み取れません",
            "자격 증명 상태를 읽을 수 없음",
        ),
    },
    Msg {
        key: "overview.week_days",
        text: row(
            "日|一|二|三|四|五|六",
            "日|一|二|三|四|五|六",
            "Sun|Mon|Tue|Wed|Thu|Fri|Sat",
            "日|月|火|水|木|金|土",
            "일|월|화|수|목|금|토",
        ),
    },
    Msg {
        key: "overview.months",
        text: row(
            "1月|2月|3月|4月|5月|6月|7月|8月|9月|10月|11月|12月",
            "1月|2月|3月|4月|5月|6月|7月|8月|9月|10月|11月|12月",
            "Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec",
            "1月|2月|3月|4月|5月|6月|7月|8月|9月|10月|11月|12月",
            "1월|2월|3월|4월|5월|6월|7월|8월|9월|10월|11월|12월",
        ),
    },
    Msg {
        key: "overview.minutes",
        text: row("{} 分钟", "{} 分鐘", "{} min", "{} 分", "{}분"),
    },
    Msg {
        key: "overview.hours_minutes",
        text: row(
            "{} 小时 {} 分",
            "{} 小時 {} 分",
            "{} h {} min",
            "{} 時間 {} 分",
            "{}시간 {}분",
        ),
    },
    Msg {
        key: "overview.copy",
        text: row("复制", "複製", "Copy", "コピー", "복사"),
    },
    Msg {
        key: "overview.copied",
        text: row("已复制", "已複製", "Copied", "コピー済み", "복사됨"),
    },
    // ---- Shared controls ---------------------------------------------------
        Msg {
        key: "common.refresh",
        text: row("刷新", "刷新", "Refresh", "更新", "새로고침"),
    },
        Msg {
        key: "common.clear",
        text: row("清空", "清空", "Clear", "クリア", "지우기"),
    },
        Msg {
        key: "common.loading",
        text: row("加载中…", "加載中…", "Loading…", "読み込み中…", "로딩 중…"),
    },
    Msg {
        key: "common.retry",
        text: row("重试", "重試", "Retry", "再試行", "다시 시도"),
    },
    Msg {
        key: "common.copy",
        text: row("复制", "複製", "Copy", "コピー", "복사"),
    },
        Msg {
        key: "common.copied",
        text: row("已复制", "已複製", "Copied", "コピーしました", "복사됨"),
    },
    Msg {
        key: "settings.unsupported_linux",
        text: row(
            "当前平台暂不支持此设置",
            "目前平台暫不支援此設定",
            "This setting is not available on Linux",
            "この設定は Linux では利用できません",
            "이 설정은 Linux에서 사용할 수 없습니다",
        ),
    },
    Msg {
        key: "status.copied",
        text: row("已复制", "已複製", "Copied", "コピー済み", "복사됨"),
    },
    // egui-only: the style page's pack counter and new-pack hint are not part of
    // the Tauri catalog (it renders the equivalent UI from Core data).
    Msg {
        key: "style.pack_count",
        text: row(
            "{} 个风格包",
            "{} 個風格包",
            "{} style packs",
            "{} 個のスタイルパック",
            "스타일 팩 {}개",
        ),
    },
    Msg {
        key: "style.new_pack_hint",
        text: row(
            "从模板开始创建自己的风格",
            "從範本開始建立自己的風格",
            "Start from a template",
            "テンプレートから作成",
            "템플릿에서 시작",
        ),
    },
    // egui-only: the Core vocabulary presets are loaded from the backend in the
    // Tauri app; the egui host lists the four built-ins by name.
    Msg {
        key: "vocab.presets_dev_tools",
        text: row(
            "开发工具",
            "開發工具",
            "Dev tools",
            "開発ツール",
            "개발 도구",
        ),
    },
    Msg {
        key: "vocab.presets_products",
        text: row(
            "产品与平台",
            "產品與平台",
            "Products & platforms",
            "製品とプラットフォーム",
            "제품 및 플랫폼",
        ),
    },
    Msg {
        key: "vocab.presets_terms",
        text: row(
            "技术术语",
            "技術術語",
            "Technical terms",
            "技術用語",
            "기술 용어",
        ),
    },
    Msg {
        key: "vocab.presets_english",
        text: row(
            "英文写作",
            "英文寫作",
            "English writing",
            "英語ライティング",
            "영어 작문",
        ),
    },
    // egui-only: the not-yet-wired page placeholder and the marketplace's
    // decorative preview text have no Tauri counterpart.
    Msg {
        key: "common.unsupported_title",
        text: row(
            "此页面暂未接线",
            "此頁面暫未接線",
            "This page is not wired up yet",
            "このページは未接続です",
            "이 페이지는 아직 연결되지 않았습니다",
        ),
    },
    Msg {
        key: "common.unsupported_hint",
        text: row(
            "数据桥接将在后续阶段完成",
            "資料橋接將在後續階段完成",
            "Data wiring lands in a later stage",
            "データ連携は後続の段階で完了します",
            "데이터 연결은 이후 단계에서 완료됩니다",
        ),
    },
    Msg {
        key: "marketplace.preview_placeholder",
        text: row(
            "本地占位预览\n将原始表达保留在上下文中，优化语气、结构和可读性。\n这段内容会由真实风格包提示词替换。",
            "本機預覽占位\n保留原始表達，優化語氣、結構與可讀性。\n這段內容會被實際風格包提示詞取代。",
            "Local placeholder preview\nKeeps the original wording in context while improving tone, structure and readability.\nReplaced by the real style-pack prompt.",
            "ローカル用のプレースホルダープレビュー\n原文の言い回しを保ちつつ、語調・構成・可読性を整えます。\n実際のスタイルパックのプロンプトに置き換わります。",
            "로컬 자리표시자 미리보기\n원문 표현을 유지하면서 어조, 구조, 가독성을 다듬습니다.\n실제 스타일 팩 프롬프트로 대체됩니다.",
        ),
    },
    Msg {
        key: "common.delete",
        text: row("删除", "刪除", "Delete", "削除", "삭제"),
    },
    Msg {
        key: "common.cancel",
        text: row("取消", "取消", "Cancel", "キャンセル", "취소"),
    },
    Msg {
        key: "common.confirm",
        text: row("确认", "確認", "Confirm", "確認", "확인"),
    },
        Msg {
        key: "common.duration_minutes",
        text: row("{} 分钟", "{} 分鐘", "{}m", "{} 分", "{}분"),
    },
    // ---- History -----------------------------------------------------------
        Msg {
        key: "history.kicker",
        text: row("历史记录", "歷史記錄", "HISTORY", "履歴", "기록"),
    },
    Msg {
        key: "history.title",
        text: row("历史记录", "歷史記錄", "History", "履歴", "기록"),
    },
        Msg {
        key: "history.desc",
        text: row(
            "本机保存的识别记录。",
            "本機保存的識別記錄。",
            "Locally stored transcripts.",
            "ローカルに保存された認識記録。",
            "로컬에 저장된 인식 기록.",
        ),
    },
        Msg {
        key: "history.search_placeholder",
        text: row(
            "搜索转写内容…（{}）",
            "搜尋轉寫內容…（{}）",
            "Search transcripts… ({})",
            "文字起こしを検索…（{}）",
            "기록 검색…（{}）",
        ),
    },
    Msg {
        key: "history.empty",
        text: row(
            "还没有历史记录。",
            "還沒有歷史記錄。",
            "No history yet.",
            "履歴はまだありません。",
            "아직 기록이 없습니다.",
        ),
    },
        Msg {
        key: "history.search_no_match",
        text: row(
            "没有匹配「{}」的记录。",
            "沒有符合「{}」的記錄。",
            "No entries match “{}”.",
            "「{}」に一致する項目はありません。",
            "“{}”과(와) 일치하는 항목이 없습니다.",
        ),
    },
        Msg {
        key: "history.load_failed",
        text: row(
            "加载历史失败：{}",
            "加載歷史失敗：{}",
            "Failed to load history: {}",
            "履歴の読み込みに失敗：{}",
            "기록 로드 실패: {}",
        ),
    },
        Msg {
        key: "history.select_hint",
        text: row(
            "左侧选一条查看详情。",
            "左側選一條查看詳情。",
            "Select an entry on the left to see details.",
            "左側から 1 件選択して詳細を表示。",
            "왼쪽에서 하나를 선택하여 자세히 보기.",
        ),
    },
    Msg {
        key: "history.recorded",
        text: row("录音 {}", "錄音 {}", "Recorded {}", "録音 {}", "녹음 {}"),
    },
    // egui-only: the in-app player bar toggles play/stop; the Tauri player uses
    // an icon-only button.
    Msg {
        key: "history.stop_playback",
        text: row("停止播放", "停止播放", "Stop", "停止", "정지"),
    },
    Msg {
        key: "history.play",
        text: row(
            "播放录音",
            "播放錄音",
            "Play recording",
            "録音を再生",
            "녹음 재생",
        ),
    },
    Msg {
        key: "history.export",
        text: row(
            "导出录音",
            "匯出錄音",
            "Export recording",
            "録音をエクスポート",
            "녹음 내보내기",
        ),
    },
    Msg {
        key: "history.retranscribe",
        text: row(
            "重新转写",
            "重新轉寫",
            "Retranscribe",
            "再文字起こし",
            "다시 받아쓰기",
        ),
    },
        Msg {
        key: "history.raw_label",
        text: row("原文", "原文", "Raw", "原文", "원문"),
    },
    Msg {
        key: "history.raw_empty",
        text: row("（空）", "（空）", "(empty)", "（空）", "(비어 있음)"),
    },
    Msg {
        key: "history.step_asr",
        text: row("识别", "辨識", "Transcribe", "認識", "인식"),
    },
        Msg {
        key: "history.step_polish",
        text: row("润色", "潤飾", "Polish", "推敲", "다듬기"),
    },
    Msg {
        key: "history.step_insert",
        text: row("插入", "插入", "Insert", "挿入", "삽입"),
    },
    Msg {
        key: "history.chars",
        text: row("{} 字", "{} 字", "{} chars", "{} 文字", "{}자"),
    },
        Msg {
        key: "history.vocab_hits",
        text: row(
            "{} 个热词",
            "{} 個熱詞",
            "{} vocab hits",
            "{} ホットワード",
            "핫워드 {}개",
        ),
    },
        Msg {
        key: "history.inserted",
        text: row("已插入", "已插入", "Inserted", "入力済み", "입력됨"),
    },
        Msg {
        key: "history.paste_sent",
        text: row("已尝试粘贴", "已嘗試粘貼", "Paste sent", "貼り付けを試行", "붙여넣기 시도됨"),
    },
        Msg {
        key: "history.copied_fallback",
        text: row(
            "已复制(需 {})",
            "已複製(需 {})",
            "Copied (use {})",
            "コピー済み（要 {}）",
            "복사됨({} 필요)",
        ),
    },
        Msg {
        key: "history.insert_failed",
        text: row(
            "插入失败",
            "插入失敗",
            "Insert failed",
            "入力失敗",
            "입력 실패",
        ),
    },
        Msg {
        key: "history.confirm_clear",
        text: row(
            "确定清空全部 {} 条记录？此操作不可恢复。",
            "確定清空全部 {} 條記錄？此操作不可恢復。",
            "Delete all {} history entries? This cannot be undone.",
            "全 {} 件の記録を削除しますか？この操作は取り消せません。",
            "전체 {}건의 기록을 삭제하시겠습니까? 이 작업은 되돌릴 수 없습니다.",
        ),
    },
    Msg {
        key: "history.confirm_delete",
        text: row(
            "确定删除这条记录？此操作不可恢复。",
            "確定刪除這筆記錄？此操作無法復原。",
            "Delete this record? This cannot be undone.",
            "この記録を削除しますか？元に戻せません。",
            "이 기록을 삭제할까요? 되돌릴 수 없습니다.",
        ),
    },
        Msg {
        key: "history.clear_failed",
        text: row(
            "清空失败：{}",
            "清空失敗：{}",
            "Failed to clear history: {}",
            "履歴の消去に失敗：{}",
            "기록 비우기 실패: {}",
        ),
    },
        Msg {
        key: "history.delete_failed",
        text: row(
            "删除失败：{}",
            "刪除失敗：{}",
            "Failed to delete entry: {}",
            "記録の削除に失敗：{}",
            "항목 삭제 실패: {}",
        ),
    },
        Msg {
        key: "history.copy_failed",
        text: row(
            "复制失败：{}",
            "複製失敗：{}",
            "Failed to copy: {}",
            "コピーに失敗：{}",
            "복사 실패: {}",
        ),
    },
        Msg {
        key: "history.export_failed",
        text: row(
            "导出失败：{}",
            "匯出失敗：{}",
            "Failed to export: {}",
            "エクスポート失敗：{}",
            "내보내기 실패: {}",
        ),
    },
        Msg {
        key: "history.retranscribe_failed",
        text: row(
            "重新转录失败：{}",
            "重新轉錄失敗：{}",
            "Retranscribe failed: {}",
            "再認識に失敗：{}",
            "다시 인식 실패: {}",
        ),
    },
    // ---- Durations ---------------------------------------------------------
    Msg {
        key: "dur.ms",
        text: row("{} 毫秒", "{} 毫秒", "{} ms", "{} ミリ秒", "{} 밀리초"),
    },
    Msg {
        key: "dur.sec",
        text: row("{} 秒", "{} 秒", "{} s", "{} 秒", "{} 초"),
    },
    Msg {
        key: "dur.min_sec",
        text: row(
            "{} 分 {} 秒",
            "{} 分 {} 秒",
            "{}m {}s",
            "{} 分 {} 秒",
            "{}분 {}초",
        ),
    },
    // ---- Language selector --------------------------------------------------
    Msg {
        key: "settings.language",
        text: row("界面语言", "介面語言", "UI language", "UI 言語", "UI 언어"),
    },
        Msg {
        key: "settings.language_follow_system",
        text: row(
            "跟随系统",
            "跟隨系統",
            "Follow system",
            "システムに従う",
            "시스템 따라가기",
        ),
    },
    Msg {
        key: "lang.zh-CN",
        text: row(
            "简体中文",
            "简体中文",
            "Simplified Chinese",
            "簡体中国語",
            "중국어(간체)",
        ),
    },
    Msg {
        key: "lang.zh-TW",
        text: row(
            "繁体中文",
            "繁體中文",
            "Traditional Chinese",
            "繁体中国語",
            "중국어(번체)",
        ),
    },
    Msg {
        key: "lang.en",
        text: row("English", "English", "English", "英語", "영어"),
    },
    Msg {
        key: "lang.ja",
        text: row("日本語", "日本語", "Japanese", "日本語", "일본어"),
    },
    Msg {
        key: "lang.ko",
        text: row("한국어", "한국어", "Korean", "韓国語", "한국어"),
    },
    Msg {
        key: "settings.locale_saved",
        text: row(
            "界面语言已更新",
            "介面語言已更新",
            "UI language updated",
            "UI 言語を更新しました",
            "UI 언어가 업데이트되었습니다",
        ),
    },
    // ---- Appearance (settings) ---------------------------------------------
    Msg {
        key: "settings.theme",
        text: row("主题", "佈景主題", "Theme", "テーマ", "테마"),
    },
    Msg {
        key: "theme.light",
        text: row("浅色", "淺色", "Light", "ライト", "라이트"),
    },
    Msg {
        key: "theme.dark",
        text: row("深色", "深色", "Dark", "ダーク", "다크"),
    },
    Msg {
        key: "theme.system",
        text: row(
            "系统默认",
            "系統預設",
            "System default",
            "システム既定",
            "시스템 기본",
        ),
    },
    // ---- Status / host ------------------------------------------------------
    Msg {
        key: "status.core_started",
        text: row(
            "Core 2.0 已启动",
            "Core 2.0 已啟動",
            "Core 2.0 ready",
            "Core 2.0 起動済み",
            "Core 2.0 시작됨",
        ),
    },
    Msg {
        key: "status.startup_failed",
        text: row(
            "启动失败",
            "啟動失敗",
            "Startup failed",
            "起動に失敗しました",
            "시작 실패",
        ),
    },
    // ---- Buttons (models / providers / vocab / styles / marketplace / history)
    Msg { key: "btn.create", text: row("创建", "建立", "Create", "作成", "생성") },
    Msg { key: "btn.delete", text: row("删除", "刪除", "Delete", "削除", "삭제") },
    Msg { key: "btn.confirm_delete", text: row("确认删除", "確認刪除", "Confirm delete", "削除を確認", "삭제 확인") },
    Msg { key: "btn.cancel_delete", text: row("取消删除", "取消刪除", "Cancel", "キャンセル", "삭제 취소") },
    Msg { key: "btn.enable", text: row("启用", "啟用", "Enable", "有効化", "사용") },
    Msg { key: "btn.disable", text: row("禁用", "停用", "Disable", "無効化", "사용 안 함") },
    Msg { key: "btn.move_up", text: row("上移", "上移", "Move up", "上へ移動", "위로 이동") },
    Msg { key: "btn.move_down", text: row("下移", "下移", "Move down", "下へ移動", "아래로 이동") },
    Msg { key: "btn.check_now", text: row("立即检查", "立即檢查", "Check now", "今すぐ確認", "지금 확인") },
    Msg { key: "btn.download_install", text: row("下载并安装", "下載並安裝", "Download & install", "ダウンロードしてインストール", "다운로드 및 설치") },
    Msg { key: "btn.open_releases", text: row("打开发布页", "開啟發布頁", "Open releases page", "リリースページを開く", "릴리스 페이지 열기") },
    Msg { key: "btn.accept", text: row("接受", "接受", "Accept", "承諾", "수락") },
    Msg { key: "btn.ignore", text: row("忽略", "忽略", "Ignore", "無視", "무시") },
    Msg { key: "btn.close_all", text: row("全部关闭", "全部關閉", "Dismiss all", "すべて閉じる", "모두 닫기") },
    Msg { key: "btn.apply", text: row("应用", "套用", "Apply", "適用", "적용") },
    Msg { key: "btn.hide_builtin", text: row("隐藏内置预设", "隱藏內建預設", "Hide built-in preset", "組み込みプリセットを非表示", "내장 프리셋 숨기기") },
    Msg { key: "btn.restore_builtin", text: row("恢复内置预设：{}", "還原內建預設：{}", "Restore built-in preset: {}", "組み込みプリセットを復元: {}", "내장 프리셋 복원: {}") },
    Msg { key: "btn.save_preset", text: row("保存预设", "儲存預設", "Save preset", "プリセットを保存", "프리셋 저장") },
    Msg { key: "btn.add", text: row("添加", "新增", "Add", "追加", "추가") },
    Msg { key: "btn.add_rule", text: row("添加规则", "新增規則", "Add rule", "ルールを追加", "규칙 추가") },
    Msg { key: "btn.save_style", text: row("保存风格包", "儲存風格包", "Save style pack", "スタイルパックを保存", "스타일 팩 저장") },
    Msg { key: "btn.cancel_edit", text: row("取消编辑", "取消編輯", "Cancel editing", "編集をキャンセル", "편집 취소") },
    Msg { key: "btn.new_style", text: row("新建风格包", "新增風格包", "New style pack", "新規スタイルパック", "새 스타일 팩") },
    Msg { key: "btn.import_zip", text: row("导入 ZIP", "匯入 ZIP", "Import ZIP", "ZIP をインポート", "ZIP 가져오기") },
    Msg { key: "btn.export_zip", text: row("导出 ZIP", "匯出 ZIP", "Export ZIP", "ZIP をエクスポート", "ZIP 내보내기") },
    Msg { key: "btn.preview_runtime", text: row("运行时 Prompt 预览", "執行期 Prompt 預覽", "Runtime prompt preview", "実行時プロンプトプレビュー", "실행 프롬프트 미리보기") },
    Msg { key: "btn.edit", text: row("编辑", "編輯", "Edit", "編集", "편집") },
    Msg { key: "btn.reset_builtin", text: row("恢复内置默认", "還原內建預設", "Restore built-in default", "組み込みデフォルトに戻す", "내장 기본값 복원") },
    Msg { key: "btn.set_active", text: row("设为 active", "設為 active", "Set active", "アクティブに設定", "활성 설정") },
    Msg { key: "btn.save_fields", text: row("保存字段/Secret", "儲存欄位/Secret", "Save fields/Secret", "欄位/Secret を保存", "필드/Secret 저장") },
    Msg { key: "btn.clear_secret", text: row("清除 Secret", "清除 Secret", "Clear Secret", "Secret を消去", "Secret 지우기") },
    Msg { key: "btn.validate", text: row("验证连接", "驗證連線", "Validate connection", "接続を検証", "연결 검증") },
    Msg { key: "btn.list_models", text: row("列出模型", "列出模型", "List models", "モデルを一覧表示", "모델 나열") },
    Msg { key: "btn.search_refresh", text: row("搜索/刷新", "搜尋/重新整理", "Search & refresh", "検索/更新", "검색/새로고침") },
    Msg { key: "btn.github_login", text: row("GitHub 登录", "GitHub 登入", "GitHub sign in", "GitHub にログイン", "GitHub 로그인") },
    Msg { key: "btn.logout", text: row("退出登录", "登出", "Sign out", "サインアウト", "로그아웃") },
    Msg { key: "btn.my_publish_like", text: row("我的发布/喜欢", "我的發布/喜歡", "My uploads / likes", "マイ投稿・お気に入り", "내 업로드/좋아요") },
    Msg { key: "btn.open_github", text: row("打开 GitHub", "開啟 GitHub", "Open GitHub", "GitHub を開く", "GitHub 열기") },
    Msg { key: "btn.check_auth", text: row("检查授权", "檢查授權", "Check authorization", "認証を確認", "인증 확인") },
    Msg { key: "btn.install", text: row("安装", "安裝", "Install", "インストール", "설치") },
    Msg { key: "btn.toggle_like", text: row("喜欢/取消喜欢", "喜歡/取消喜歡", "Like / unlike", "いいね/いいね解除", "좋아요/좋아요 취소") },
    Msg { key: "btn.detail", text: row("详情", "詳情", "Details", "詳細", "상세") },
    Msg { key: "btn.download_zip", text: row("下载 ZIP", "下載 ZIP", "Download ZIP", "ZIP をダウンロード", "ZIP 다운로드") },
    Msg { key: "btn.upload_update", text: row("上传/更新", "上傳/更新", "Upload / update", "アップロード/更新", "업로드/업데이트") },
    Msg { key: "btn.delete_publish", text: row("删除发布", "刪除發布", "Delete release", "リリースを削除", "배포 삭제") },
    Msg { key: "btn.clear_all", text: row("清空全部", "清空全部", "Clear all", "すべてクリア", "전체 지우기") },
    Msg { key: "btn.copy", text: row("复制", "複製", "Copy", "コピー", "복사") },
    Msg { key: "btn.repolish", text: row("重新润色", "重新潤飾", "Repolish", "再推敲", "다시 다듬기") },
    Msg { key: "btn.play_recording", text: row("播放录音", "播放錄音", "Play recording", "録音を再生", "녹음 재생") },
    Msg { key: "btn.export_recording", text: row("导出录音", "匯出錄音", "Export recording", "録音をエクスポート", "녹음 내보내기") },
    Msg { key: "btn.retranscribe", text: row("重新转写", "重新轉寫", "Retranscribe", "再文字起こし", "다시 받아쓰기") },
    Msg { key: "btn.preload_current", text: row("预加载当前模型", "預載目前模型", "Preload active model", "現在のモデルをプリロード", "현재 모델 미리 로드") },
    Msg { key: "btn.release_model", text: row("释放模型", "釋放模型", "Release model", "モデルを解放", "모델 해제") },
    Msg { key: "btn.cancel_prepare", text: row("取消准备", "取消準備", "Cancel preparation", "準備をキャンセル", "준비 취소") },
    Msg { key: "btn.download", text: row("下载", "下載", "Download", "ダウンロード", "다운로드") },
    Msg { key: "btn.cancel_download", text: row("取消下载", "取消下載", "Cancel download", "ダウンロードをキャンセル", "다운로드 취소") },
    Msg { key: "btn.verify_prepare", text: row("验证/准备", "驗證/準備", "Verify / prepare", "検証/準備", "검증/준비") },
    Msg { key: "btn.test", text: row("测试", "測試", "Test", "テスト", "테스트") },
    Msg { key: "btn.export_error_log", text: row("导出错误日志", "匯出錯誤日誌", "Export error log", "エラーログをエクスポート", "오류 로그 내보내기") },
    Msg { key: "btn.save_settings", text: row("保存设置", "儲存設定", "Save settings", "設定を保存", "설정 저장") },
    Msg { key: "btn.reset_pairing", text: row("重置配对码", "重置配對碼", "Reset pairing code", "ペアリングコードをリセット", "페어링 코드 재설정") },
    Msg { key: "btn.activate", text: row("激活", "啟用", "Activate", "アクティブ化", "활성화") },
    Msg { key: "btn.new_channel", text: row("新增渠道", "新增管道", "Add channel", "チャネルを追加", "채널 추가") },
    Msg { key: "btn.refresh_channel", text: row("刷新渠道", "重新整理管道", "Refresh channels", "チャネルを更新", "채널 새로고침") },
    Msg { key: "btn.restore_default", text: row("恢复内置默认", "還原內建預設", "Restore built-in default", "組み込みデフォルトに戻す", "내장 기본값 복원") },
    Msg { key: "btn.set_current", text: row("设为当前", "設為目前", "Set current", "現在に設定", "현재로 설정") },
    Msg { key: "btn.enable_label", text: row("启用", "啟用", "Enable", "有効化", "사용") },
    Msg { key: "btn.status_active", text: row(" · active", " · active", " · active", " · active", " · 활성") },
    Msg { key: "btn.status_disabled", text: row(" · 已禁用", " · 已停用", " · disabled", " · 無効", " · 비활성화됨") },
    // ---- Page / section headings
    Msg { key: "head.software_update", text: row("软件更新", "軟體更新", "Software update", "ソフトウェア更新", "소프트웨어 업데이트") },
    Msg { key: "head.pending_corrections", text: row("待确认的手改建议", "待確認的手動修改建議", "Pending manual corrections", "保留中の手動修正候補", "대기 중인 수동 교정 제안") },
    Msg { key: "head.vocab_presets", text: row("词汇预设", "詞彙預設", "Vocabulary presets", "語彙プリセット", "어휘 프리셋") },
    Msg { key: "head.custom_vocab", text: row("自定义词汇", "自訂詞彙", "Custom vocabulary", "カスタム語彙", "사용자 어휘") },
    Msg { key: "head.correction_rules", text: row("纠错规则", "糾錯規則", "Correction rules", "修正ルール", "교정 규칙") },
    Msg { key: "head.style_pack_editor", text: row("风格包编辑器", "風格包編輯器", "Style pack editor", "スタイルパック編集", "스타일 팩 편집기") },
    Msg { key: "head.marketplace_mine", text: row("我的 Marketplace", "我的 Marketplace", "My Marketplace", "マイマーケットプレイス", "내 마켓플레이스") },
    Msg { key: "head.publish_local", text: row("发布本地风格包", "發佈本機風格包", "Publish a local style pack", "ローカルスタイルパックを公開", "로컬 스타일 팩 배포") },
    Msg { key: "head.marketplace_detail", text: row("详情：{}", "詳情：{}", "Details: {}", "詳細: {}", "상세: {}") },
    Msg { key: "head.history_empty", text: row("历史", "歷史", "History", "履歴", "기록") },
    // ---- Update UI
    Msg { key: "update.system_managed", text: row("当前安装包由系统包管理器更新", "目前套件由系統套件管理員更新", "This build is updated by your system package manager", "このパッケージはシステムのパッケージマネージャで更新されます", "이 패키지는 시스템 패키지 관리자가 업데이트합니다") },
    Msg { key: "update.discovered", text: row("发现新版本 {}", "發現新版本 {}", "New version available: {}", "新しいバージョン: {}", "새 버전 발견: {}") },
    Msg { key: "update.up_to_date", text: row("当前已是最新版本", "目前已是最新版本", "You are up to date", "最新バージョン입니다", "최신 버전입니다") },
    Msg { key: "update.check_failed", text: row("检查更新失败：{}", "檢查更新失敗：{}", "Update check failed: {}", "更新確認に失敗: {}", "업데이트 확인 실패: {}") },
    Msg { key: "update.installed_restart", text: row("已安装 {}，请重启 OpenLess", "已安裝 {}，請重新啟動 OpenLess", "{} installed — restart OpenLess", "{} をインストールしました。OpenLess を再起動してください", "{} 설치됨 — OpenLess를 재시작하세요") },
    Msg { key: "update.install_failed", text: row("安装更新失败：{}", "安裝更新失敗：{}", "Update install failed: {}", "更新のインストールに失敗: {}", "업데이트 설치 실패: {}") },
    // ---- Settings / preferences
    Msg { key: "settings.recording_input", text: row("录音与输入", "錄音與輸入", "Recording & input", "録音と入力", "녹음 및 입력") },
    Msg { key: "settings.rec_mode", text: row("录音方式", "錄音方式", "Recording mode", "録音方式", "녹음 방식") },
    Msg { key: "recmode.toggle", text: row("切换", "切換", "Toggle", "トグル", "전환") },
    Msg { key: "recmode.hold", text: row("按住说话", "按住說話", "Push to talk", "押して話す", "누르고 말하기") },
    Msg { key: "recmode.double_click", text: row("双击", "雙擊", "Double-click", "ダブルクリック", "더블 클릭") },
    Msg { key: "recmode.auto", text: row("自动识别", "自動辨識", "Auto detect", "自動認識", "자동 인식") },
    Msg { key: "settings.auto_stop", text: row("说完后自动停止", "說畢後自動停止", "Stop automatically after silence", "無音で自動停止", "침묵 시 자동 중지") },
    Msg { key: "settings.silence_duration", text: row("连续静音时长", "連續靜音時長", "Silence timeout", "無音の継続時間", "침묵 지속 시간") },
    Msg { key: "settings.seconds", text: row("{} 秒", "{} 秒", "{} s", "{} 秒", "{}초") },
    Msg { key: "settings.microphone", text: row("麦克风", "麥克風", "Microphone", "マイク", "마이크") },
    Msg { key: "settings.system_default", text: row("系统默认", "系統預設", "System default", "システム既定", "시스템 기본") },
    Msg { key: "settings.mute_while", text: row("录音期间暂时静音系统声音", "錄音期間暫時靜音系統聲音", "Mute system audio while recording", "録音中はシステム音声をミュート", "녹음 중 시스템 소리 음소거") },
    Msg { key: "settings.cue_audio", text: row("录音开始/结束播放提示音", "錄音開始/結束播放提示音", "Play cue sounds when recording starts/stops", "録音開始/終了時に合図音を再生", "녹음 시작/종료 시 알림음 재생") },
    Msg { key: "settings.appearance", text: row("外观", "外觀", "Appearance", "外観", "모양") },
    Msg { key: "theme.follow_system", text: row("跟随系统", "跟隨系統", "Follow system", "システムに従う", "시스템 따르기") },
    Msg { key: "settings.show_heatmap", text: row("显示活动热力图", "顯示活動熱力圖", "Show activity heatmap", "活動ヒートマップを表示", "활동 히트맵 표시") },
    Msg { key: "settings.hotkeys_group", text: row("fcitx5 快捷键", "fcitx5 快速鍵", "fcitx5 shortcuts", "fcitx5 ショートカット", "fcitx5 단축키") },
    Msg { key: "settings.streaming_insert", text: row("流式插入", "串流插入", "Streaming insert", "ストリーミング挿入", "스트리밍 삽입") },
    Msg { key: "settings.enable_coding_agent", text: row("启用 Less Computer", "啟用 Less Computer", "Enable Less Computer", "Less Computer を有効化", "Less Computer 사용") },
    Msg { key: "settings.start_minimized", text: row("启动时隐藏主窗口", "啟動時隱藏主視窗", "Hide main window on launch", "起動時にメインウィンドウを非表示", "시작 시 메인 창 숨기기") },
    Msg { key: "settings.launch_at_login", text: row("开机启动", "開機啟動", "Launch at login", "ログイン時に起動", "로그인 시 실행") },
    Msg { key: "settings.auto_update", text: row("自动检查更新", "自動檢查更新", "Automatically check for updates", "自動更新確認", "자동 업데이트 확인") },
    Msg { key: "settings.update_channel", text: row("更新渠道", "更新管道", "Update channel", "更新チャネル", "업데이트 채널") },
    Msg { key: "channel.stable", text: row("稳定版", "穩定版", "Stable", "安定版", "안정판") },
    Msg { key: "settings.enable_remote", text: row("启用远程输入", "啟用遠端輸入", "Enable remote input", "リモート入力を有効化", "원격 입력 사용") },
    Msg { key: "settings.port", text: row("端口 ", "連接埠 ", "Port ", "ポート ", "포트 ") },
    // ---- Hotkey control labels
    Msg { key: "hotkey.dictation", text: row("听写", "聽寫", "Dictation", "ディクテーション", "받아쓰기") },
    Msg { key: "hotkey.translation", text: row("翻译修饰键", "翻譯修飾鍵", "Translate modifier", "翻訳修飾キー", "번역 수정자") },
    Msg { key: "hotkey.selection_polish", text: row("选区润色", "選區潤飾", "Polish selection", "選択範囲の推敲", "선택 다듬기") },
    Msg { key: "hotkey.switch_style", text: row("切换风格", "切換風格", "Switch style", "スタイル切替", "스타일 전환") },
    Msg { key: "hotkey.open_app", text: row("打开应用", "開啟應用", "Open app", "アプリを開く", "앱 열기") },
    Msg { key: "hotkey.coding_agent", text: row("Coding Agent 语音", "Coding Agent 語音", "Coding Agent voice", "Coding Agent 音声", "Coding Agent 음성") },
    Msg { key: "hotkey.enable", text: row("启用{}", "啟用{}", "Enable {}", "{} を有効化", "{} 사용") },
    // ---- Remote input
    Msg { key: "remote.running", text: row("远程输入：运行中", "遠端輸入：執行中", "Remote input: running", "リモート入力: 実行中", "원격 입력: 실행 중") },
    Msg { key: "remote.starting", text: row("远程输入：启动中", "遠端輸入：啟動中", "Remote input: starting", "リモート入力: 起動中", "원격 입력: 시작 중") },
    Msg { key: "remote.stopped", text: row("远程输入：已停止", "遠端輸入：已停止", "Remote input: stopped", "リモート入力: 停止中", "원격 입력: 중지됨") },
    Msg { key: "remote.lang_conns", text: row("语言：{} · 连接数：{}", "語言：{} · 連線數：{}", "Language: {} · Connections: {}", "言語: {} · 接続数: {}", "언어: {} · 연결 수: {}") },
    Msg { key: "lbl.status_colon", text: row("状态：{}", "狀態：{}", "Status: {}", "状態: {}", "상태: {}") },
    Msg { key: "lbl.search", text: row("搜索", "搜尋", "Search", "検索", "검색") },
    Msg { key: "lbl.name", text: row("名称", "名稱", "Name", "名前", "이름") },
    Msg { key: "lbl.version", text: row("版本", "版本", "Version", "バージョン", "버전") },
    Msg { key: "lbl.description", text: row("描述", "描述", "Description", "説明", "설명") },
    Msg { key: "lbl.base_mode", text: row("基础模式", "基礎模式", "Base mode", "基本モード", "기본 모드") },
    Msg { key: "lbl.phrase", text: row("词语", "詞語", "Phrase", "語句", "단어") },
    Msg { key: "lbl.note", text: row("备注", "備註", "Note", "メモ", "메모") },
    Msg { key: "lbl.dictation_prompt", text: row("听写 Prompt", "聽寫 Prompt", "Dictation prompt", "ディクテーションプロンプト", "받아쓰기 프롬프트") },
    Msg { key: "lbl.selection_prompt", text: row("选区 Prompt（留空则使用 Core 默认）", "選區 Prompt（留空則使用 Core 預設）", "Selection prompt (blank uses Core default)", "選択範囲プロンプト（空欄は Core 既定）", "선택 프롬프트(비우면 Core 기본값)") },
    Msg { key: "lbl.current", text: row("当前", "目前", "Current", "現在", "현재") },
    Msg { key: "lbl.primary", text: row("主键", "主鍵", "Primary key", "主キー", "주 키") },
    Msg { key: "lbl.modifiers", text: row("修饰键（+ 分隔）", "修飾鍵（+ 分隔）", "Modifiers (separate with +)", "修飾キー（+ で区切る）", "수정자(+로 구분)") },
    Msg { key: "lbl.device_code", text: row("设备码：{}", "裝置碼：{}", "Device code: {}", "デバイスコード: {}", "기기 코드: {}") },
    Msg { key: "lbl.liked", text: row("喜欢的风格：{}", "喜歡的風格：{}", "Liked style packs: {}", "いいねしたスタイルパック: {}", "좋아요한 스타일 팩: {}") },
    Msg { key: "lbl.like_dl", text: row("喜欢 {} · 下载 {} · {}", "喜歡 {} · 下載 {} · {}", "{} likes · {} downloads · {}", "いいね {} · DL {} · {}", "좋아요 {} · 다운로드 {} · {}") },
    Msg { key: "lbl.published", text: row("已发布：{}", "已發佈：{}", "Published: {}", "公開済み: {}", "배포됨: {}") },
    Msg { key: "lbl.hits", text: row("命中 {}", "命中 {}", "Hits: {}", "ヒット数: {}", "적중: {}") },
    Msg { key: "lbl.preset_count", text: row("{} 个词", "{} 個詞", "{} phrases", "{} 語句", "{}개 단어") },
    Msg { key: "lbl.preset_note", text: row("预设由 Core 合并内置版本、用户覆盖和自定义内容。", "預設由 Core 合併內建版本、使用者覆蓋與自訂內容。", "Presets merge Core built-ins, user overrides and custom entries.", "プリセットは Core の内蔵版・ユーザー上書き・カスタムを統合します。", "프리셋은 Core 내장, 사용자 덮어쓰기, 사용자 지정을 통합합니다.") },
    Msg { key: "lbl.new_custom_preset", text: row("新建自定义预设", "新增自訂預設", "New custom preset", "新規カスタムプリセット", "새 사용자 프리셋") },
    Msg { key: "lbl.new_style_default", text: row("新风格", "新風格", "New style", "新規スタイル", "새 스타일") },
    Msg { key: "hint.preset_phrases", text: row("每行或逗号分隔一个词", "每行或逗號分隔一個詞", "One phrase per line or comma-separated", "各行に1語句、またはカンマ区切り", "한 줄에 하나 또는 쉼표로 구분") },
    Msg { key: "lbl.author_version", text: row("作者：{} · 版本 {}", "作者：{} · 版本 {}", "By {} · v{}", "作者: {} · バージョン {}", "작성자: {} · 버전 {}") },
    Msg { key: "lbl.style_note", text: row("风格包数据直接来自 Core repository；运行时 Prompt 由 Core 组合。", "風格包資料直接來自 Core repository；執行期 Prompt 由 Core 組合。", "Style packs come directly from the Core repository; Core composes runtime prompts.", "スタイルパックは Core リポジトリ由来で、実行時プロンプトは Core が組み立てます。", "스타일 팩은 Core 저장소에서 오며 Core가 실행 프롬프트를 구성합니다.") },
    Msg { key: "lbl.direct_hotkey", text: row("风格包直达快捷键", "風格包直達快速鍵", "Direct style-pack shortcut", "スタイルパック直接ショートカット", "스타일 팩 직접 단축키") },
    Msg { key: "lbl.choose_style", text: row("选择风格包", "選擇風格包", "Select a style pack", "スタイルパックを選択", "스타일 팩 선택") },
    Msg { key: "btn.save_direct_hotkey", text: row("保存直达快捷键", "儲存直達快速鍵", "Save direct shortcut", "直接ショートカットを保存", "직접 단축키 저장") },
    Msg { key: "btn.remove_direct_hotkey", text: row("移除直达快捷键", "移除直達快速鍵", "Remove direct shortcut", "直接ショートカットを削除", "직접 단축키 제거") },
    Msg { key: "lbl.choose_provider", text: row("选择 Provider", "選擇 Provider", "Select a provider", "プロバイダーを選択", "프로바이더 선택") },
    Msg { key: "providers.credentials", text: row("凭据渠道", "憑證管道", "Credential channels", "資格情報チャネル", "자격 증명 채널") },
    Msg { key: "providers.core_note", text: row("Provider 类型、默认 Endpoint/Model 与鉴权要求均来自 Core descriptor。", "Provider 類型、預設 Endpoint/Model 與鑑權要求皆來自 Core descriptor。", "Provider type, default Endpoint/Model and auth requirements come from the Core descriptor.", "Provider 種別・既定 Endpoint/Model・認証要件は Core descriptor 由来です。", "Provider 유형, 기본 Endpoint/Model 및 인증 요구 사항은 Core descriptor에서 옵니다.") },
    Msg { key: "providers.empty", text: row("尚无渠道；先从上方 Core Provider 列表创建一个。", "尚無管道；請先從上方 Core Provider 清單建立一個。", "No channels yet — create one from the Core provider list above.", "チャネルがありません。上の Core Provider 一覧から作成してください。", "채널이 없습니다. 위 Core Provider 목록에서 생성하세요.") },
    Msg { key: "providers.loading_dir", text: row("正在读取 Core 渠道目录…", "正在讀取 Core 管道目錄…", "Reading the Core channel catalog…", "Core チャネル目録を読み込み中…", "Core 채널 목록을 읽는 중…") },
    Msg { key: "providers.reading_channel", text: row("正在读取 {} 渠道 {}…", "正在讀取 {} 管道 {}…", "Reading {} channel {}…", "{} チャネル {} を読み込み中…", "{} 채널 {} 읽는 중…") },
    Msg { key: "providers.editing", text: row("编辑渠道 {}", "編輯管道 {}", "Edit channel {}", "チャネル {} を編集", "채널 {} 편집") },
    Msg { key: "providers.auth_probe", text: row("鉴权：{} · 探针：{}", "鑑權：{} · 探測：{}", "Auth: {} · Probe: {}", "認証: {} · プローブ: {}", "인증: {} · 프로브: {}") },
    Msg { key: "providers.name", text: row("名称", "名稱", "Name", "名前", "이름") },
    Msg { key: "providers.model_list", text: row("模型列表（点击填入）：", "模型清單（點擊填入）：", "Models (click to fill):", "モデル一覧（クリックで入力）", "모델 목록(클릭하여 입력)") },
    Msg { key: "providers.no_cloud_note", text: row("此 Provider 不使用云凭据；模型由本地模型面板管理。", "此 Provider 不使用雲端憑證；模型由本機模型面板管理。", "This provider uses no cloud credentials; models are managed in Local Models.", "この Provider はクラウド資格情報を使いません。モデルはローカルモデルで管理します。", "이 프로바이더는 클라우드 자격 증명을 사용하지 않습니다. 모델은 로컬 모델에서 관리합니다.") },
    Msg { key: "providers.oauth_note", text: row("此 Provider 使用 OAuth；Linux egui 不读取或显示 OAuth token。", "此 Provider 使用 OAuth；Linux egui 不讀取或顯示 OAuth token。", "This provider uses OAuth; the Linux egui UI never reads or shows the OAuth token.", "この Provider は OAuth を使用します。Linux egui は OAuth トークンを読み取らず表示もしません。", "이 프로바이더는 OAuth를 사용합니다. Linux egui는 OAuth 토큰을 읽거나 표시하지 않습니다.") },
    Msg { key: "providers.api_key_hint", text: row("API Key（留空表示不修改）", "API Key（留空表示不修改）", "API Key (blank leaves unchanged)", "API Key（空欄なら変更しない）", "API Key(비우면 변경 안 함)") },
    Msg { key: "auth.none", text: row("无需 Secret", "無需 Secret", "No secret", "Secret 不要", "Secret 불필요") },
    Msg { key: "auth.api_key", text: row("API Key", "API Key", "API Key", "API キー", "API 키") },
    Msg { key: "auth.endpoint_model_optional", text: row("Endpoint + Model，API Key 可选", "Endpoint + Model，API Key 可選", "Endpoint + Model, optional API Key", "Endpoint + Model、API Key は任意", "Endpoint + Model, API Key 선택") },
    Msg { key: "auth.api_key_unless_custom", text: row("公共 Endpoint 需要 API Key；自建 Endpoint 可无 Key", "公共 Endpoint 需要 API Key；自建 Endpoint 可無 Key", "Public endpoints need an API Key; self-hosted may omit it", "公開 Endpoint は API Key が必要。自前 Endpoint は不要", "공개 엔드포인트는 API 키 필요, 자체 엔드포인트는 불필요") },
    Msg { key: "auth.volcengine", text: row("火山引擎凭据", "火山引擎憑證", "Volcengine credentials", "Volcengine 資格情報", "Volcengine 자격 증명") },
    Msg { key: "auth.xfyun", text: row("讯飞 AppID + API Key", "訊飛 AppID + API Key", "iFlytek AppID + API Key", "讯飛 AppID + API Key", "iFlytek AppID + API Key") },
    Msg { key: "auth.oauth", text: row("OAuth", "OAuth", "OAuth", "OAuth", "OAuth") },
    // ---- Empty / info labels
    Msg { key: "history.failed", text: row("失败", "失敗", "Failed", "失敗", "실패") },
    Msg { key: "history.not_requested", text: row("未请求插入", "未請求插入", "Not requested", "挿入未要求", "삽입 요청 안 됨") },
    Msg { key: "models.loading_dir", text: row("正在加载模型目录…", "正在載入模型目錄…", "Loading the model catalog…", "モデル目録を読み込み中…", "모델 목록을 불러오는 중…") },
    Msg { key: "models.empty", text: row("模型目录未返回任何可用模型", "模型目錄未傳回任何可用模型", "No usable models were returned", "利用可能なモデルがありません", "사용 가능한 모델이 없습니다") },
    Msg { key: "models.installed", text: row("已安装", "已安裝", "Installed", "インストール済み", "설치됨") },
    Msg { key: "models.not_installed", text: row("未安装", "未安裝", "Not installed", "未インストール", "미설치") },
    Msg { key: "marketplace.not_loaded", text: row("尚未加载 Marketplace；点击“搜索/刷新”。", "尚未載入 Marketplace；點按「搜尋/重新整理」。", "Marketplace not loaded yet — use Search & refresh.", "Marketplace は未読込です。「検索/更新」を押してください。", "마켓플레이스가 아직 로드되지 않았습니다. 검색/새로고침을 누르세요.") },
    // ---- Status / toast messages
    Msg { key: "status.done_chars", text: row("完成：{} 字", "完成：{} 字", "Done: {} chars", "完了：{} 文字", "완료: {} 글자") },
    Msg { key: "status.dictation_cancelled", text: row("听写已取消", "聽寫已取消", "Dictation cancelled", "ディクテーションをキャンセル", "받아쓰기 취소됨") },
    Msg { key: "status.auto_stopped", text: row("录音已自动结束", "錄音已自動結束", "Recording auto-stopped", "録音を自動終了", "녹음 자동 종료") },
    Msg { key: "status.less_compacted", text: row("Less Computer 已压缩上下文", "Less Computer 已壓縮上下文", "Less Computer compacted context", "Less Computer がコンテキストを圧縮", "Less Computer 컨텍스트 압축됨") },
    Msg { key: "status.less_waiting", text: row("Less Computer 等待审批", "Less Computer 等待審批", "Less Computer awaiting approval", "Less Computer が承認待ち", "Less Computer 승인 대기 중") },
    Msg { key: "status.less_tool", text: row("Less Computer 正在使用工具：{}", "Less Computer 正在使用工具：{}", "Less Computer is using a tool: {}", "Less Computer がツールを使用中: {}", "Less Computer 도구 사용 중: {}") },
    Msg { key: "status.less_running", text: row("Less Computer 正在运行", "Less Computer 正在執行", "Less Computer is running", "Less Computer 実行中", "Less Computer 실행 중") },
    Msg { key: "status.provider_models_loaded", text: row("已读取 {} 个模型", "已讀取 {} 個模型", "Loaded {} models", "{} 個のモデルを読み込み", "모델 {}개 로드됨") },
    Msg { key: "status.marketplace_loaded", text: row("Marketplace 已加载 {} 个风格包", "Marketplace 已載入 {} 個風格包", "Marketplace loaded {} style packs", "Marketplace が {} 個のスタイルパックを読込", "마켓플레이스 스타일 팩 {}개 로드됨") },
    Msg { key: "status.device_code", text: row("GitHub 设备码：{}", "GitHub 裝置碼：{}", "GitHub device code: {}", "GitHub デバイスコード: {}", "GitHub 기기 코드: {}") },
    Msg { key: "status.logged_in", text: row("Marketplace 已登录：{}", "Marketplace 已登入：{}", "Marketplace signed in as {}", "Marketplace に {} でログイン", "마켓플레이스에 {}로 로그인됨") },
    Msg { key: "status.logout_done", text: row("Marketplace 已退出登录", "Marketplace 已登出", "Marketplace signed out", "Marketplace をサインアウト", "마켓플레이스 로그아웃됨") },
    Msg { key: "status.oauth_pending", text: row("GitHub 授权仍在等待", "GitHub 授權仍在等待", "Waiting for GitHub authorization", "GitHub の認可を待機中", "GitHub 인가 대기 중") },
    Msg { key: "status.oauth_slowdown", text: row("GitHub 要求降低检查频率", "GitHub 要求降低檢查頻率", "GitHub asks you to slow down checks", "GitHub が確認頻度を下げるよう求めています", "GitHub가 확인 빈도를 낮추라고 요청함") },
    Msg { key: "status.detail_loaded", text: row("已加载风格详情：{}", "已載入風格詳情：{}", "Loaded style details: {}", "スタイル詳細を読込: {}", "스타일 상세 로드됨: {}") },
    Msg { key: "status.my_publish_likes", text: row("我的发布 {} 个，喜欢 {} 个", "我的發布 {} 個，喜歡 {} 個", "{} of my uploads · {} liked", "マイ投稿 {} 件・いいね {} 件", "내 업로드 {}개 · 좋아요 {}개") },
    Msg { key: "status.settings_saved", text: row("设置已保存", "設定已儲存", "Settings saved", "設定を保存しました", "설정 저장됨") },
    Msg { key: "status.remote_updated", text: row("远程输入状态已更新", "遠端輸入狀態已更新", "Remote input updated", "リモート入力を更新しました", "원격 입력 업데이트됨") },
    Msg { key: "status.preloaded", text: row("当前模型已预加载", "目前模型已預載", "Active model preloaded", "現在のモデルをプリロードしました", "현재 모델 미리 로드됨") },
    Msg { key: "status.model_released", text: row("模型已释放", "模型已釋放", "Model released", "モデルを解放しました", "모델 해제됨") },
    Msg { key: "status.cancel_prepare_ok", text: row("已请求取消模型准备", "已請求取消模型準備", "Cancellation requested", "準備のキャンセルを要求しました", "준비 취소 요청됨") },
    Msg { key: "status.activated", text: row("本地模型已激活并预加载", "本機模型已啟用並預載", "Local model activated and preloaded", "ローカルモデルをアクティブ化してプリロードしました", "로컬 모델 활성화 및 미리 로드됨") },
    Msg { key: "status.download_done", text: row("模型下载完成", "模型下載完成", "Model download finished", "モデルのダウンロードが完了", "모델 다운로드 완료") },
    Msg { key: "status.download_cancel_requested", text: row("已请求取消模型下载", "已請求取消模型下載", "Download cancellation requested", "ダウンロードのキャンセルを要求しました", "다운로드 취소 요청됨") },
    Msg { key: "status.download_cancelled", text: row("模型下载已取消", "模型下載已取消", "Model download cancelled", "モデルのダウンロードをキャンセル", "모델 다운로드 취소됨") },
    Msg { key: "status.prepare_done", text: row("模型验证完成：{}", "模型驗證完成：{}", "Model verification done: {}", "モデル検証が完了: {}", "모델 검증 완료: {}") },
    Msg { key: "status.test_done", text: row("模型测试完成：{}（{} ms）", "模型測試完成：{}（{} ms）", "Model test done: {} ({} ms)", "モデルテスト完了: {}（{} ms）", "모델 테스트 완료: {}({}ms)") },
    Msg { key: "status.model_deleted", text: row("模型已删除", "模型已刪除", "Model deleted", "モデルを削除しました", "모델 삭제됨") },
    Msg { key: "status.channel_created", text: row("渠道已创建", "管道已建立", "Channel created", "チャネルを作成しました", "채널 생성됨") },
    Msg { key: "status.channel_active", text: row("active 渠道已更新", "active 管道已更新", "Active channel updated", "アクティブチャネルを更新しました", "활성 채널 업데이트됨") },
    Msg { key: "status.channel_enabled", text: row("渠道启用状态已更新", "管道啟用狀態已更新", "Channel enabled state updated", "チャネルの有効状態を更新しました", "채널 사용 상태 업데이트됨") },
    Msg { key: "status.channel_reordered", text: row("渠道顺序已更新", "管道順序已更新", "Channel order updated", "チャネルの順序を更新しました", "채널 순서 업데이트됨") },
    Msg { key: "status.channel_deleted", text: row("渠道已删除", "管道已刪除", "Channel deleted", "チャネルを削除しました", "채널 삭제됨") },
    Msg { key: "status.provider_type_updated", text: row("Provider 类型已更新", "Provider 類型已更新", "Provider type updated", "Provider 種別を更新しました", "Provider 유형 업데이트됨") },
    Msg { key: "status.channel_saved", text: row("渠道配置已保存", "管道設定已儲存", "Channel configuration saved", "チャネル設定を保存しました", "채널 설정 저장됨") },
    Msg { key: "status.secret_cleared", text: row("渠道 Secret 已清除", "管道 Secret 已清除", "Channel Secret cleared", "チャネルの Secret を消去しました", "채널 Secret 지워짐") },
    Msg { key: "status.provider_validated", text: row("Provider 验证通过（{} ms）", "Provider 驗證通過（{} ms）", "Provider validated ({} ms)", "Provider 検証成功（{} ms）", "Provider 검증 통과({}ms)") },
    Msg { key: "status.export_log_done", text: row("错误日志已导出", "錯誤日誌已匯出", "Error log exported", "エラーログをエクスポートしました", "오류 로그 내보냄") },
    Msg { key: "status.hotkey_handled", text: row("已处理快捷键", "已處理快速鍵", "Hotkey handled", "ホットキーを処理しました", "단축키 처리됨") },
    Msg { key: "status.launch_handled", text: row("已处理启动请求", "已處理啟動請求", "Launch request handled", "起動要求を処理しました", "시작 요청 처리됨") },
    Msg { key: "status.request_restart", text: row("请手动重启 OpenLess", "請手動重新啟動 OpenLess", "Please restart OpenLess manually", "OpenLess を手動で再起動してください", "OpenLess를 수동으로 재시작하세요") },
    Msg { key: "status.tray_stopped", text: row("系统托盘已停止：{}", "系統托盤已停止：{}", "System tray stopped: {}", "システムトレイを停止: {}", "시스템 트레이 중지됨: {}") },
    Msg { key: "status.style_switched", text: row("已切换风格：{}", "已切換風格：{}", "Switched style: {}", "スタイルを切替: {}", "스타일 전환됨: {}") },
    Msg { key: "status.no_previous_style", text: row("没有可切换的上一风格", "沒有可切換的上一風格", "No previous style to switch to", "切替可能な前スタイルがありません", "전환할 이전 스타일이 없습니다") },
    Msg { key: "status.mic_selected", text: row("已选择麦克风：{}", "已選擇麥克風：{}", "Microphone selected: {}", "マイクを選択: {}", "마이크 선택됨: {}") },
    Msg { key: "status.preset_updated", text: row("词汇预设已更新", "詞彙預設已更新", "Vocabulary preset updated", "語彙プリセットを更新しました", "어휘 프리셋 업데이트됨") },
    Msg { key: "status.preset_gone", text: row("词汇预设已不存在", "詞彙預設已不存在", "Vocabulary preset no longer exists", "語彙プリセットはもうありません", "어휘 프리셋이 더 이상 없음") },
    Msg { key: "status.suggestion_handled", text: row("词汇建议已处理", "詞彙建議已處理", "Vocabulary suggestion handled", "語彙提案を処理しました", "어휘 제안 처리됨") },
    Msg { key: "status.vocab_saved", text: row("词汇已保存", "詞彙已儲存", "Vocabulary saved", "語彙を保存しました", "어휘 저장됨") },
    Msg { key: "status.vocab_updated", text: row("词汇已更新", "詞彙已更新", "Vocabulary updated", "語彙を更新しました", "어휘 업데이트됨") },
    Msg { key: "status.correction_saved", text: row("纠错规则已保存", "糾錯規則已儲存", "Correction rule saved", "修正ルールを保存しました", "교정 규칙 저장됨") },
    Msg { key: "status.correction_updated", text: row("纠错规则已更新", "糾錯規則已更新", "Correction rule updated", "修正ルールを更新しました", "교정 규칙 업데이트됨") },
    Msg { key: "status.style_hotkey_saved", text: row("风格包快捷键已更新", "風格包快速鍵已更新", "Style-pack shortcut updated", "スタイルパックのショートカットを更新しました", "스타일 팩 단축키 업데이트됨") },
    Msg { key: "status.style_imported", text: row("已导入风格包：{}", "已匯入風格包：{}", "Imported style pack: {}", "スタイルパックを読込: {}", "스타일 팩 가져옴: {}") },
    Msg { key: "status.style_saved", text: row("风格包已保存：{}", "風格包已儲存：{}", "Style pack saved: {}", "スタイルパックを保存: {}", "스타일 팩 저장됨: {}") },
    Msg { key: "status.style_updated", text: row("风格包已更新", "風格包已更新", "Style pack updated", "スタイルパックを更新しました", "스타일 팩 업데이트됨") },
    Msg { key: "status.style_preview", text: row("{}：单轮 {} 字，多轮 {} 字，热词 {} 个", "{}：單輪 {} 字，多輪 {} 字，熱詞 {} 個", "{}: {} chars/single · {} multi · {} hotwords", "{}: 単発 {} 文字・多発 {} 文字・ホットワード {} 個", "{}: 단일 {}자 · 다중 {}자 · 핫워드 {}개") },
    Msg { key: "status.marketplace_installed", text: row("已安装风格包：{}", "已安裝風格包：{}", "Installed style pack: {}", "スタイルパックをインストール: {}", "스타일 팩 설치됨: {}") },
    Msg { key: "status.marketplace_like", text: row("喜欢数：{}", "喜歡數：{}", "Likes: {}", "いいね数: {}", "좋아요 수: {}") },
    Msg { key: "status.marketplace_zip_saved", text: row("Marketplace ZIP 已保存", "Marketplace ZIP 已儲存", "Marketplace ZIP saved", "Marketplace ZIP を保存しました", "마켓플레이스 ZIP 저장됨") },
    Msg { key: "status.marketplace_published", text: row("发布状态：{} · {}", "發佈狀態：{} · {}", "Publish status: {} · {}", "公開状態: {} · {}", "배포 상태: {} · {}") },
    Msg { key: "status.marketplace_deleted", text: row("Marketplace 发布已删除", "Marketplace 發佈已刪除", "Marketplace release deleted", "Marketplace のリリースを削除しました", "마켓플레이스 배포 삭제됨") },
    Msg { key: "status.history_copied", text: row("历史文本已复制", "歷史文字已複製", "Text copied to clipboard", "クリップボードにコピーしました", "텍스트가 복사됨") },
    Msg { key: "status.copy_failed", text: row("复制失败：{}", "複製失敗：{}", "Copy failed: {}", "コピー失敗: {}", "복사 실패: {}") },
    Msg { key: "status.repolish_done", text: row("重新润色完成：{}", "重新潤飾完成：{}", "Repolish done: {}", "再推敲完了: {}", "다시 다듬기 완료: {}") },
    Msg { key: "status.history_deleted", text: row("历史记录已删除", "歷史紀錄已刪除", "History entry deleted", "履歴を削除しました", "기록 삭제됨") },
    Msg { key: "status.opened_player", text: row("已交给系统播放器", "已交給系統播放器", "Opened in the system player", "システムプレーヤーで開きました", "시스템 플레이어에서 열림") },
    Msg { key: "status.recording_exported", text: row("录音已导出：{}", "錄音已匯出：{}", "Recording exported: {}", "録音をエクスポート: {}", "녹음 내보냄: {}") },
    Msg { key: "status.retranscribed", text: row("重新转写完成：{}", "重新轉寫完成：{}", "Retranscription done: {}", "再文字起こし完了: {}", "다시 받아쓰기 완료: {}") },
    Msg { key: "status.dictation_phase", text: row("听写：{}", "聽寫：{}", "Dictation: {}", "ディクテーション: {}", "받아쓰기: {}") },
    Msg { key: "status.dictation_done", text: row("听写完成：{}", "聽寫完成：{}", "Dictation done: {}", "ディクテーション完了: {}", "받아쓰기 완료: {}") },
    Msg { key: "status.model_progress", text: row("模型 {}：{} {}/{}", "模型 {}：{} {}/{}", "Model {}: {} {}/{}", "モデル {}: {} {}/{}", "모델 {}: {} {}/{}") },
    Msg { key: "status.backlog_reset", text: row("事件积压 {} 条，已重置派生界面并重放可用事件", "事件積壓 {} 條，已重置衍生介面並重放可用事件", "{} events backlogged — reset derived UI and replayed available events", "{} 件のイベントが滞り、派生UIをリセットして再送しました", "이벤트 {}건 밀림 — 파생 UI 재설정 및 재생됨") },
    Msg { key: "status.backlog_replay", text: row("事件积压 {} 条，已从 Core 重放补齐", "事件積壓 {} 條，已從 Core 重放補齊", "{} events backlogged — replayed from Core", "{} 件のイベントが滞り、Core から再送しました", "이벤트 {}건 밀림 — Core에서 재생됨") },
    Msg { key: "status.dialog_cancelled", text: row("操作已取消", "操作已取消", "Operation cancelled", "操作をキャンセルしました", "작업 취소됨") },
    Msg { key: "status.voice_cancelled_ok", text: row("语音会话已取消", "語音工作階段已取消", "Voice session cancelled", "音声セッションをキャンセルしました", "음성 세션 취소됨") },
    Msg { key: "popup.ignore_no_session", text: row("已忽略没有活动会话的弹窗操作", "已忽略沒有活動工作階段的彈窗操作", "Ignored popup action without an active session", "アクティブなセッションのないポップアップ操作を無視しました", "활성 세션이 없는 팝업 동작 무시됨") },
    Msg { key: "popup.ignore_stale", text: row("已忽略迟到、重复或跨类型的弹窗操作", "已忽略遲到、重複或跨類型的彈窗操作", "Ignored late, duplicate or cross-kind popup action", "遅延・重複・異種のポップアップ操作を無視しました", "지연/중복/유형 오류 팝업 동작 무시됨") },
    Msg { key: "popup.ignore_late_qa", text: row("已忽略迟到的问答弹窗操作", "已忽略遲到的問答彈窗操作", "Ignored a late Q&A popup action", "遅れた Q&A ポップアップ操作を無視しました", "지연된 Q&A 팝업 동작 무시됨") },
    Msg { key: "popup.protocol_error", text: row("原生弹窗协议错误：{}", "原生彈窗協定錯誤：{}", "Native popup protocol error: {}", "ネイティブポップアップのプロトコルエラー: {}", "네이티브 팝업 프로토콜 오류: {}") },
    Msg { key: "popup.spawn_failed", text: row("原生弹窗启动失败：{}", "原生彈窗啟動失敗：{}", "Failed to start native popup: {}", "ネイティブポップアップの起動に失敗: {}", "네이티브 팝업 시작 실패: {}") },
    Msg { key: "popup.exited", text: row("原生弹窗异常退出：{}", "原生彈窗異常結束：{}", "Native popup exited unexpectedly: {}", "ネイティブポップアップが異常終了: {}", "네이티브 팝업 비정상 종료: {}") },
    Msg { key: "popup.start_failed", text: row("无法启动原生弹窗：{}", "無法啟動原生彈窗：{}", "Could not start the native popup: {}", "ネイティブポップアップを起動できません: {}", "네이티브 팝업을 시작할 수 없음: {}") },
    Msg { key: "popup.channel_rebuild", text: row("原生弹窗通道重建：{}", "原生彈窗通道重建：{}", "Rebuilt native popup channel: {}", "ネイティブポップアップのチャネルを再構築: {}", "네이티브 팝업 채널 재구축: {}") },
    Msg { key: "popup.recover_failed", text: row("原生弹窗恢复失败：{}", "原生彈窗恢復失敗：{}", "Native popup recovery failed: {}", "ネイティブポップアップの復元に失敗: {}", "네이티브 팝업 복구 실패: {}") },
    Msg { key: "popup.session_invalid", text: row("弹窗 session 无效：{}", "彈窗 session 無效：{}", "Invalid popup session: {}", "無効なポップアップセッション: {}", "잘못된 팝업 세션: {}") },
    Msg { key: "status.from_preset", text: row("预设：{}", "預設：{}", "Preset: {}", "プリセット: {}", "프리셋: {}") },
    Msg { key: "dialog.export_log_cancelled", text: row("日志导出已取消", "日誌匯出已取消", "Log export cancelled", "ログのエクスポートをキャンセル", "로그 내보내기 취소됨") },
    Msg { key: "dialog.style_import_cancelled", text: row("风格包导入已取消", "風格包匯入已取消", "Style-pack import cancelled", "スタイルパックのインポートをキャンセル", "스타일 팩 가져오기 취소됨") },
    Msg { key: "dialog.style_export_cancelled", text: row("风格包导出已取消", "風格包匯出已取消", "Style-pack export cancelled", "スタイルパックのエクスポートをキャンセル", "스타일 팩 내보내기 취소됨") },
    Msg { key: "dialog.recording_export_cancelled", text: row("录音导出已取消", "錄音匯出已取消", "Recording export cancelled", "録音のエクスポートをキャンセル", "녹음 내보내기 취소됨") },
    Msg { key: "dialog.marketplace_zip_cancelled", text: row("Marketplace 下载已取消", "Marketplace 下載已取消", "Marketplace download cancelled", "Marketplace のダウンロードをキャンセル", "마켓플레이스 다운로드 취소됨") },
    Msg { key: "status.history_cleared", text: row("历史已清空", "歷史已清空", "History cleared", "履歴をクリアしました", "기록이 지워졌습니다") },
    Msg { key: "status.remote_pin_reset", text: row("远程输入配对码已重置", "遠端輸入配對碼已重設", "Remote input pairing code reset", "リモート入力のペアリングコードを再発行しました", "원격 입력 페어링 코드 재설정됨") },
    Msg { key: "tray.show", text: row("显示 OpenLess", "顯示 OpenLess", "Show OpenLess", "OpenLess を表示", "OpenLess 표시") },
    Msg { key: "tray.previous_style", text: row("切换到上一风格", "切換到上一風格", "Switch to previous style", "前のスタイルに切り替え", "이전 스타일로 전환") },
    Msg { key: "tray.quit", text: row("退出", "結束", "Quit", "終了", "종료") },
    Msg {
        key: "selection_ask.desc",
        text: row(
            "选中文字后语音提问，支持多轮追问。",
            "選中文字後語音提問，支援多輪追問。",
            "Select text and ask questions by voice, with multi-turn follow-ups.",
            "テキストを選択して音声で質問。複数ターンの追問対応。",
            "텍스트 선택 후 음성으로 질문. 다중 라운드 후속 질문 지원.",
        ),
    },
    Msg {
        key: "selection_ask.guide_ask_desc",
        text: row(
            "按 {} 录音，再按一次提交。",
            "按 {} 錄音，再按一次提交。",
            "Press {} to record, then press again to submit.",
            "{} で録音し、もう一度押して送信します。",
            "{}로 녹음하고, 다시 눌러 전송하세요.",
        ),
    },
    Msg {
        key: "selection_ask.guide_ask_title",
        text: row(
            "开口说出问题",
            "開口說出問題",
            "Say your question",
            "声で質問する",
            "말로 질문하기",
        ),
    },
    Msg {
        key: "selection_ask.guide_dismiss",
        text: row(
            "关闭浮窗，结束本次对话",
            "關閉浮窗，結束本次對話",
            "Close the panel and end this conversation",
            "パネルを閉じて、この会話を終了",
            "패널을 닫고 이번 대화 종료",
        ),
    },
    Msg {
        key: "selection_ask.guide_followup",
        text: row(
            "继续使用录音快捷键，即可多轮追问。",
            "繼續使用錄音快捷鍵，即可多輪追問。",
            "Use the recording shortcut again to ask a follow-up.",
            "録音キーでもう一度、続けて質問できます。",
            "녹음 단축키를 다시 눌러 후속 질문을 할 수 있어요.",
        ),
    },
    Msg {
        key: "selection_ask.guide_open_desc",
        text: row(
            "按 {}，开始一轮对话。",
            "按 {}，開始一輪對話。",
            "Press {} to start a conversation.",
            "{} で会話を始めます。",
            "{}로 대화를 시작하세요.",
        ),
    },
    Msg {
        key: "selection_ask.guide_open_title",
        text: row(
            "打开追问浮窗",
            "開啟追問浮窗",
            "Open the panel",
            "パネルを開く",
            "질문 패널 열기",
        ),
    },
    Msg {
        key: "selection_ask.guide_select_title",
        text: row(
            "选中想了解的内容",
            "選取想了解的內容",
            "Select something to explore",
            "知りたい内容を選択",
            "궁금한 내용 선택",
        ),
    },
    Msg {
        key: "selection_ask.guide_unset_desc",
        text: row(
            "先在快捷键设置中，为划词追问设置一个快捷键。",
            "先到快捷鍵設定中，為劃詞追問設定快捷鍵。",
            "Assign a Selection Ask shortcut in Shortcut settings first.",
            "まずショートカット設定で選択追問のキーを割り当ててください。",
            "먼저 단축키 설정에서 선택 질문 단축키를 지정하세요.",
        ),
    },
    Msg {
        key: "selection_ask.history_desc",
        text: row(
            "开启后在本地保存问答记录，默认关闭。",
            "開啟後在本地保存問答記錄，預設關閉。",
            "Save Q&A records locally when enabled. Off by default.",
            "有効時、Q&A 記録をローカルに保存。デフォルト OFF。",
            "활성화 시 Q&A 기록을 로컬에 저장. 기본 OFF.",
        ),
    },
    Msg {
        key: "selection_ask.history_title",
        text: row("保存历史", "保存歷史", "Save history", "履歴を保存", "기록 저장"),
    },
    Msg {
        key: "selection_ask.howto_step2",
        text: row(
            "在任意 app 选中文字。",
            "在任意 app 選中文字。",
            "Select text in any app.",
            "任意のアプリでテキストを選択。",
            "아무 앱에서 텍스트 선택.",
        ),
    },
    Msg {
        key: "selection_ask.howto_title",
        text: row("使用方法", "使用方法", "How to use", "使い方", "사용 방법"),
    },
    Msg {
        key: "selection_ask.title",
        text: row(
            "划词追问",
            "劃詞追問",
            "Selection Ask",
            "選択追問",
            "선택 질문",
        ),
    },
    Msg {
        key: "translation.desc",
        text: row(
            "录音后自动翻译为目标语言再插入。",
            "錄音後自動翻譯為目標語言再插入。",
            "Auto-translate recordings into a target language before insertion.",
            "録音後に自動翻訳してから入力。",
            "녹음 후 대상 언어로 자동 번역하여 삽입.",
        ),
    },
    Msg {
        key: "translation.howto_step1",
        text: row(
            "在任意输入框聚焦光标。",
            "在任意輸入框聚焦游標。",
            "Place cursor in any text field.",
            "任意の入力欄にカーソルを置く。",
            "아무 입력 필드에 커서를 놓으세요.",
        ),
    },
    Msg {
        key: "translation.howto_step2",
        text: row(
            "按 {} 开始录音。",
            "按 {} 開始錄音。",
            "Press {} to start recording.",
            "{} を押して録音開始。",
            "{} 를 눌러 녹음 시작.",
        ),
    },
    Msg {
        key: "translation.howto_step3",
        text: row(
            "录音中按一下 {} 激活翻译。",
            "錄音中按一下 {} 啟動翻譯。",
            "Press {} once during recording to activate translation.",
            "録音中に {} を一度押して翻訳を起動。",
            "녹음 중 {} 를 한 번 눌러 번역 활성화.",
        ),
    },
    Msg {
        key: "translation.howto_step4",
        text: row(
            "再按 {} 停止录音。",
            "再按 {} 停止錄音。",
            "Press {} again to stop.",
            "再度 {} を押して停止。",
            "다시 {} 를 눌러 정지.",
        ),
    },
    Msg {
        key: "translation.howto_step5",
        text: row(
            "翻译结果自动插入到光标位置。",
            "翻譯結果自動插入到游標位置。",
            "Translated text is inserted at the cursor.",
            "翻訳結果がカーソル位置に挿入されます。",
            "번역 결과가 커서 위치에 삽입됩니다.",
        ),
    },
    Msg {
        key: "translation.howto_title",
        text: row("使用方法", "使用方法", "How to use", "使い方", "사용 방법"),
    },
    Msg {
        key: "translation.kicker",
        text: row("翻译", "翻譯", "TRANSLATION", "翻訳", "번역"),
    },
    Msg {
        key: "translation.status_disabled",
        text: row("未启用", "未啓用", "Disabled", "無効", "비활성화됨"),
    },
    Msg {
        key: "translation.status_enabled",
        text: row("已启用", "已啓用", "Enabled", "有効", "활성화됨"),
    },
    Msg {
        key: "translation.style_desc",
        text: row(
            "自动继承「风格」页当前激活的风格包。",
            "自動沿用「風格」頁目前啓用的風格包。",
            "Automatically inherits the active style pack from the Style page.",
            "「スタイル」ページで現在有効なスタイルパックを自動的に引き継ぎます。",
            "「스타일」 페이지에서 현재 활성화된 스타일 팩을 자동으로 사용합니다.",
        ),
    },
    Msg {
        key: "translation.style_title",
        text: row(
            "翻译风格",
            "翻譯風格",
            "Translation style",
            "翻訳スタイル",
            "번역 스타일",
        ),
    },
    Msg {
        key: "translation.target_desc",
        text: row(
            "录音时按 Shift 触发翻译。选「不启用」则 Shift 无效。",
            "錄音時按 Shift 觸發翻譯。選「不啟用」則 Shift 無效。",
            "Press Shift during recording to trigger translation. \"Disabled\" makes Shift a no-op.",
            "録音中に Shift で翻訳を起動。「無効」で Shift 無効化。",
            "녹음 중 Shift 로 번역 실행. \"비활성화\" 시 Shift 무효.",
        ),
    },
    Msg {
        key: "translation.target_disabled",
        text: row(
            "不启用（Shift 按下不触发翻译）",
            "不啓用（Shift 按下不觸發翻譯）",
            "Disabled (Shift does nothing)",
            "無効（Shift で翻訳を発動しない）",
            "비활성화 (Shift 로 번역 발동 안 함)",
        ),
    },
    Msg {
        key: "translation.target_same_as_working",
        text: row(
            "目标语言与你唯一的工作语言相同，翻译不会生效：按 Shift 仍按普通润色处理。换一个目标语言，或在上方多勾选一个工作语言。",
            "目標語言與你唯一的工作語言相同，翻譯不會生效：按 Shift 仍按普通潤色處理。換一個目標語言，或在上方多勾選一個工作語言。",
            "The target matches your only working language, so translation cannot take effect — Shift will just run a normal polish. Pick a different target, or add another working language above.",
            "ターゲット言語が唯一の作業言語と同じため、翻訳は発動しません（Shift を押しても通常の整文になります）。別のターゲットを選ぶか、上で作業言語を追加してください。",
            "대상 언어가 유일한 작업 언어와 같아 번역이 실행되지 않습니다. Shift 를 눌러도 일반 정리로 처리됩니다. 다른 대상 언어를 고르거나 위에서 작업 언어를 추가하세요.",
        ),
    },
    Msg {
        key: "translation.target_title",
        text: row(
            "翻译目标语言",
            "翻譯目標語言",
            "Translation target language",
            "翻訳ターゲット言語",
            "번역 대상 언어",
        ),
    },
    Msg {
        key: "translation.title",
        text: row("翻译", "翻譯", "Translation", "翻訳", "번역"),
    },
    Msg {
        key: "translation.working_desc",
        text: row(
            "勾选日常使用的语言，影响润色与翻译效果。",
            "勾選日常使用的語言，影響潤色與翻譯效果。",
            "Select languages you use regularly to improve polish and translation.",
            "日常使用する言語を選択し、整文と翻訳に反映。",
            "일상적으로 사용하는 언어를 선택하여 정리와 번역에 반영.",
        ),
    },
    Msg {
        key: "translation.working_title",
        text: row(
            "工作语言",
            "工作語言",
            "Working languages",
            "作業言語",
            "작업 언어",
        ),
    },
    Msg {
        key: "vocab.corrections_empty",
        text: row(
            "还没有纠正规则。",
            "還沒有糾正規則。",
            "No correction rules yet.",
            "補正ルールはまだありません。",
            "아직 교정 규칙이 없습니다.",
        ),
    },
    Msg {
        key: "vocab.corrections_learned_badge",
        text: row("自动", "自動", "auto", "自動", "자동"),
    },
    Msg {
        key: "vocab.corrections_pattern_placeholder",
        text: row(
            "误识别写法，如 {num}粒",
            "誤識別寫法，如 {num}粒",
            "Mistaken text, e.g. {num}粒",
            "誤認識された表記（例：{num}粒）",
            "오인식 표현, 예: {num}粒",
        ),
    },
    Msg {
        key: "vocab.corrections_replacement_placeholder",
        text: row(
            "目标写法，如 {num}例",
            "目標寫法，如 {num}例",
            "Target text, e.g. {num}例",
            "修正後の表記（例：{num}例）",
            "대상 표현, 예: {num}例",
        ),
    },
    Msg {
        key: "vocab.corrections_tip",
        text: row(
            "修正常见 ASR 误识别，支持 {num} 数字通配。",
            "修正常見 ASR 誤識別，支援 {num} 數字通配。",
            "Fix common ASR mistakes. Supports {num} number wildcard.",
            "ASR の誤認識を修正。{num} 数字ワイルドカード対応。",
            "ASR 오인식 수정. {num} 숫자 와일드카드 지원.",
        ),
    },
    Msg {
        key: "vocab.corrections_title",
        text: row(
            "纠正规则",
            "糾正規則",
            "Correction rules",
            "補正ルール",
            "교정 규칙",
        ),
    },
    Msg {
        key: "vocab.desc",
        text: row(
            "添加生词或专业术语，提高识别准确率。",
            "添加生詞或專業術語，提高識別準確率。",
            "Add terms or jargon to improve recognition accuracy.",
            "新語や専門用語を追加して認識精度を向上。",
            "새 단어나 전문 용어를 추가하여 인식 정확도 향상.",
        ),
    },
    Msg {
        key: "vocab.kicker",
        text: row("词典", "詞典", "DICTIONARY", "辞書", "사전"),
    },
    Msg {
        key: "vocab.learned_section",
        text: row(
            "自动收集（{}）",
            "自動收集（{}）",
            "Auto-collected ({})",
            "自動収集（{}）",
            "자동 수집 ({})",
        ),
    },
    Msg {
        key: "vocab.placeholder",
        text: row(
            "输入词语，按 Enter 或点添加…",
            "輸入詞語，按 Enter 或點添加…",
            "Type a word, press Enter or click Add…",
            "単語を入力し、Enter または追加をクリック…",
            "단어를 입력하고 Enter 또는 추가 클릭…",
        ),
    },
    Msg {
        key: "vocab.presets_apply",
        text: row(
            "启用所选",
            "啓用所選",
            "Apply selected",
            "選択中を有効化",
            "선택 활성화",
        ),
    },
    Msg {
        key: "vocab.presets_create",
        text: row("新建预设", "新建預設", "New preset", "プリセット新規作成", "프리셋 새로 만들기"),
    },
    Msg {
        key: "vocab.presets_edit",
        text: row("编辑 {}", "編輯 {}", "Edit {}", "{} を編集", "{} 편집"),
    },
    Msg {
        key: "vocab.presets_name_placeholder",
        text: row("预设名称", "預設名稱", "Preset name", "プリセット名", "프리셋 이름"),
    },
    Msg {
        key: "vocab.presets_new_preset",
        text: row("新预设", "新預設", "New preset", "新しいプリセット", "새 프리셋"),
    },
    Msg {
        key: "vocab.presets_save",
        text: row("保存预设", "保存預設", "Save preset", "プリセットを保存", "프리셋 저장"),
    },
    Msg {
        key: "vocab.presets_tip",
        text: row(
            "可多选批量启用，支持编辑和新建。",
            "可多選批量啟用，支援編輯和新建。",
            "Multi-select to apply in batch. Supports edit and create.",
            "複数選択で一括適用。編集・新規作成対応。",
            "다중 선택 일괄 적용 가능. 편집 및 생성 지원.",
        ),
    },
    Msg {
        key: "vocab.presets_title",
        text: row(
            "场景预设",
            "場景預設",
            "Scenario presets",
            "シーンプリセット",
            "시나리오 프리셋",
        ),
    },
    Msg {
        key: "vocab.presets_words_placeholder",
        text: row(
            "词条（用逗号或换行分隔）",
            "詞條（用逗號或換行分隔）",
            "Terms (comma or newline separated)",
            "語彙（カンマまたは改行区切り）",
            "어휘(쉼표 또는 줄바꿈으로 구분)",
        ),
    },
    Msg {
        key: "vocab.remove_all_learned",
        text: row("全部删除", "全部刪除", "Remove all", "すべて削除", "모두 삭제"),
    },
    Msg {
        key: "vocab.section_title",
        text: row("词条", "詞條", "Entries", "項目", "항목"),
    },
    Msg {
        key: "vocab.tip",
        text: row(
            "支持中英混合 · 数字开头按字面识别 · 命中次数自动计数",
            "支持中英混合 · 數字開頭按字面識別 · 命中次數自動計數",
            "Mixed Chinese/English supported · numeric prefixes are matched literally · hits counted automatically",
            "日本語と英数の混在対応 · 数字始まりは字面通り認識 · ヒット回数を自動カウント",
            "한영 혼용 지원 · 숫자로 시작하면 그대로 인식 · 적중 횟수 자동 카운트",
        ),
    },
    Msg {
        key: "vocab.title",
        text: row("词典", "詞典", "Dictionary", "辞書", "사전"),
    },
    Msg {
        key: "style.custom_prompt_save",
        text: row("保存提示词", "保存提示詞", "Save prompt", "プロンプトを保存", "프롬프트 저장"),
    },
    Msg {
        key: "style.desc",
        text: row(
            "选择录音的默认输出风格。",
            "選擇錄音的預設輸出風格。",
            "Choose the default output style for recording.",
            "録音のデフォルト出力スタイルを選択。",
            "녹음의 기본 출력 스타일 선택.",
        ),
    },
    Msg {
        key: "style.kicker",
        text: row("风格", "風格", "STYLE", "スタイル", "스타일"),
    },
    Msg {
        key: "style.pack.builtin",
        text: row("内置", "內建", "Built-in", "ビルトイン", "기본"),
    },
    Msg {
        key: "style.pack.current",
        text: row("当前", "目前", "Current", "現在", "현재"),
    },
    Msg {
        key: "style.pack.dictation_prompt_title",
        text: row(
            "录音 / ASR Prompt",
            "錄音 / ASR Prompt",
            "Recording / ASR prompt",
            "録音 / ASRプロンプト",
            "녹음 / ASR 프롬프트",
        ),
    },
    Msg {
        key: "style.pack.dictation_tab",
        text: row(
            "录音 / ASR 风格",
            "錄音 / ASR 風格",
            "Recording / ASR styles",
            "録音 / ASRスタイル",
            "녹음 / ASR 스타일",
        ),
    },
    Msg {
        key: "style.pack.new_description",
        text: row(
            "简短描述这个风格的使用场景。",
            "簡短描述這個風格的使用情境。",
            "Briefly describe when to use this style.",
            "このスタイルを使う場面を簡潔に説明してください。",
            "이 스타일을 언제 사용하는지 간단히 설명하세요.",
        ),
    },
    Msg {
        key: "style.pack.selection_tab",
        text: row(
            "选区润色",
            "選區潤色",
            "Selection polish",
            "選択範囲の推敲",
            "선택 영역 다듬기",
        ),
    },
    Msg {
        key: "style.title",
        text: row("输出风格", "輸出風格", "Output style", "出力スタイル", "출력 스타일"),
    },
    Msg {
        key: "marketplace.desc",
        text: row(
            "浏览、安装和分享社区风格包。",
            "瀏覽、安裝和分享社區風格包。",
            "Browse, install, and share community style packs.",
            "コミュニティのスタイルパックを閲覧・インストール・共有。",
            "커뮤니티 스타일 팩 둘러보기, 설치, 공유.",
        ),
    },
    Msg {
        key: "marketplace.download_zip_btn",
        text: row("下载 ZIP", "下載 ZIP", "Download ZIP", "ZIP をダウンロード", "ZIP 다운로드"),
    },
    Msg {
        key: "marketplace.empty",
        text: row(
            "还没有风格包",
            "還沒有風格包",
            "No style packs yet",
            "まだスタイルパックがありません",
            "아직 스타일 팩이 없습니다",
        ),
    },
    Msg {
        key: "marketplace.empty_hint",
        text: row(
            "换个搜索词，或自己上传一个分享给社区",
            "換個搜尋詞，或自己上傳一個分享給社群",
            "Try a different keyword, or upload your own",
            "別のキーワードを試すか、自分のパックを共有してみましょう",
            "다른 키워드로 검색하거나 직접 업로드해 보세요",
        ),
    },
    Msg {
        key: "marketplace.install_btn",
        text: row("安装到本地", "安裝到本機", "Install", "インストール", "설치"),
    },
    Msg {
        key: "marketplace.kicker",
        text: row("风格市场", "風格市場", "MARKETPLACE", "マーケット", "마켓"),
    },
    Msg {
        key: "marketplace.my_packs_button_label",
        text: row("我的发布", "我的發布", "My Packs", "自分の公開", "내 게시물"),
    },
    Msg {
        key: "marketplace.refresh_btn",
        text: row("刷新", "重新整理", "Refresh", "更新", "새로고침"),
    },
    Msg {
        key: "marketplace.search_placeholder",
        text: row(
            "搜索名称 / 描述 / 标签…",
            "搜尋名稱 / 描述 / 標籤…",
            "Search name / description / tags…",
            "名前 / 説明 / タグを検索…",
            "이름 / 설명 / 태그 검색…",
        ),
    },
    Msg {
        key: "marketplace.sort_liked",
        text: row("我赞过的", "我讚過的", "Liked", "いいね済み", "좋아요한 팩"),
    },
    Msg {
        key: "marketplace.sort_new",
        text: row("最新", "最新", "Newest", "新着", "최신"),
    },
    Msg {
        key: "marketplace.sort_popular",
        text: row("按热度", "按熱度", "Popular", "人気順", "인기순"),
    },
    Msg {
        key: "hotkey.triggers.right_option",
        text: row("右 Option", "右 Option", "Right Option", "右 Option", "오른쪽 Option"),
    },
    Msg {
        key: "modal.about.export_error_log",
        text: row(
            "导出错误日志",
            "匯出錯誤日誌",
            "Export error log",
            "エラーログをエクスポート",
            "오류 로그 내보내기",
        ),
    },
    Msg {
        key: "modal.sections.help_center",
        text: row("帮助中心", "幫助中心", "Help center", "ヘルプセンター", "도움말 센터"),
    },
    Msg {
        key: "modal.sections.release_notes",
        text: row(
            "发布日志",
            "發佈日誌",
            "Release notes",
            "リリースノート",
            "릴리스 노트",
        ),
    },
    Msg {
        key: "overview.actions.shortcuts",
        text: row("快捷键", "快捷鍵", "Shortcuts", "ショートカット", "단축키"),
    },
    Msg {
        key: "overview.llm_name",
        text: row(
            "OpenAI 兼容",
            "OpenAI 兼容",
            "OpenAI-compatible",
            "OpenAI 互換",
            "OpenAI 호환",
        ),
    },
    Msg {
        key: "settings.about.beta_channel_label",
        text: row(
            "加入 Beta 渠道",
            "加入 Beta 渠道",
            "Join Beta channel",
            "Beta チャンネルに参加",
            "Beta 채널 참여",
        ),
    },
    Msg {
        key: "settings.coding_agent.enable",
        text: row(
            "启用 Less Computer",
            "啟用 Less Computer",
            "Enable Less Computer",
            "Less Computer を有効化",
            "Less Computer 켜기",
        ),
    },
    Msg {
        key: "settings.coding_console.permission_mode",
        text: row(
            "权限模式",
            "權限模式",
            "Permission mode",
            "権限モード",
            "권한 모드",
        ),
    },
    Msg {
        key: "settings.coding_console.title",
        text: row(
            "Claude 控制台",
            "Claude 主控台",
            "Claude Console",
            "Claude コンソール",
            "Claude 콘솔",
        ),
    },
    Msg {
        key: "settings.coding_console.workdir",
        text: row(
            "工作目录",
            "工作目錄",
            "Working directory",
            "作業ディレクトリ",
            "작업 디렉터리",
        ),
    },
    Msg {
        key: "settings.data_storage.title",
        text: row("数据存储", "資料儲存", "Data storage", "データ保存", "데이터 저장"),
    },
    Msg {
        key: "settings.debug.title",
        text: row("调试工具", "除錯工具", "Debug tools", "デバッグツール", "디버그 도구"),
    },
    Msg {
        key: "settings.language.title",
        text: row(
            "界面语言",
            "界面語言",
            "Interface language",
            "表示言語",
            "인터페이스 언어",
        ),
    },
    Msg {
        key: "settings.language.zh",
        text: row("简体中文", "簡體中文", "简体中文", "简体中文", "简体中文"),
    },
    Msg {
        key: "settings.layout.title",
        text: row("布局", "布局", "Layout", "レイアウト", "레이아웃"),
    },
    Msg {
        key: "settings.marketplace.github.open_github",
        text: row("打开 GitHub", "開啟 GitHub", "Open GitHub", "GitHub を開く", "GitHub 열기"),
    },
    Msg {
        key: "settings.marketplace.title",
        text: row("扩展市场", "擴充市集", "Marketplace", "拡張マーケット", "확장 마켓"),
    },
    Msg {
        key: "settings.network.use_system_proxy_label",
        text: row(
            "使用系统代理",
            "使用系統代理",
            "Use system proxy",
            "システムプロキシを使用",
            "시스템 프록시 사용",
        ),
    },
    Msg {
        key: "settings.permissions.title",
        text: row("权限", "權限", "Permissions", "権限", "권한"),
    },
    Msg {
        key: "settings.recording.auto_update_check_label",
        text: row(
            "自动检查更新",
            "自動檢查更新",
            "Auto-check for updates",
            "アップデートを自動チェック",
            "자동 업데이트 확인",
        ),
    },
    Msg {
        key: "settings.recording.desc",
        text: row(
            "全局录音的快捷键与触发方式。",
            "定義全局錄音的快捷鍵與觸發方式。",
            "Global recording hotkey and trigger mode.",
            "グローバル録音のショートカットとトリガー方式を定義します。",
            "전역 녹음의 단축키와 트리거 방식을 정의합니다.",
        ),
    },
    Msg {
        key: "settings.recording.insert_group_title",
        text: row(
            "插入与剪贴板",
            "插入與剪貼板",
            "Insertion & clipboard",
            "挿入とクリップボード",
            "삽입 및 클립보드",
        ),
    },
    Msg {
        key: "settings.recording.microphone_system_default",
        text: row(
            "系统默认",
            "系統默認",
            "system default",
            "システムデフォルト",
            "시스템 기본값",
        ),
    },
    Msg {
        key: "settings.recording.mode_label",
        text: row("录音方式", "錄音方式", "Trigger mode", "録音方式", "녹음 방식"),
    },
    Msg {
        key: "settings.recording.mode_toggle",
        text: row("切换式", "切換式", "Toggle", "トグル式", "토글 방식"),
    },
    Msg {
        key: "settings.recording.mute_during_recording_label",
        text: row(
            "录音时静音",
            "錄音時靜音",
            "Mute while recording",
            "録音中はミュート",
            "녹음 중 음소거",
        ),
    },
    Msg {
        key: "settings.recording.paste_shortcut_label",
        text: row(
            "模拟粘贴快捷键",
            "模擬粘貼快捷鍵",
            "Simulated paste shortcut",
            "貼り付けショートカット",
            "붙여넣기 단축키",
        ),
    },
    Msg {
        key: "settings.recording.startup_group_title",
        text: row("启动", "啟動", "Startup", "起動", "시작"),
    },
    Msg {
        key: "settings.remote_input.enable_label",
        text: row(
            "启用远程输入",
            "啟用遠端輸入",
            "Enable remote input",
            "リモート入力を有効化",
            "원격 입력 활성화",
        ),
    },
    Msg {
        key: "settings.remote_input.port_label",
        text: row("监听端口", "監聽連接埠", "Port", "待ち受けポート", "수신 포트"),
    },
    Msg {
        key: "settings.remote_input.title",
        text: row("远程输入", "遠端輸入", "Remote Input", "リモート入力", "원격 입력"),
    },
    Msg {
        key: "settings.theme.dark",
        text: row("深色", "深色", "Dark", "ダーク", "다크"),
    },
    Msg {
        key: "settings.theme.label",
        text: row("主题", "主題", "Theme", "テーマ", "테마"),
    },
    Msg {
        key: "settings.theme.light",
        text: row("浅色", "淺色", "Light", "ライト", "라이트"),
    },
    Msg {
        key: "marketplace.title",
        text: row(
            "风格包市场",
            "風格包市場",
            "Style Pack Marketplace",
            "スタイルパック マーケット",
            "스타일 팩 마켓",
        ),
    },
    Msg {
        key: "settings.selection_workspace.title",
        text: row(
            "选区助手",
            "選區助手",
            "Selection Assistant",
            "選択範囲アシスタント",
            "선택 영역 도우미",
        ),
    },
    Msg {
        key: "modal.sections.about",
        text: row(
            "关于与更新",
            "關於與更新",
            "About & updates",
            "バージョンと更新",
            "정보 및 업데이트",
        ),
    },
    Msg {
        key: "modal.sections.advanced",
        text: row(
            "实验与扩展",
            "實驗與擴充",
            "Experiments & extensions",
            "実験機能と拡張",
            "실험 기능 및 확장",
        ),
    },
    Msg {
        key: "modal.sections.appearance",
        text: row(
            "外观与语言",
            "外觀與語言",
            "Appearance & language",
            "外観と言語",
            "모양 및 언어",
        ),
    },
    Msg {
        key: "modal.sections.general",
        text: row(
            "录音与输入",
            "錄音與輸入",
            "Recording & input",
            "録音と入力",
            "녹음 및 입력",
        ),
    },
    Msg {
        key: "modal.sections.privacy",
        text: row(
            "权限与数据",
            "權限與資料",
            "Permissions & data",
            "権限とデータ",
            "권한 및 데이터",
        ),
    },
    Msg {
        key: "modal.sections.services",
        text: row(
            "AI 服务与模型",
            "AI 服務與模型",
            "AI services & models",
            "AI サービスとモデル",
            "AI 서비스 및 모델",
        ),
    },
    Msg {
        key: "modal.sections.shortcuts",
        text: row(
            "快捷键与选区",
            "快捷鍵與選取文字",
            "Shortcuts & selection",
            "ショートカットと選択",
            "단축키 및 선택",
        ),
    },
    Msg {
        key: "settings.selection_workspace.hint",
        text: row(
            "选中文字后按同一快捷键：关闭语音编辑时直接润色；开启后口述指令，说完再选择「提问」或「编辑选区」。",
            "選中文字後按同一快捷鍵：關閉語音編輯時直接潤色；開啟後口述指令，說完再選擇「提問」或「編輯選區」。",
            "Select text, then use one shortcut: polish when voice edit is off; hold and speak when voice edit is on, then choose Ask or Edit.",
            "テキスト選択後、同じショートカットで：音声編集オフ時は推敲、オン時は押しながら話してから「質問」か「編集」を選択。",
            "텍스트 선택 후 같은 단축키: 음성 편집 끄면 바로 다듬기, 켜면 누른 채 말한 뒤 「질문」 또는 「편집」 선택.",
        ),
    },
    Msg {
        key: "settings.selection_workspace.voice_enable",
        text: row("语音编辑", "語音編輯", "Voice edit", "音声編集", "음성 편집"),
    },
    Msg {
        key: "settings.advanced.multimodal_pipeline_label",
        text: row(
            "启用多模态识别管线",
            "啟用多模態辨識管線",
            "Enable multimodal pipeline",
            "マルチモーダルパイプラインを有効化",
            "멀티모달 파이프라인 활성화",
        ),
    },
    Msg {
        key: "settings.advanced.multimodal_pipeline_title",
        text: row(
            "多模态识别管线",
            "多模態辨識管線",
            "Multimodal recognition pipeline",
            "マルチモーダル認識パイプライン",
            "멀티모달 인식 파이프라인 ",
        ),
    },
    Msg {
        key: "settings.language.label",
        text: row("语言", "語言", "Language", "言語", "언어"),
    },
    Msg {
        key: "settings.network.title",
        text: row("网络", "網路", "Network", "ネットワーク", "네트워크"),
    },
    Msg {
        key: "settings.recording.title",
        text: row(
            "录音与输入",
            "錄音與輸入",
            "Recording & input",
            "録音と入力",
            "녹음 및 입력",
        ),
    },
    Msg {
        key: "selection_ask.shortcut_settings",
        text: row(
            "快捷键设置",
            "快捷鍵設定",
            "Shortcut settings",
            "ショートカット設定",
            "단축키 설정",
        ),
    },
    Msg {
        key: "vocab.corrections_only_learned",
        text: row(
            "只看自动收集的（{}）",
            "只看自動收集的（{}）",
            "Only auto-collected ({})",
            "自動収集のみ表示（{}）",
            "자동 수집만 보기 ({})",
        ),
    },
    Msg {
        key: "vocab.corrections_remove_all_learned",
        text: row(
            "删除全部自动收集的",
            "刪除全部自動收集的",
            "Delete all auto-collected",
            "自動収集をすべて削除",
            "자동 수집 전체 삭제",
        ),
    },
    Msg {
        key: "vocab.empty",
        text: row(
            "还没有词条。在上面输入一个生词或专业术语，让模型在听写时优先匹配。",
            "還沒有詞條。在上面輸入一個生詞或專業術語，讓模型在聽寫時優先匹配。",
            "No entries yet. Add a new term or piece of jargon above so the model can prioritize it.",
            "語彙がありません。新語や専門用語を上に入力すると、ディクテーション時に優先的にマッチします。",
            "어휘가 없습니다. 위에 새 단어나 전문 용어를 입력하면 받아쓰기 시 우선 매칭됩니다.",
        ),
    },
    Msg {
        key: "vocab.filter_all",
        text: row("所有", "所有", "All", "すべて", "전체"),
    },
    Msg {
        key: "vocab.filter_auto",
        text: row("自动添加", "自動新增", "Auto-Added", "自動追加", "자동 추가"),
    },
    Msg {
        key: "vocab.filter_manual",
        text: row(
            "手动添加",
            "手動新增",
            "Manually Added",
            "手動追加",
            "수동 추가",
        ),
    },
    Msg {
        key: "vocab.new_word",
        text: row("新词", "新詞", "New Word", "新語", "새 단어"),
    },
    Msg {
        key: "vocab.search_empty",
        text: row(
            "没有匹配的词条。",
            "沒有符合的詞條。",
            "No matching words.",
            "一致する単語がありません。",
            "일치하는 단어가 없습니다.",
        ),
    },
    Msg {
        key: "vocab.search_placeholder",
        text: row("搜索", "搜尋", "Search", "検索", "검색"),
    },
    Msg {
        key: "translation.howto_fallback_desc",
        text: row(
            "翻译失败时回退为插入原始转写，不会丢字。",
            "翻譯失敗時回退為插入原始轉寫，不會丟字。",
            "If translation fails, the raw transcript is inserted instead.",
            "翻訳失敗時は原文がそのまま挿入されます。",
            "번역 실패 시 원본 전사가 삽입됩니다.",
        ),
    },
    Msg {
        key: "translation.howto_fallback_title",
        text: row(
            "安全兜底",
            "安全兜底",
            "Safety fallbacks",
            "セーフティフォールバック",
            "안전 폴백",
        ),
    },
    Msg {
        key: "translation.howto_indicator_desc",
        text: row(
            "按 Shift 后屏幕底部会显示蓝色「正在翻译」标识。",
            "按 Shift 後螢幕底部會顯示藍色「正在翻譯」標識。",
            "A blue \"Translating\" indicator appears at the bottom of the screen after pressing Shift.",
            "Shift を押すと画面下部に青い「翻訳中」表示が出ます。",
            "Shift 를 누르면 화면 하단에 파란색 \"번역 중\" 표시가 나타납니다.",
        ),
    },
    Msg {
        key: "translation.howto_indicator_title",
        text: row(
            "翻译模式指示",
            "怎麼知道翻譯模式生效了",
            "How to confirm translation mode is on",
            "翻訳モードの確認方法",
            "번역 모드 활성화 확인 방법",
        ),
    },
    Msg {
        key: "translation.language_support_hint",
        text: row(
            "语音服务支持的语种可能不同；翻译目标不受界面语言限制。",
            "語音服務支援的語種可能不同；翻譯目標不受介面語言限制。",
            "Available speech languages depend on your provider. Translation targets are independent of the app language.",
            "音声認識で使える言語はサービスによって異なります。翻訳先はアプリの表示言語とは独立しています。",
            "음성 서비스에 따라 지원 언어가 다릅니다. 번역 언어는 앱 표시 언어와 별개입니다.",
        ),
    },
    Msg {
        key: "translation.no_matching_languages",
        text: row(
            "没有匹配的语言",
            "沒有符合的語言",
            "No matching languages",
            "一致する言語がありません",
            "일치하는 언어가 없습니다",
        ),
    },
    Msg {
        key: "translation.search_languages",
        text: row(
            "搜索语言…",
            "搜尋語言…",
            "Search languages…",
            "言語を検索…",
            "언어 검색…",
        ),
    },
    Msg {
        key: "translation.selected_languages",
        text: row(
            "已选择 {} 种语言",
            "已選擇 {} 種語言",
            "{} languages selected",
            "{} 言語を選択中",
            "언어 {}개 선택됨",
        ),
    },
    Msg {
        key: "modal.auto_save_hint",
        text: row(
            "修改后自动保存",
            "修改後自動儲存",
            "Changes save automatically",
            "変更は自動保存されます",
            "변경 사항이 자동 저장됩니다",
        ),
    },
    Msg {
        key: "modal.search_placeholder",
        text: row(
            "查找设置分类…",
            "尋找設定分類…",
            "Find a settings category…",
            "設定カテゴリを検索…",
            "설정 카테고리 찾기…",
        ),
    },
    Msg {
        key: "modal.service_views.asr",
        text: row(
            "语音识别",
            "語音辨識",
            "Speech recognition",
            "音声認識",
            "음성 인식",
        ),
    },
    Msg {
        key: "modal.service_views.connections",
        text: row("连接与扩展", "連線與擴充", "Connections", "接続と拡張", "연결 및 확장"),
    },
    Msg {
        key: "modal.service_views.llm",
        text: row(
            "语言模型",
            "語言模型",
            "Language models",
            "言語モデル",
            "언어 모델",
        ),
    },
    Msg {
        key: "modal.service_views.models",
        text: row("本地模型", "本機模型", "Local models", "ローカルモデル", "로컬 모델"),
    },
    Msg {
        key: "selection_ask.hotkey_title",
        text: row(
            "弹出浮窗的快捷键",
            "彈出浮窗的快捷鍵",
            "Hotkey to open the panel",
            "フロートウィンドウのショートカット",
            "플로팅 창 단축키",
        ),
    },
    Msg {
        key: "settings.about.beta_channel_toggle_label",
        text: row(
            "启用 Beta 渠道",
            "啟用 Beta 渠道",
            "Enable Beta channel",
            "Beta チャンネルを有効化",
            "Beta 채널 사용",
        ),
    },
    Msg {
        key: "settings.about.check_stable_update_btn",
        text: row(
            "检查正式版更新",
            "檢查正式版更新",
            "Check stable update",
            "正式版を確認",
            "정식판 확인",
        ),
    },
    Msg {
        key: "settings.about.docs",
        text: row("文档", "文檔", "Docs", "ドキュメント", "문서"),
    },
    Msg {
        key: "settings.about.feedback",
        text: row("反馈", "反饋", "Feedback", "フィードバック", "피드백"),
    },
    Msg {
        key: "settings.about.links_title",
        text: row(
            "文档链接",
            "文件連結",
            "Documentation",
            "ドキュメント",
            "문서 링크",
        ),
    },
    Msg {
        key: "settings.about.local_first",
        text: row("本地优先", "本地優先", "Local-first", "ローカル優先", "로컬 우선"),
    },
    Msg {
        key: "settings.about.privacy_desc",
        text: row(
            "录音可能会发送到你配置的云端服务商进行转写。",
            "錄音可能會傳送至你設定的雲端服務商進行轉寫。",
            "Recordings may be sent to the cloud provider you configure for transcription.",
            "録音は、設定したクラウドプロバイダーへ文字起こしのため送信される場合があります。",
            "녹음은 전사를 위해 설정한 클라우드 공급자에게 전송될 수 있습니다.",
        ),
    },
    Msg {
        key: "settings.about.qq",
        text: row(
            "社区 QQ 群",
            "社區 QQ 羣",
            "QQ community group",
            "コミュニティ QQ グループ",
            "커뮤니티 QQ 그룹",
        ),
    },
    Msg {
        key: "settings.about.source",
        text: row("源码", "源碼", "Source", "ソース", "소스"),
    },
    Msg {
        key: "settings.about.tagline",
        text: row(
            "自然说话，完美书写",
            "自然說話，完美書寫",
            "Speak naturally, write perfectly",
            "自然に話し、きれいに書く",
            "자연스럽게 말하고, 정확하게 작성하세요",
        ),
    },
    Msg {
        key: "settings.advanced.local_asr_desc",
        text: row(
            "把转写从云端切到本机推理。仅推荐离线 / 隐私敏感场景。",
            "把轉寫從雲端切到本機推理。僅推薦離線 / 隱私敏感場景。",
            "Move transcription from cloud ASR to on-device inference. Offline / privacy-sensitive use only.",
            "転写をクラウドから本機推論に切り替えます。オフライン／プライバシー重視向け。",
            "전사를 클라우드에서 로컬 추론으로 전환합니다. 오프라인 / 프라이버시용에만 권장됩니다.",
        ),
    },
    Msg {
        key: "settings.advanced.multimodal_pipeline_title_hint",
        text: row(
            "用单个多模态模型一步完成语音识别；与传统 ASR + LLM 配置完全隔离。",
            "用單一多模態模型一步完成語音辨識；與傳統 ASR + LLM 設定完全隔離。",
            "One-pass audio recognition with a single multimodal model; traditional ASR + LLM configuration is fully isolated from it.",
            "1つのマルチモーダルモデルで音声認識を一括実行。従来の ASR + LLM 設定から完全に分離されます。",
            "단일 멀티모달 모델로 음성 인식을 한 번에 처리합니다. 기존 ASR + LLM 설정과 완전히 분리됩니다.",
        ),
    },
    Msg {
        key: "settings.advanced.platform_not_supported",
        text: row(
            "该平台暂未支持本地 ASR 模型集成。",
            "該平臺暫未支持本地 ASR 模型集成。",
            "Local ASR model integration is not supported on this platform.",
            "このプラットフォームではローカル ASR モデル統合に対応していません。",
            "이 플랫폼에서는 로컬 ASR 모델 통합이 아직 지원되지 않습니다.",
        ),
    },
    Msg {
        key: "settings.advanced.streaming_insert_desc",
        text: row(
            "逐字实时插入，降低感知延迟。不满足条件时回落到一次性粘贴。",
            "逐字即時插入，降低感知延遲。不滿足條件時回落到一次性貼上。",
            "Streams text to cursor character by character, reducing perceived latency. Falls back to one-shot paste when conditions are not met.",
            "逐字リアルタイム挿入で体感遅延を低減。条件不一致時はワンショット貼り付けにフォールバック。",
            "실시간 글자별 삽입으로 체감 지연 감소. 조건 불충족 시 일괄 붙여넣기로 전환.",
        ),
    },
    Msg {
        key: "settings.advanced.streaming_insert_label",
        text: row(
            "流式输入",
            "流式輸入",
            "Streaming insertion",
            "ストリーミング入力",
            "스트리밍 입력",
        ),
    },
    Msg {
        key: "settings.advanced.streaming_insert_save_clipboard_label",
        text: row(
            "同步到剪贴板",
            "同步到剪貼簿",
            "Copy to clipboard",
            "クリップボードに保存",
            "클립보드에 저장",
        ),
    },
    Msg {
        key: "settings.advanced.streaming_insert_title_linux",
        text: row(
            "流式输入（实验性）",
            "流式輸入（實驗性）",
            "Streaming insertion (Experimental)",
            "ストリーミング入力（実験的）",
            "스트리밍 입력 (실험적)",
        ),
    },
    Msg {
        key: "settings.channels.add",
        text: row("添加渠道", "新增渠道", "Add channel", "チャネルを追加", "채널 추가"),
    },
    Msg {
        key: "settings.channels.asr_title",
        text: row(
            "语音识别渠道",
            "語音辨識渠道",
            "Speech recognition channels",
            "音声認識チャンネル",
            "음성 인식 채널",
        ),
    },
    Msg {
        key: "settings.channels.create",
        text: row("创建", "建立", "Create", "作成", "만들기"),
    },
    Msg {
        key: "settings.channels.current",
        text: row(
            "当前使用",
            "目前使用",
            "Currently used",
            "使用中",
            "현재 사용 중",
        ),
    },
    Msg {
        key: "settings.channels.delete",
        text: row(
            "删除渠道",
            "刪除渠道",
            "Delete channel",
            "チャネルを削除",
            "채널 삭제",
        ),
    },
    Msg {
        key: "settings.channels.disabled",
        text: row("已停用", "已停用", "Disabled", "無効", "사용 안 함"),
    },
    Msg {
        key: "settings.channels.elapsed",
        text: row("耗时 {} ms", "耗時 {} ms", "Took {} ms", "所要時間 {} ms", "소요 시간 {} ms"),
    },
    Msg {
        key: "settings.channels.empty",
        text: row(
            "还没有渠道。点击「添加渠道」，连接你的第一个服务。",
            "還沒有渠道。點選「新增渠道」，連接你的第一個服務。",
            "No channels yet. Choose \"Add channel\" to connect your first service.",
            "チャネルがまだありません。「チャネルを追加」で最初のサービスを接続しましょう。",
            "아직 채널이 없습니다. \"채널 추가\"로 첫 서비스를 연결하세요.",
        ),
    },
    Msg {
        key: "settings.channels.enabled",
        text: row("启用", "啟用", "Enabled", "有効", "사용"),
    },
    Msg {
        key: "settings.channels.failed",
        text: row(
            "验证失败 · {}",
            "驗證失敗 · {}",
            "Check failed · {}",
            "確認に失敗 · {}",
            "확인 실패 · {}",
        ),
    },
    Msg {
        key: "settings.channels.llm_title",
        text: row(
            "文字处理渠道",
            "文字處理渠道",
            "Text processing channels",
            "テキスト処理チャンネル",
            "텍스트 처리 채널",
        ),
    },
    Msg {
        key: "settings.channels.name_hint",
        text: row(
            "名称仅用于区分同一供应商的多个渠道，不影响模型或连接。",
            "名稱僅用於區分同一供應商的多個渠道，不影響模型或連線。",
            "This name distinguishes channels from the same provider. It does not affect the model or connection.",
            "同じプロバイダーのチャンネルを区別するための名前です。モデルや接続には影響しません。",
            "같은 제공업체의 여러 채널을 구분하는 이름입니다. 모델이나 연결에는 영향을 주지 않습니다.",
        ),
    },
    Msg {
        key: "settings.channels.name_placeholder",
        text: row(
            "例如：硅基流动-主号",
            "例如：矽基流動-主帳號",
            "e.g. SiliconFlow — main key",
            "例：SiliconFlow — メインキー",
            "예: SiliconFlow — 메인 키",
        ),
    },
    Msg {
        key: "settings.channels.not_verified",
        text: row(
            "尚未验证",
            "尚未驗證",
            "Not checked yet",
            "未確認",
            "아직 확인하지 않음",
        ),
    },
    Msg {
        key: "settings.channels.order_hint",
        text: row(
            "列表中第一个启用的渠道用于请求。拖动调整顺序；停用的渠道移到末尾。",
            "請求會使用列表中第一個啟用的渠道。拖曳可調整順序；停用的渠道會移到末尾。",
            "Requests use the first enabled channel. Drag to reorder; disabled channels move to the bottom.",
            "有効なチャネルのうち、先頭のものを使用します。ドラッグで順序を変更できます。無効なチャネルは末尾に移動します。",
            "사용 중인 채널 중 맨 위의 채널로 요청합니다. 드래그로 순서를 바꾸면 사용하지 않는 채널은 맨 아래로 이동합니다.",
        ),
    },
    Msg {
        key: "settings.channels.passed",
        text: row("验证通过", "驗證通過", "Check passed", "確認に成功", "확인 성공"),
    },
    Msg {
        key: "settings.channels.verify",
        text: row("验证", "驗證", "Verify", "検証", "검증"),
    },
    Msg {
        key: "settings.coding_agent.desc",
        text: row(
            "按住一个键说话，由所选 Agent 帮你操作电脑。仅 macOS。",
            "按住一個鍵說話，由所選 Agent 幫你操作電腦。僅 macOS。",
            "Hold a key, speak, and your selected agent operates your computer. macOS only.",
            "キーを押して話すと、選択した Agent が PC を操作します。macOS のみ。",
            "키를 누르고 말하면 선택한 Agent가 PC를 조작합니다. macOS 전용.",
        ),
    },
    Msg {
        key: "settings.coding_agent.title",
        text: row(
            "Less Computer",
            "Less Computer",
            "Less Computer",
            "Less Computer",
            "Less Computer",
        ),
    },
    Msg {
        key: "settings.coding_console.desc",
        text: row(
            "检测本机 Claude Code 与 MCP（computer use）状态，并护栏化地无头跑一次 Claude、流式查看输出与用量。",
            "偵測本機 Claude Code 與 MCP（computer use）狀態，並以護欄方式無頭執行一次 Claude、串流檢視輸出與用量。",
            "Detect your local Claude Code and MCP (computer use) status, then run Claude headlessly behind guardrails and watch the streamed output and cost.",
            "ローカルの Claude Code と MCP（computer use）の状態を検出し、ガードレール付きで Claude をヘッドレス実行して、出力とコストをストリーミング表示します。",
            "로컬 Claude Code 와 MCP(computer use) 상태를 감지하고, 가드레일 아래에서 Claude 를 헤드리스로 실행하여 출력과 비용을 스트리밍으로 확인합니다.",
        ),
    },
    Msg {
        key: "settings.coding_console.detect",
        text: row("检测", "偵測", "Detect", "検出", "감지"),
    },
    Msg {
        key: "settings.coding_console.status",
        text: row("状态", "狀態", "Status", "状態", "상태"),
    },
    Msg {
        key: "settings.data_storage.desc",
        text: row(
            "本机保留的历史会话与对话上下文。",
            "本機保留的歷史會話與對話上下文。",
            "Conversation history and context kept on this device.",
            "この端末に保存される会話履歴とコンテキスト。",
            "이 기기에 보관되는 대화 기록과 컨텍스트.",
        ),
    },
    Msg {
        key: "settings.debug.desc",
        text: row(
            "排查识别问题时使用，平时无需开启。",
            "排查辨識問題時使用，平時無需開啟。",
            "For troubleshooting recognition issues; off by default.",
            "認識の問題を調査するときに使用。通常はオフのままで構いません。",
            "인식 문제를 진단할 때 사용합니다. 평소에는 꺼두어도 됩니다.",
        ),
    },
    Msg {
        key: "settings.language.desc",
        text: row(
            "切换 UI 显示语言。当前会话即时生效，下次启动自动沿用。",
            "切換 UI 顯示語言。當前會話即時生效，下次啓動自動沿用。",
            "Switch the UI language. Applies to the current session immediately and persists across launches.",
            "UI の表示言語を切り替えます。現在のセッションに即時反映され、次回起動時も維持されます。",
            "UI 표시 언어를 전환합니다. 현재 세션에 즉시 반영되며 다음 실행에도 유지됩니다.",
        ),
    },
    Msg {
        key: "settings.language.en",
        text: row("English", "English", "English", "English", "English"),
    },
    Msg {
        key: "settings.language.follow_system",
        text: row(
            "跟随系统",
            "跟隨系統",
            "Follow system",
            "システムに従う",
            "시스템 따라가기",
        ),
    },
    Msg {
        key: "settings.language.ja",
        text: row("日本語 (Beta)", "日本語 (Beta)", "日本語 (Beta)", "日本語 (Beta)", "日本語 (Beta)"),
    },
    Msg {
        key: "settings.language.ko",
        text: row("한국어 (Beta)", "한국어 (Beta)", "한국어 (Beta)", "한국어 (Beta)", "한국어 (Beta)"),
    },
    Msg {
        key: "settings.language.restart_hint",
        text: row(
            "部分原生菜单（系统托盘等）可能需要重启 App 才会切换。",
            "部分原生菜單（系統托盤等）可能需要重啓 App 纔會切換。",
            "Some native menus (system tray, etc.) may require an app restart to fully switch.",
            "一部のネイティブメニュー（トレイ等）は再起動後に反映されます。",
            "일부 네이티브 메뉴(트레이 등)는 앱 재시작 후 반영될 수 있습니다.",
        ),
    },
    Msg {
        key: "settings.language.zh_tw",
        text: row("繁體中文", "繁體中文", "繁體中文", "繁體中文", "繁體中文"),
    },
    Msg {
        key: "settings.marketplace.desc",
        text: row(
            "风格市场的上传身份。浏览与安装风格在「风格」页内完成。",
            "風格市集的上傳身份。瀏覽與安裝風格在「風格」頁內完成。",
            "Upload identity for the style marketplace. Browse and install styles on the Styles page.",
            "スタイルマーケットの投稿者 ID。スタイルの閲覧とインストールは「スタイル」ページで行います。",
            "스타일 마켓 업로드 신원. 스타일 둘러보기와 설치는 「스타일」 페이지에서 합니다.",
        ),
    },
    Msg {
        key: "settings.marketplace.github.sign_in",
        text: row(
            "用 GitHub 账号登录",
            "用 GitHub 帳號登入",
            "Sign in with GitHub",
            "GitHub でログイン",
            "GitHub로 로그인",
        ),
    },
    Msg {
        key: "settings.permissions.acc_label",
        text: row(
            "辅助功能",
            "輔助功能",
            "Accessibility",
            "アクセシビリティ",
            "접근성",
        ),
    },
    Msg {
        key: "settings.permissions.desc_no_acc",
        text: row(
            "麦克风必需；全局快捷键状态用来检测 native hook 是否运行。",
            "OpenLess 需要麥克風可用，並依賴全局快捷鍵監聽狀態判斷 native hook 是否正常工作。",
            "OpenLess needs microphone access and uses the global hotkey listener state to verify the native hook is running.",
            "OpenLess はマイクへのアクセスと、グローバルショートカット監視状態を通じてネイティブフックの正常動作を判定する必要があります。",
            "OpenLess 는 마이크 사용과 전역 단축키 감지 상태를 통해 네이티브 후크의 정상 동작을 판정해야 합니다.",
        ),
    },
    Msg {
        key: "settings.permissions.granted",
        text: row("已授权", "已授權", "Granted", "許可済み", "허용됨"),
    },
    Msg {
        key: "settings.permissions.hotkey_label",
        text: row(
            "全局快捷键",
            "全局快捷鍵",
            "Global hotkey",
            "グローバルショートカット",
            "전역 단축키",
        ),
    },
    Msg {
        key: "settings.permissions.indeterminate",
        text: row("未确定", "未確定", "Undetermined", "未確定", "미결정"),
    },
    Msg {
        key: "settings.permissions.mic_label",
        text: row("麦克风", "麥克風", "Microphone", "マイク", "마이크"),
    },
    Msg {
        key: "settings.permissions.network_label",
        text: row("网络", "網絡", "Network", "ネットワーク", "네트워크"),
    },
    Msg {
        key: "settings.permissions.network_ok",
        text: row("可用", "可用", "Available", "利用可能", "사용 가능"),
    },
    Msg {
        key: "settings.permissions.open_system",
        text: row(
            "打开系统设置",
            "打開系統設置",
            "Open System Settings",
            "システム設定を開く",
            "시스템 설정 열기",
        ),
    },
    Msg {
        key: "settings.providers.credential_storage_notice",
        text: row(
            "凭据保存在系统凭据库中。",
            "憑據保存在系統憑據庫中。",
            "Credentials are stored in the OS credential vault.",
            "資格情報は OS の資格情報ストアに保存されます。",
            "자격 증명은 OS 자격 증명 저장소에 보관됩니다.",
        ),
    },
    Msg {
        key: "settings.recording.audio_cue_label",
        text: row(
            "录音提示音",
            "錄音提示音",
            "Recording start sound",
            "録音開始音",
            "녹음 시작음",
        ),
    },
    Msg {
        key: "settings.recording.audio_recording_max_entries_label",
        text: row(
            "原始录音保留条数",
            "原始錄音保留條數",
            "Max raw recordings",
            "元音声の保持件数",
            "원본 녹음 보관 개수",
        ),
    },
    Msg {
        key: "settings.recording.history_group_title",
        text: row(
            "历史与上下文",
            "歷史與上下文",
            "History & context",
            "履歴とコンテキスト",
            "기록 및 컨텍스트",
        ),
    },
    Msg {
        key: "settings.recording.history_max_entries_label",
        text: row(
            "历史条数上限",
            "歷史條數上限",
            "Max history entries",
            "履歴件数の上限",
            "기록 개수 상한",
        ),
    },
    Msg {
        key: "settings.recording.history_retention_label",
        text: row(
            "历史保留天数",
            "歷史保留天數",
            "History retention (days)",
            "履歴保持期間（日）",
            "기록 보관 기간(일)",
        ),
    },
    Msg {
        key: "settings.recording.hotkey_label",
        text: row(
            "录音快捷键",
            "錄音快捷鍵",
            "Recording hotkey",
            "録音ショートカット",
            "녹음 단축키",
        ),
    },
    Msg {
        key: "settings.recording.microphone_label",
        text: row(
            "首选麦克风",
            "首選麥克風",
            "Preferred microphone",
            "優先マイク",
            "기본 선택 마이크",
        ),
    },
    Msg {
        key: "settings.recording.paste_shortcut_ctrl_shift_v",
        text: row(
            "Ctrl+Shift+V（kitty / alacritty / wezterm / 多数终端）",
            "Ctrl+Shift+V（kitty / alacritty / wezterm / 多數終端）",
            "Ctrl+Shift+V (kitty / alacritty / wezterm / most terminals)",
            "Ctrl+Shift+V（kitty / alacritty / wezterm / ほとんどのターミナル）",
            "Ctrl+Shift+V (kitty / alacritty / wezterm / 대부분 터미널)",
        ),
    },
    Msg {
        key: "settings.recording.paste_shortcut_ctrl_v",
        text: row(
            "Ctrl+V（默认 / 多数应用）",
            "Ctrl+V（默認 / 多數應用）",
            "Ctrl+V (default / most apps)",
            "Ctrl+V（既定 / ほとんどのアプリ）",
            "Ctrl+V (기본 / 대부분 앱)",
        ),
    },
    Msg {
        key: "settings.recording.paste_shortcut_shift_insert",
        text: row(
            "Shift+Insert（xterm / urxvt）",
            "Shift+Insert（xterm / urxvt）",
            "Shift+Insert (xterm / urxvt)",
            "Shift+Insert（xterm / urxvt）",
            "Shift+Insert (xterm / urxvt)",
        ),
    },
    Msg {
        key: "settings.recording.record_audio_for_debug_label",
        text: row(
            "保留原始录音（调试）",
            "保留原始錄音（除錯）",
            "Keep raw recording (debug)",
            "元の録音を保持（デバッグ）",
            "원본 녹음 보관(디버그)",
        ),
    },
    Msg {
        key: "settings.recording.restore_clipboard_label",
        text: row(
            "插入后恢复剪贴板",
            "插入後恢復剪貼板",
            "Restore clipboard after insert",
            "入力後にクリップボードを復元",
            "입력 후 클립보드 복원",
        ),
    },
    Msg {
        key: "settings.recording.silence_auto_stop_label",
        text: row(
            "静音后自动停止",
            "靜音後自動停止",
            "Auto-stop after silence",
            "無音で自動停止",
            "침묵 시 자동 중지",
        ),
    },
    Msg {
        key: "settings.recording.silence_auto_stop_seconds_label",
        text: row(
            "静音时长",
            "靜音時長",
            "Silence duration",
            "無音の長さ",
            "침묵 시간",
        ),
    },
    Msg {
        key: "settings.recording.silence_auto_stop_seconds_value",
        text: row("{} 秒", "{} 秒", "{}s", "{} 秒", "{}초"),
    },
    Msg {
        key: "settings.recording.start_minimized_label",
        text: row(
            "启动时静默运行",
            "啓動時靜默運行",
            "Start minimized (no main window)",
            "起動時にメインウィンドウを表示しない",
            "시작 시 메인 창 숨기기",
        ),
    },
    Msg {
        key: "settings.recording.startup_at_boot",
        text: row(
            "开机自启",
            "開機自啓",
            "Launch at login",
            "起動時に自動起動",
            "부팅 시 자동 시작",
        ),
    },
    Msg {
        key: "settings.remote_input.default_mode_label",
        text: row(
            "默认录音方式",
            "預設錄音方式",
            "Default recording mode",
            "既定の録音方式",
            "기본 녹음 방식",
        ),
    },
    Msg {
        key: "settings.remote_input.enable_desc",
        text: row(
            "手机/平板浏览器连到电脑录音，语音实时落到电脑光标处（需 HTTPS，首次访问要信任证书）",
            "手機/平板瀏覽器連到電腦錄音，語音即時落到電腦游標處（需 HTTPS，首次存取要信任憑證）",
            "Record from a phone/tablet browser on your LAN; speech is typed at your computer's cursor (HTTPS required; trust the certificate on first visit)",
            "スマホ/タブレットのブラウザから PC に接続して録音し、音声を PC のカーソル位置にリアルタイムで入力します（HTTPS が必要。初回アクセス時は証明書を信頼してください）",
            "휴대폰/태블릿 브라우저를 PC에 연결해 녹음하고, 음성을 PC 커서 위치에 실시간으로 입력합니다(HTTPS 필요, 첫 접속 시 인증서를 신뢰해야 함)",
        ),
    },
    Msg {
        key: "settings.remote_input.mode_hold",
        text: row("按住说话", "按住說話", "Hold to talk", "押し続けて話す", "눌러서 말하기"),
    },
    Msg {
        key: "settings.remote_input.mode_toggle",
        text: row(
            "点击切换",
            "點擊切換",
            "Tap to toggle",
            "タップで切替",
            "탭하여 전환",
        ),
    },
    Msg {
        key: "settings.shortcuts.agent_voice",
        text: row(
            "Less Computer",
            "Less Computer",
            "Less Computer",
            "Less Computer",
            "Less Computer",
        ),
    },
    Msg {
        key: "settings.shortcuts.cancel",
        text: row(
            "取消本次录音",
            "取消本次錄音",
            "Cancel current recording",
            "本回の録音をキャンセル",
            "이번 녹음 취소",
        ),
    },
    Msg {
        key: "settings.shortcuts.desc_no_acc",
        text: row(
            "所有快捷键全局生效。若无响应，请在权限页查看全局快捷键监听状态。",
            "所有快捷鍵全局生效。若無響應，請在權限頁查看全局快捷鍵監聽狀態。",
            "All shortcuts apply globally. If unresponsive, check the global hotkey status in Permissions.",
            "すべてのショートカットはグローバルで有効。応答がない場合は権限ページでグローバルショートカット監視の状態を確認してください。",
            "모든 단축키는 전역에서 작동. 응답이 없으면 권한 페이지에서 전역 단축키 감지 상태를 확인해 주세요.",
        ),
    },
    Msg {
        key: "settings.shortcuts.open_app",
        text: row(
            "打开 OpenLess",
            "打開 OpenLess",
            "Open OpenLess",
            "OpenLess を開く",
            "OpenLess 열기",
        ),
    },
    Msg {
        key: "settings.shortcuts.start_stop",
        text: row(
            "开始 / 停止录音",
            "開始 / 停止錄音",
            "Start / Stop recording",
            "録音開始 / 停止",
            "녹음 시작 / 정지",
        ),
    },
    Msg {
        key: "settings.shortcuts.style_pack_add",
        text: row(
            "添加风格快捷键",
            "新增風格快捷鍵",
            "Add style shortcut",
            "スタイルショートカットを追加",
            "스타일 단축키 추가",
        ),
    },
    Msg {
        key: "settings.shortcuts.style_pack_title",
        text: row(
            "风格直达快捷键",
            "風格直達快捷鍵",
            "Style shortcuts",
            "スタイル直行ショートカット",
            "스타일 바로가기 단축키",
        ),
    },
    Msg {
        key: "settings.shortcuts.switch_style",
        text: row(
            "切换到上一个风格",
            "切換到上一個風格",
            "Switch to previous style",
            "前のスタイルに切り替え",
            "이전 스타일로 전환",
        ),
    },
    Msg {
        key: "settings.shortcuts.title",
        text: row(
            "快捷键设置",
            "快捷鍵設定",
            "Shortcut settings",
            "ショートカット設定",
            "단축키 설정",
        ),
    },
    Msg {
        key: "settings.theme.activity_heatmap_label",
        text: row(
            "概览页显示年度活动热力图",
            "概覽頁顯示年度活動熱力圖",
            "Show annual activity heatmap on Overview",
            "概要ページに年間アクティビティを表示",
            "개요 페이지에 연간 활동 표시",
        ),
    },
    Msg {
        key: "settings.theme.conservative_layout_label",
        text: row(
            "保守排版",
            "保守排版",
            "Conservative layout",
            "保守レイアウト",
            "보수적 레이아웃",
        ),
    },
    Msg {
        key: "settings.theme.stacked_row_layout_label",
        text: row(
            "易读布局（防溢出换行）",
            "易讀布局（防溢出換行）",
            "Readable layout (wrap rows)",
            "読みやすいレイアウト（はみ出し防止）",
            "읽기 쉬운 레이아웃(넘침 방지 줄바꿈)",
        ),
    },
    Msg {
        key: "settings.theme.system",
        text: row(
            "跟随系统",
            "跟隨系統",
            "Follow system",
            "システムに従う",
            "시스템 따르기",
        ),
    },
    Msg {
        key: "settings.theme.title",
        text: row("外观", "外觀", "Appearance", "外観", "모양"),
    },
    Msg {
        key: "vocab.delete_selected",
        text: row(
            "删除已选（{}）",
            "刪除已選（{}）",
            "Delete selected ({})",
            "選択項目を削除（{}）",
            "선택 항목 삭제({})",
        ),
    },
    Msg {
        key: "vocab.select_all_visible",
        text: row(
            "选择当前结果",
            "選取目前結果",
            "Select current results",
            "現在の結果を選択",
            "현재 결과 선택",
        ),
    },
    Msg {
        key: "vocab.selected_count",
        text: row(
            "已选择 {} 个词",
            "已選取 {} 個詞",
            "{} words selected",
            "{} 語を選択中",
            "단어 {}개 선택됨",
        ),
    },
    Msg {
        key: "settings.providers.presets.ark",
        text: row(
            "ARK（火山方舟）",
            "ARK（火山方舟）",
            "ARK (Volcengine Ark)",
            "ARK（Volcengine Ark）",
            "ARK (Volcengine Ark)",
        ),
    },
    Msg { key: "settings.providers.presets.deepseek", text: row("DeepSeek", "DeepSeek", "DeepSeek", "DeepSeek", "DeepSeek") },
    Msg { key: "settings.providers.presets.siliconflow", text: row("硅基流动", "硅基流動", "SiliconFlow", "SiliconFlow", "SiliconFlow") },
    Msg { key: "settings.providers.presets.atlascloud", text: row("Atlas Cloud", "Atlas Cloud", "Atlas Cloud", "Atlas Cloud", "Atlas Cloud") },
    Msg { key: "settings.providers.presets.openai", text: row("OpenAI", "OpenAI", "OpenAI", "OpenAI", "OpenAI") },
    Msg {
        key: "settings.providers.presets.gemini",
        text: row(
            "Google Gemini",
            "Google Gemini",
            "Google Gemini",
            "Google Gemini",
            "Google Gemini",
        ),
    },
    Msg { key: "settings.providers.presets.codexOAuth", text: row("Codex OAuth", "Codex OAuth", "Codex OAuth", "Codex OAuth", "Codex OAuth") },
    Msg { key: "settings.providers.presets.mimo", text: row("小米 MiMo", "小米 MiMo", "Xiaomi MiMo", "Xiaomi MiMo", "Xiaomi MiMo") },
    Msg { key: "settings.providers.presets.cometapi", text: row("CometAPI", "CometAPI", "CometAPI", "CometAPI", "CometAPI") },
    Msg {
        key: "settings.providers.presets.openrouterFree",
        text: row(
            "OpenRouter（免费模型）",
            "OpenRouter（免費模型）",
            "OpenRouter (free models)",
            "OpenRouter（無料モデル）",
            "OpenRouter(무료 모델)",
        ),
    },
    Msg { key: "settings.providers.presets.orcarouter", text: row("OrcaRouter", "OrcaRouter", "OrcaRouter", "OrcaRouter", "OrcaRouter") },
    Msg {
        key: "settings.providers.presets.alibabaCoding",
        text: row(
            "阿里云 Coding Plan",
            "阿里雲 Coding Plan",
            "Alibaba Cloud Coding Plan",
            "Alibaba Cloud Coding Plan",
            "Alibaba Cloud Coding Plan",
        ),
    },
    Msg { key: "settings.providers.presets.codingPlanX", text: row("CodingPlanX", "CodingPlanX", "CodingPlanX", "CodingPlanX", "CodingPlanX") },
    Msg { key: "settings.providers.presets.minimax", text: row("MiniMax（M3）", "MiniMax（M3）", "MiniMax (M3)", "MiniMax（M3）", "MiniMax (M3)") },
    Msg {
        key: "settings.providers.presets.stepfun",
        text: row(
            "StepFun（阶跃星辰）",
            "StepFun（階躍星辰）",
            "StepFun",
            "StepFun（階躍星辰）",
            "StepFun",
        ),
    },
    Msg { key: "settings.providers.presets.opencode", text: row("OpenCode Zen", "OpenCode Zen", "OpenCode Zen", "OpenCode Zen", "OpenCode Zen") },
    Msg {
        key: "settings.providers.presets.tencentTokenHub",
        text: row(
            "腾讯云 TokenHub",
            "騰訊雲 TokenHub",
            "Tencent Cloud TokenHub",
            "Tencent Cloud TokenHub",
            "Tencent Cloud TokenHub",
        ),
    },
    Msg {
        key: "settings.providers.presets.customChatCompletions",
        text: row(
            "自定义 · Chat Completions",
            "自訂 · Chat Completions",
            "Custom · Chat Completions",
            "カスタム · Chat Completions",
            "사용자 지정 · Chat Completions",
        ),
    },
    Msg {
        key: "settings.providers.presets.customResponses",
        text: row(
            "自定义 · Responses",
            "自訂 · Responses",
            "Custom · Responses",
            "カスタム · Responses",
            "사용자 지정 · Responses",
        ),
    },
    Msg {
        key: "settings.providers.presets.customMessages",
        text: row(
            "自定义 · Messages",
            "自訂 · Messages",
            "Custom · Messages",
            "カスタム · Messages",
            "사용자 지정 · Messages",
        ),
    },
    Msg { key: "settings.providers.presets.custom", text: row("自定义", "自定義", "Custom", "カスタム", "사용자 정의") },
    Msg {
        key: "settings.providers.presets.asrVolcengine",
        text: row(
            "火山引擎 bigasr",
            "火山引擎 bigasr",
            "Volcengine bigasr",
            "Volcengine bigasr",
            "Volcengine bigasr",
        ),
    },
    Msg {
        key: "settings.providers.presets.asrBailian",
        text: row(
            "阿里云百炼实时 ASR",
            "阿里雲百煉即時 ASR",
            "Alibaba Bailian realtime ASR",
            "Alibaba Bailian リアルタイム ASR",
            "Alibaba Bailian 실시간 ASR",
        ),
    },
    Msg {
        key: "settings.providers.presets.asrBailianQwen3",
        text: row(
            "阿里云百炼 Qwen3 实时 ASR",
            "阿里雲百煉 Qwen3 即時 ASR",
            "Bailian Qwen3 Realtime ASR",
            "Bailian Qwen3 リアルタイム ASR",
            "Bailian Qwen3 실시간 ASR",
        ),
    },
    Msg {
        key: "settings.providers.presets.asrBailianFunAsrFlash",
        text: row(
            "阿里云百炼 Fun-ASR-Flash（录音文件）",
            "阿里雲百煉 Fun-ASR-Flash（錄音檔）",
            "Bailian Fun-ASR-Flash (recorded file)",
            "Bailian Fun-ASR-Flash（録音ファイル）",
            "Bailian Fun-ASR-Flash (녹음 파일)",
        ),
    },
    Msg {
        key: "settings.providers.presets.asrSiliconflow",
        text: row(
            "硅基流动 SenseVoice",
            "硅基流動 SenseVoice",
            "SiliconFlow SenseVoice",
            "SiliconFlow SenseVoice",
            "SiliconFlow SenseVoice",
        ),
    },
    Msg {
        key: "settings.providers.presets.asrStepfun",
        text: row(
            "阶跃星辰 StepAudio",
            "階躍星辰 StepAudio",
            "StepFun StepAudio ASR",
            "StepFun StepAudio ASR",
            "StepFun StepAudio ASR",
        ),
    },
    Msg {
        key: "settings.providers.presets.asrZhipu",
        text: row(
            "智谱 GLM-ASR",
            "智譜 GLM-ASR",
            "Zhipu GLM-ASR",
            "Zhipu GLM-ASR",
            "Zhipu GLM-ASR",
        ),
    },
    Msg {
        key: "settings.providers.presets.asrGroq",
        text: row(
            "Groq Whisper-large-v3",
            "Groq Whisper-large-v3",
            "Groq Whisper-large-v3",
            "Groq Whisper-large-v3",
            "Groq Whisper-large-v3",
        ),
    },
    Msg {
        key: "settings.providers.presets.asrWhisper",
        text: row(
            "OpenAI Whisper（兼容）",
            "OpenAI Whisper（兼容）",
            "OpenAI Whisper (compatible)",
            "OpenAI Whisper（互換）",
            "OpenAI Whisper(호환)",
        ),
    },
    Msg {
        key: "settings.providers.presets.asrOpenrouter",
        text: row(
            "OpenRouter Whisper",
            "OpenRouter Whisper",
            "OpenRouter Whisper",
            "OpenRouter Whisper",
            "OpenRouter Whisper",
        ),
    },
    Msg { key: "settings.providers.presets.asrZenmux", text: row("ZenMux", "ZenMux", "ZenMux", "ZenMux", "ZenMux") },
    Msg {
        key: "settings.providers.presets.asrOpenAiCompatible",
        text: row(
            "自定义 OpenAI 兼容",
            "自訂 OpenAI 相容",
            "Custom OpenAI-compatible",
            "カスタム OpenAI 互換",
            "커스텀 OpenAI 호환",
        ),
    },
    Msg {
        key: "settings.providers.presets.asrXiaomiMimo",
        text: row(
            "小米 MiMo ASR",
            "小米 MiMo ASR",
            "Xiaomi MiMo ASR",
            "Xiaomi MiMo ASR",
            "Xiaomi MiMo ASR",
        ),
    },
    Msg {
        key: "settings.providers.presets.asrIflytek",
        text: row(
            "讯飞实时语音转写",
            "訊飛即時語音轉寫",
            "iFlytek Realtime ASR",
            "iFlytek リアルタイム音声認識",
            "iFlytek 실시간 음성 인식",
        ),
    },
    Msg {
        key: "settings.providers.presets.asrTencentCloud",
        text: row(
            "腾讯云混元实时 ASR",
            "騰訊雲混元即時 ASR",
            "Tencent Cloud Hunyuan Realtime ASR",
            "Tencent Cloud Hunyuan リアルタイム ASR",
            "Tencent Cloud Hunyuan 실시간 ASR",
        ),
    },
    Msg {
        key: "settings.providers.presets.asrElevenLabs",
        text: row(
            "ElevenLabs Scribe",
            "ElevenLabs Scribe",
            "ElevenLabs Scribe",
            "ElevenLabs Scribe",
            "ElevenLabs Scribe",
        ),
    },
    Msg {
        key: "settings.providers.presets.asrSherpaOnnxLocal",
        text: row(
            "本地 sherpa-onnx（实验性）",
            "本地 sherpa-onnx（實驗性）",
            "Local sherpa-onnx (Experimental)",
            "ローカル sherpa-onnx（実験的）",
            "로컬 sherpa-onnx(실험적)",
        ),
    },
    Msg {
        key: "settings.providers.presets.asrFoundryLocalWhisper",
        text: row(
            "本地 Whisper（Foundry Local）",
            "本地 Whisper（Foundry Local）",
            "Local Whisper (Foundry Local)",
            "ローカル Whisper（Foundry Local）",
            "로컬 Whisper(Foundry Local)",
        ),
    },
    Msg {
        key: "settings.providers.presets.asrLocalWhisper",
        text: row(
            "本地 Whisper（批量解码）",
            "本地 Whisper（批次解碼）",
            "Local Whisper (batch)",
            "ローカル Whisper（バッチ）",
            "로컬 Whisper(배치)",
        ),
    },
    Msg {
        key: "settings.providers.presets.asrLocalQwen3",
        text: row(
            "本地 Qwen3-ASR",
            "本地 Qwen3-ASR",
            "Local Qwen3-ASR",
            "ローカル Qwen3-ASR",
            "로컬 Qwen3-ASR",
        ),
    },
    Msg {
        key: "settings.providers.presets.asrLocalQwen3Mlx",
        text: row(
            "本地 Qwen3-ASR（MLX / Metal）",
            "本地 Qwen3-ASR（MLX / Metal）",
            "Local Qwen3-ASR (MLX / Metal)",
            "ローカル Qwen3-ASR（MLX / Metal）",
            "로컬 Qwen3-ASR(MLX / Metal)",
        ),
    },
    Msg {
        key: "settings.providers.presets.asrLocalQwen3C",
        text: row(
            "本地 Qwen3-ASR（C / CPU）",
            "本地 Qwen3-ASR（C / CPU）",
            "Local Qwen3-ASR (C / CPU)",
            "ローカル Qwen3-ASR（C / CPU）",
            "로컬 Qwen3-ASR(C / CPU)",
        ),
    },
    Msg {
        key: "settings.providers.presets.asrAppleSpeech",
        text: row(
            "Apple 语音（macOS）",
            "Apple 語音（macOS）",
            "Apple Speech (macOS)",
            "Apple 音声認識 (macOS)",
            "Apple 음성 (macOS)",
        ),
    },
    Msg {
        key: "settings.providers.presets.omniOpenai",
        text: row(
            "OpenAI（支持音频）",
            "OpenAI（支援音訊）",
            "OpenAI (audio-capable)",
            "OpenAI（音声対応）",
            "OpenAI (오디오 지원)",
        ),
    },
    Msg {
        key: "settings.providers.presets.omniGemini",
        text: row(
            "Google Gemini",
            "Google Gemini",
            "Google Gemini",
            "Google Gemini",
            "Google Gemini",
        ),
    },
    Msg {
        key: "settings.providers.presets.omniDashscope",
        text: row(
            "阿里云百炼 Omni",
            "阿里雲百煉 Omni",
            "Alibaba DashScope Omni",
            "Alibaba DashScope Omni",
            "Alibaba DashScope Omni",
        ),
    },
    Msg { key: "settings.recording.history_retention_never", text: row("不按时间清理", "不按時間清理", "Never", "時間で削除しない", "시간 기준 삭제 안 함") },
    Msg { key: "settings.recording.history_retention_days", text: row("{} 天", "{} 天", "{} days", "{} 日", "{}일") },
    Msg { key: "channel.beta", text: row("Beta", "Beta", "Beta", "ベータ", "베타") },
    Msg {
        key: "style.pack.activate",
        text: row("激活", "啟用", "Activate", "有効化", "활성화"),
    },
    Msg {
        key: "style.pack.add_pack_tile_hint",
        text: row(
            "从空白模板开始。",
            "從空白範本開始。",
            "Start from a blank template.",
            "空のテンプレートから開始。",
            "빈 템플릿으로 시작.",
        ),
    },
    Msg {
        key: "style.pack.add_pack_tile_title",
        text: row("新建风格包", "新建風格包", "New Pack", "新規パック", "새 팩"),
    },
    Msg {
        key: "style.pack.dictation_prompt_hint",
        text: row(
            "用于录音转写后的 ASR 文本；这里可以写口语整理、ASR 错字纠正和专有名词还原规则。",
            "用於錄音轉寫後的 ASR 文本；這裡可以寫口語整理、ASR 錯字糾正和專有名詞還原規則。",
            "For ASR text after dictation; write spoken-language cleanup, ASR typo fixes and term restoration rules here.",
            "録音の書き起こし後のASRテキスト用。口語整理、ASR誤字修正、固有名詞の復元ルールをここに書けます。",
            "녹음 후 받아쓰기한 ASR 텍스트용. 구어 정리, ASR 오타 수정, 고유명사 복원 규칙을 여기에 작성하세요.",
        ),
    },
    Msg {
        key: "style.pack.edit",
        text: row("编辑", "編輯", "Edit", "編集", "편집"),
    },
    Msg {
        key: "style.pack.export_short",
        text: row("导出", "匯出", "Export", "エクスポート", "내보내기"),
    },
    Msg {
        key: "style.pack.import_zip",
        text: row("导入 ZIP", "匯入 ZIP", "Import ZIP", "ZIP をインポート", "ZIP 가져오기"),
    },
    Msg {
        key: "style.pack.imported",
        text: row("导入", "匯入", "Imported", "インポート", "가져옴"),
    },
    Msg {
        key: "style.pack.list_count",
        text: row("{} 个风格包", "{} 個風格包", "{} packs", "{} 個", "{}개"),
    },
        Msg {
        key: "style.pack.list_desc",
        text: row(
            "浏览和切换风格包。",
            "瀏覽和切換風格包。",
            "Browse and switch packs.",
            "パックを閲覧・切替。",
            "팩 둘러보기·전환.",
        ),
    },
    Msg {
        key: "style.pack.list_title",
        text: row("本地风格包", "本機風格包", "Local Packs", "ローカルパック", "로컬 팩"),
    },
    Msg {
        key: "marketplace.liked_empty",
        text: row(
            "你还没有赞过任何风格包",
            "你還沒有讚過任何風格包",
            "You have not liked any style packs yet",
            "まだいいねしたパックがありません",
            "아직 좋아요한 팩이 없습니다",
        ),
    },
    Msg {
        key: "marketplace.liked_empty_hint",
        text: row(
            "点开任一风格包，红色星星点亮后会出现在这里",
            "點開任一風格包，紅色星星點亮後會出現在這裡",
            "Open any pack and tap the star — liked packs appear here",
            "パックを開いて星をタップするとここに表示されます",
            "팩을 열고 별을 누르면 여기에 표시됩니다",
        ),
    },
    Msg {
        key: "modal.descriptions.about",
        text: row(
            "查看当前版本、更新渠道与自动更新设置。",
            "查看目前版本、更新管道與自動更新設定。",
            "View your version, update channel and automatic update settings.",
            "現在のバージョン、更新チャンネル、自動更新を確認します。",
            "현재 버전, 업데이트 채널 및 자동 업데이트 설정을 확인합니다.",
        ),
    },
    Msg {
        key: "modal.descriptions.advanced",
        text: row(
            "按需配置 Less Computer、多模态与调试功能。",
            "按需設定 Less Computer、多模態與除錯功能。",
            "Configure Less Computer, multimodal processing and debugging as needed.",
            "必要に応じて Less Computer、マルチモーダル処理、デバッグを設定します。",
            "필요에 따라 Less Computer, 멀티모달 처리 및 디버깅을 설정합니다.",
        ),
    },
    Msg {
        key: "modal.descriptions.appearance",
        text: row(
            "调整主题、页面排版和界面语言，让阅读更舒服。",
            "調整主題、頁面排版和介面語言，讓閱讀更舒服。",
            "Adjust the theme, page layout and interface language for comfortable reading.",
            "テーマ、レイアウト、表示言語を読みやすく調整します。",
            "테마, 페이지 배치, 인터페이스 언어를 편하게 읽도록 조정합니다.",
        ),
    },
    Msg {
        key: "modal.descriptions.general",
        text: row(
            "选择麦克风、设置录音方式与文字输入，也可连接手机输入。",
            "選擇麥克風、設定錄音方式與文字輸入，也可連接手機輸入。",
            "Choose a microphone, adjust recording and text input, or connect your phone.",
            "マイク、録音方法、文字入力を設定し、スマートフォンからの入力を接続します。",
            "마이크, 녹음 방식, 텍스트 입력을 설정하고 휴대폰 입력을 연결합니다.",
        ),
    },
    Msg {
        key: "modal.descriptions.privacy",
        text: row(
            "检查系统权限与连接状态，管理历史、录音和本地数据。",
            "檢查系統權限與連線狀態，管理歷史、錄音和本機資料。",
            "Check system permissions and connections. Manage history, recordings and local data.",
            "システム権限と接続を確認し、履歴、録音、ローカルデータを管理します。",
            "시스템 권한과 연결을 확인하고 기록, 녹음 및 로컬 데이터를 관리합니다.",
        ),
    },
    Msg {
        key: "modal.descriptions.services",
        text: row(
            "选择语音识别与文字处理服务，管理渠道、本地模型和网络连接。",
            "選擇語音辨識與文字處理服務，管理管道、本機模型和網路連線。",
            "Choose speech recognition and text processing services. Manage channels, local models and connections.",
            "音声認識と文章処理のサービス、チャンネル、ローカルモデル、接続を管理します。",
            "음성 인식과 텍스트 처리 서비스, 채널, 로컬 모델 및 연결을 관리합니다.",
        ),
    },
    Msg {
        key: "modal.descriptions.shortcuts",
        text: row(
            "设置各功能的触发方式，以及选中文字后的操作。",
            "設定各功能的觸發方式，以及選取文字後的操作。",
            "Set up shortcuts and choose what happens when you select text.",
            "各機能のショートカットと、テキスト選択後の操作を設定します。",
            "기능별 단축키와 텍스트 선택 후 동작을 설정합니다.",
        ),
    },
    Msg {
        key: "modal.advanced_pages.debug",
        text: row(
            "保留调试录音、探测光标上下文和导出日志。",
            "保留偵錯錄音、探測游標上下文與匯出日誌。",
            "Keep debug recordings, inspect cursor context, and export logs.",
            "デバッグ録音の保持、カーソル周辺の確認、ログの書き出しを行います。",
            "디버그 녹음을 보관하고 커서 문맥을 확인하며 로그를 내보냅니다.",
        ),
    },
    Msg {
        key: "modal.advanced_pages.less_computer",
        text: row(
            "选择 Agent，配置模型、权限与工作目录。",
            "選擇 Agent，設定模型、權限與工作目錄。",
            "Choose an agent and configure its model, permissions, and working directory.",
            "Agent を選び、モデル・権限・作業ディレクトリを設定します。",
            "Agent를 선택하고 모델, 권한, 작업 디렉터리를 설정합니다.",
        ),
    },
    Msg {
        key: "modal.advanced_pages.multimodal",
        text: row(
            "管理多模态识别的实验性开关。",
            "管理多模態辨識的實驗性開關。",
            "Manage the experimental multimodal recognition switch.",
            "実験的なマルチモーダル認識の有効・無効を設定します。",
            "실험적 멀티모달 인식 기능의 사용 여부를 설정합니다.",
        ),
    },
    Msg {
        key: "modal.service_views.omni",
        text: row("多模态模型", "多模態模型", "Multimodal", "マルチモーダル", "멀티모달"),
    },
    Msg {
        key: "settings.about.beta_channel_desc",
        text: row(
            "开启后，后台自动更新将跟随 Beta 渠道；关闭则回到正式版。下方按钮可随时手动检查 Beta 更新。",
            "開啟後，背景自動更新將跟隨 Beta 渠道；關閉則回到正式版。下方按鈕可隨時手動檢查 Beta 更新。",
            "When on, background auto-update follows Beta; when off, it uses stable. Use the button below to manually check Beta anytime.",
            "オンにするとバックグラウンド自動更新が Beta に従います。オフで正式版に戻ります。下のボタンでいつでも Beta を手動確認できます。",
            "켜면 백그라운드 자동 업데이트가 Beta를 따릅니다. 끄면 정식판으로 돌아갑니다. 아래 버튼으로 언제든 Beta를 수동 확인할 수 있습니다.",
        ),
    },
    Msg {
        key: "settings.about.check_beta_update_btn",
        text: row(
            "检查 Beta 更新",
            "檢查 Beta 更新",
            "Check Beta update",
            "Beta を確認",
            "Beta 확인",
        ),
    },
    Msg {
        key: "settings.advanced.multimodal_pipeline_hint",
        text: row(
            "开启后，「服务 → AI 提供商」页出现「传统模式 / 多模态模式」切换。传统 = ASR + LLM；多模态 = 单个支持音频的模型。两套配置分开存储、绝不共享凭据。",
            "開啟後，「服務 → AI 提供者」頁出現「傳統模式 / 多模態模式」切換。傳統 = ASR + LLM；多模態 = 單一支援音訊的模型。兩套設定分開儲存、絕不共用憑證。",
            "Adds a Traditional / Multimodal switch on the AI providers page. Traditional = ASR + LLM; Multimodal = one audio-capable model. The two configurations are stored separately and never share credentials.",
            "有効にすると「サービス → AI プロバイダー」ページに従来 / マルチモーダルの切り替えが表示されます。従来 = ASR + LLM、マルチモーダル = 音声対応モデル1つ。設定は別々に保存され、認証情報を共有しません。",
            "활성화하면 「서비스 → AI 공급자」 페이지에 전통 / 멀티모달 전환이 나타납니다. 전통 = ASR + LLM, 멀티모달 = 오디오 지원 모델 1개. 두 설정은 별도로 저장되며 자격 증명을 공유하지 않습니다.",
        ),
    },
    Msg {
        key: "settings.language.label_desc",
        text: row(
            "选择「跟随系统」时按操作系统当前语言显示。",
            "選擇「跟隨系統」時按操作系統當前語言顯示。",
            "Choose \"Follow system\" to match the OS language at launch.",
            "「システムに従う」を選ぶと OS の言語に合わせます。",
            "\"시스템 따라가기\"를 선택하면 OS 언어를 따릅니다.",
        ),
    },
    Msg {
        key: "settings.network.use_system_proxy_desc",
        text: row(
            "开启时请求跟随系统代理；关闭后所有网络请求直连（国内服务延迟通常更低），GitHub 登录、更新等境外服务可能连不上。实时语音流与 Less Computer 不受此开关影响。",
            "開啟時請求跟隨系統代理；關閉後所有網路請求直連（國內服務延遲通常更低），GitHub 登入、更新等境外服務可能連不上。即時語音串流與 Less Computer 不受此開關影響。",
            "When on, requests follow the system proxy. When off, all requests connect directly (usually lower latency for domestic services), but overseas services such as GitHub sign-in and updates may fail. Realtime voice streams and Less Computer are unaffected.",
            "オンにするとリクエストはシステムプロキシを経由します。オフにするとすべて直接接続します（国内サービスの遅延が低くなる傾向）。GitHub ログインやアップデートなど海外サービスには接続できない場合があります。リアルタイム音声ストリームと Less Computer は影響を受けません。",
            "켜면 요청이 시스템 프록시를 따릅니다. 끄면 모든 요청이 직결됩니다(국내 서비스는 보통 더 빠름). GitHub 로그인·업데이트 등 해외 서비스는 연결되지 않을 수 있습니다. 실시간 음성 스트림과 Less Computer는 영향을 받지 않습니다.",
        ),
    },
    Msg {
        key: "settings.permissions.denied",
        text: row("未授权", "未授權", "Not granted", "未許可", "허용되지 않음"),
    },
    Msg {
        key: "settings.permissions.not_applicable",
        text: row("无需授权", "無需授權", "Not required", "権限不要", "권한 불필요"),
    },
    Msg {
        key: "settings.recording.audio_cue_desc",
        text: row(
            "按下热键开始录音时播放一段合成提示音，提醒已开始录音。胶囊隐藏时也会响。",
            "按下熱鍵開始錄音時播放一段合成提示音，提醒已開始錄音。膠囊隱藏時也會響。",
            "Play a short synthesized chime when you press the hotkey to start recording. Plays even when the capsule is hidden.",
            "ホットキーで録音を開始するとき、合成した短い通知音を再生します。カプセルが非表示でも鳴ります。",
            "단축키로 녹음을 시작할 때 합성된 짧은 알림음을 재생합니다. 캡슐이 숨겨져 있어도 재생됩니다.",
        ),
    },
    Msg {
        key: "settings.recording.audio_recording_max_entries_desc",
        text: row(
            "本地保留 wav 文件数上限，留空 = 200。",
            "本地保留 wav 檔案數上限，留空 = 200。",
            "Max wav files retained locally. Blank = 200.",
            "ローカル保持 wav ファイル上限。空欄 = 200。",
            "로컬 보관 wav 파일 상한. 빈칸 = 200.",
        ),
    },
    Msg {
        key: "settings.recording.combo_disable_hint",
        text: row(
            "核心快捷键不可停用，录音必须绑定一个热键",
            "核心快捷鍵不可停用，錄音必須綁定一個快捷鍵",
            "Core hotkey cannot be disabled — recording needs a hotkey",
            "コアショートカットは無効化できません（録音にはショートカットが必須です）",
            "핵심 단축키는 비활성화할 수 없습니다 (녹음에는 단축키가 필수입니다)",
        ),
    },
    Msg {
        key: "settings.recording.microphone_desc",
        text: row(
            "选择优先输入设备。设备断开时自动切到系统默认。",
            "選擇優先使用的輸入設備。設備暫時不可用時會使用系統默認麥克風，重新連接後自動切回首選設備。",
            "Choose the preferred input device; falls back to system default when unavailable.",
            "優先して使用する入力デバイスを選択します。一時的に利用できない場合はシステムのデフォルトマイクを使い、再接続後に自動で優先デバイスへ戻します。",
            "우선 사용할 입력 장치를 선택합니다. 장치를 일시적으로 사용할 수 없으면 시스템 기본 마이크를 사용하고, 다시 연결되면 자동으로 우선 장치로 돌아갑니다.",
        ),
    },
    Msg {
        key: "settings.recording.mode_auto",
        text: row("自动", "自動", "Auto", "自動", "자동"),
    },
    Msg {
        key: "settings.recording.mode_desc",
        text: row(
            "切换式按一次开始、再按一次结束；按住说话按下保持、松开结束。",
            "切換式 = 按一次開始、再按一次結束；按住說話 = 按住開始、鬆開結束。",
            "Toggle = tap once to start, again to stop. Push-to-talk = hold to record.",
            "トグル式 = 1 回押して開始、もう 1 回押して終了；押し続けて話す = 押している間だけ録音。",
            "토글 방식 = 한 번 누르면 시작, 다시 누르면 종료; 눌러서 말하기 = 누르고 있는 동안만 녹음.",
        ),
    },
    Msg {
        key: "settings.recording.mode_hold",
        text: row("按住说话", "按住說話", "Push-to-talk", "押し続けて話す", "눌러서 말하기"),
    },
    Msg {
        key: "settings.recording.mute_during_recording_desc",
        text: row(
            "录音期间临时静音系统输出，避免扬声器回音。",
            "錄音期間臨時靜音系統輸出，避免揚聲器回音。",
            "Temporarily mute system output during voice input to avoid speaker echo.",
            "録音中にシステム出力を一時的にミュートし、スピーカーのエコーを防ぎます。",
            "녹음 중 시스템 출력을 일시적으로 음소거하여 스피커 에코를 방지합니다.",
        ),
    },
    Msg {
        key: "settings.recording.paste_shortcut_desc",
        text: row(
            "插入时模拟按下的粘贴键，部分终端类应用需要 Ctrl+Shift+V（仅 Windows / Linux）。",
            "插入時模擬按下的粘貼鍵，部分終端類應用需要 Ctrl+Shift+V（僅 Windows / Linux）。",
            "Which paste combo to simulate when inserting; some terminals need Ctrl+Shift+V (Windows / Linux only).",
            "挿入時に模擬するペーストショートカット。一部のターミナルでは Ctrl+Shift+V が必要（Windows / Linux のみ）。",
            "삽입 시 시뮬레이션할 붙여넣기 단축키. 일부 터미널은 Ctrl+Shift+V 가 필요 (Windows / Linux 만).",
        ),
    },
    Msg {
        key: "settings.recording.polish_context_window_desc",
        text: row(
            "把最近 N 分钟内已润色的转写作为多轮上下文，0 = 关闭。",
            "把最近 N 分鐘內已潤色的轉寫作為多輪上下文，0 = 關閉。",
            "Use the last N minutes of polished transcripts as multi-turn context; 0 = disabled.",
            "直近 N 分間の整文済み転写をマルチターン文脈として渡します。0 = 無効。",
            "최근 N 분간 정리된 전사를 멀티턴 컨텍스트로 전달합니다. 0 = 비활성화.",
        ),
    },
    Msg {
        key: "settings.recording.polish_context_window_label",
        text: row(
            "对话上下文窗口（分钟）",
            "對話上下文窗口（分鐘）",
            "Polish context window (minutes)",
            "会話コンテキスト窓（分）",
            "대화 컨텍스트 윈도(분)",
        ),
    },
    Msg {
        key: "settings.recording.restore_clipboard_desc",
        text: row(
            "粘贴成功后恢复你原来的剪贴板内容（仅 Windows / Linux）。",
            "粘貼成功後恢復你原來的剪貼板內容（僅 Windows / Linux）。",
            "Restore your original clipboard after a successful paste (Windows / Linux only).",
            "ペースト成功後に元のクリップボード内容を復元（Windows / Linux のみ）。",
            "붙여넣기 성공 후 원래 클립보드 내용을 복원합니다 (Windows / Linux 만).",
        ),
    },
    Msg {
        key: "settings.recording.silence_auto_stop_desc",
        text: row(
            "仅切换模式生效。检测到语音后，连续静音达到所选时长即自动结束并提交；一直没说话则 10 秒后取消。默认关闭；第二次按键停止和 Esc 取消仍然有效。",
            "僅切換模式生效。偵測到語音後，連續靜音達到所選時長即自動結束並提交；一直沒說話則 10 秒後取消。預設關閉；第二次按鍵停止和 Esc 取消仍然有效。",
            "Toggle only. After speech is detected, recording stops and submits automatically once silence lasts the chosen duration. Off by default; a second hotkey press and Esc still work.",
            "トグルモードのみ有効。音声を検出した後、無音が選択した時間続いたら録音を自動停止して送信します。一度も話さない場合は10秒後にキャンセル。既定ではオフで、2回目のキー押下による停止と Esc によるキャンセルは引き続き有効です。",
            "토글 모드에서만 동작합니다. 음성이 감지된 후 선택한 시간 동안 침묵이 이어지면 녹음을 자동으로 종료하고 제출합니다. 말을 전혀 하지 않으면 10초 후 취소됩니다. 기본적으로 꺼져 있으며, 두 번째 키 누름으로 중지하고 Esc로 취소하는 동작은 그대로 유지됩니다.",
        ),
    },
    Msg {
        key: "settings.remote_input.cert_fingerprint_copy",
        text: row(
            "复制完整指纹",
            "複製完整指紋",
            "Copy full fingerprint",
            "指紋全体をコピー",
            "전체 지문 복사",
        ),
    },
    Msg {
        key: "settings.remote_input.cert_fingerprint_label",
        text: row(
            "本机根证书 SHA-256",
            "本機根憑證 SHA-256",
            "This computer's root CA SHA-256",
            "このコンピューターのルート CA SHA-256",
            "이 컴퓨터의 루트 CA SHA-256",
        ),
    },
    Msg {
        key: "settings.remote_input.cert_fingerprint_unavailable",
        text: row(
            "完整指纹不可用。请勿安装或信任下载的证书。",
            "完整指紋無法取得。請勿安裝或信任下載的憑證。",
            "The full fingerprint is unavailable. Do not install or trust a downloaded certificate.",
            "完全な指紋を取得できません。ダウンロードした証明書をインストールしたり信頼したりしないでください。",
            "전체 지문을 확인할 수 없습니다. 다운로드한 인증서를 설치하거나 신뢰하지 마세요.",
        ),
    },
    Msg {
        key: "settings.remote_input.cert_verify_hint",
        text: row(
            "在手机系统的证书详情中找到 SHA-256，与这里的全部 64 个字符逐一核对（忽略空格和冒号）。必须在开启完全信任前完成。网页、描述文件名称和标识不能证明证书身份；若不一致或无法查看完整指纹，请停止并移除已下载或安装的描述文件。",
            "在手機系統的憑證詳細資訊中找到 SHA-256，與此處全部 64 個字元逐一核對（忽略空格和冒號）。必須在開啟完全信任前完成。網頁、描述檔名稱與識別碼不能證明憑證身分；若不一致或無法查看完整指紋，請停止並移除已下載或安裝的描述檔。",
            "Find SHA-256 in the phone's system certificate details and compare all 64 characters with this value (ignore spaces and colons) before enabling full trust. A web page, profile name or identifier cannot prove identity. If the fingerprint differs or cannot be viewed in full, stop and remove the downloaded or installed profile.",
            "スマートフォンのシステム証明書詳細にある SHA-256 の全 64 文字を、空白とコロンを除いてこの値と照合し、完全に信頼する前に確認してください。Web ページ、プロファイル名や識別子は身元の証明にはなりません。一致しない場合や全体を表示できない場合は中止し、ダウンロード済みまたはインストール済みのプロファイルを削除してください。",
            "휴대폰 시스템의 인증서 상세 정보에서 SHA-256을 찾아, 완전한 신뢰를 켜기 전에 공백과 콜론을 제외한 64자 전체를 이 값과 비교하세요. 웹 페이지, 프로파일 이름이나 식별자는 신원 증명이 아닙니다. 일치하지 않거나 전체 지문을 볼 수 없으면 중단하고 다운로드했거나 설치한 프로파일을 제거하세요.",
        ),
    },
    Msg {
        key: "settings.remote_input.pin_label",
        text: row("配对码", "配對碼", "Pairing code", "ペアリングコード", "페어링 코드"),
    },
    Msg {
        key: "settings.remote_input.security_hint",
        text: row(
            "仅同一局域网可访问，需输入配对码；不用时建议关闭。",
            "僅同一區域網路可存取，需輸入配對碼；不用時建議關閉。",
            "Reachable only on the same LAN and requires the pairing code; turn it off when not in use.",
            "同一 LAN からのみアクセス可能で、ペアリングコードの入力が必要です。使わないときはオフにすることを推奨します。",
            "같은 LAN에서만 접속 가능하며 페어링 코드 입력이 필요합니다. 사용하지 않을 때는 끄는 것을 권장합니다.",
        ),
    },
    Msg {
        key: "settings.remote_input.url_label",
        text: row("访问网址", "存取網址", "Access URL", "アクセス URL", "접속 URL"),
    },
    Msg {
        key: "settings.selection_polish.direct_replace",
        text: row(
            "直接覆盖",
            "直接覆蓋",
            "Replace directly",
            "直接置き換え",
            "직접 교체",
        ),
    },
    Msg {
        key: "settings.selection_polish.preview_confirm",
        text: row(
            "预览确认",
            "預覽確認",
            "Preview & confirm",
            "プレビューして確認",
            "미리보기 후 확인",
        ),
    },
    Msg {
        key: "settings.selection_workspace.polish_delivery",
        text: row(
            "结果处理",
            "結果處理",
            "Result handling",
            "結果の処理",
            "결과 처리",
        ),
    },
    Msg {
        key: "settings.selection_workspace.polish_hotkey",
        text: row(
            "选区助手快捷键",
            "選區助手快捷鍵",
            "Selection assistant shortcut",
            "選択範囲アシスタントのショートカット",
            "선택 영역 도우미 단축키",
        ),
    },
    Msg {
        key: "settings.selection_workspace.polish_hotkey_desc",
        text: row(
            "关闭语音编辑时直接润色；开启语音编辑时按住口述指令（录音方式跟随全局设置）。",
            "關閉語音編輯時直接潤色；開啟語音編輯時按住口述指令（錄音方式跟隨全域設定）。",
            "Polishes directly when voice edit is off; hold to speak when voice edit is on (recording follows global settings).",
            "音声編集オフ時は推敲、オン時は押しながら話す（録音方式はグローバル設定に従う）。",
            "음성 편집 끄면 바로 다듬기, 켜면 누른 채 말하기(녹음 방식은 전역 설정 따름).",
        ),
    },
    Msg {
        key: "settings.shortcuts.style_pack_desc",
        text: row(
            "为常用风格包各配一个快捷键，按下直接切换；停用中的包会自动启用。",
            "為常用風格包各配一個快捷鍵，按下直接切換；停用中的包會自動啟用。",
            "Bind a shortcut to each favorite style pack for one-press switching; disabled packs are re-enabled automatically.",
            "よく使うスタイルパックにショートカットを割り当てて一発切替；無効中のパックは自動で有効化されます。",
            "자주 쓰는 스타일 팩에 단축키를 지정해 한 번에 전환합니다. 비활성화된 팩은 자동으로 다시 활성화됩니다.",
        ),
    },
    Msg {
        key: "modal.about.docs_btn",
        text: row(
            "openless.app/docs ↗",
            "openless.app/docs ↗",
            "openless.app/docs ↗",
            "openless.app/docs ↗",
            "openless.app/docs ↗",
        ),
    },
    Msg {
        key: "modal.about.feedback_btn",
        text: row(
            "GitHub Issues ↗",
            "GitHub Issues ↗",
            "GitHub Issues ↗",
            "GitHub Issues ↗",
            "GitHub Issues ↗",
        ),
    },
    Msg {
        key: "settings.coding_agent.coming_soon_note",
        text: row(
            "配置即时保存；热键触发与执行链路随后续版本生效。",
            "設定即時儲存；熱鍵觸發與執行鏈路隨後續版本生效。",
            "Config is saved now; hotkey triggering and the execution flow land in a later version.",
            "設定はすぐ保存されます。ホットキー起動と実行フローは今後のバージョンで対応。",
            "설정은 즉시 저장됩니다. 단축키 트리거와 실행 흐름은 이후 버전에서 제공됩니다.",
        ),
    },
    Msg {
        key: "settings.coding_agent.exe",
        text: row(
            "可执行文件路径",
            "可執行檔路徑",
            "Executable path",
            "実行ファイルのパス",
            "실행 파일 경로",
        ),
    },
    Msg {
        key: "settings.coding_agent.hotkey_hint",
        text: row(
            "开启后，按住快捷键说话，松开后由所选 Agent 处理并把结果显示在胶囊里。",
            "開啟後，按住快捷鍵說話，放開後由所選 Agent 處理並把結果顯示在膠囊裡。",
            "When enabled, hold the shortcut to talk; release it and the selected agent shows the result in the capsule.",
            "有効にすると、ショートカットを押しながら話し、離すと選択した Agent の結果がカプセルに表示されます。",
            "켜면 단축키를 누른 채 말하고, 놓으면 선택한 Agent 결과가 캡슐에 표시됩니다.",
        ),
    },
    Msg {
        key: "settings.coding_agent.model",
        text: row("模型", "模型", "Model", "モデル", "모델"),
    },
    Msg {
        key: "settings.coding_agent.model_hint",
        text: row(
            "Haiku 最快 · Sonnet 均衡 · Opus 最强",
            "Haiku 最快 · Sonnet 均衡 · Opus 最強",
            "Haiku = fastest · Sonnet = balanced · Opus = strongest",
            "Haiku = 最速 · Sonnet = バランス · Opus = 最強",
            "Haiku = 가장 빠름 · Sonnet = 균형 · Opus = 최강",
        ),
    },
    Msg {
        key: "settings.coding_agent.model_placeholder",
        text: row(
            "默认 sonnet",
            "預設 sonnet",
            "Default: sonnet",
            "デフォルト: sonnet",
            "기본: sonnet",
        ),
    },
    Msg {
        key: "settings.coding_agent.provider",
        text: row(
            "Agent 后端",
            "Agent 後端",
            "Agent backend",
            "Agent バックエンド",
            "Agent 백엔드",
        ),
    },
    Msg {
        key: "settings.coding_console.mode.accept_edits",
        text: row(
            "放行（可恢复操作）",
            "放行（可復原操作）",
            "Allow (reversible)",
            "許可（復元可能）",
            "허용(복구 가능)",
        ),
    },
    Msg {
        key: "settings.coding_console.mode.bypass_permissions",
        text: row(
            "完全放行（高风险）",
            "完全放行（高風險）",
            "Full bypass (risky)",
            "完全許可（高リスク）",
            "완전 허용(위험)",
        ),
    },
    Msg {
        key: "settings.coding_console.mode.default",
        text: row(
            "默认（逐项确认）",
            "預設（逐項確認）",
            "Default (ask each)",
            "デフォルト（都度確認）",
            "기본(매번 확인)",
        ),
    },
    Msg {
        key: "settings.coding_console.mode.plan",
        text: row(
            "只读 / 计划",
            "唯讀 / 計畫",
            "Read-only / plan",
            "読み取り専用 / 計画",
            "읽기 전용 / 계획",
        ),
    },
    Msg {
        key: "settings.coding_console.workdir_desc",
        text: row(
            "可选。Claude 在此目录内运行；填写 git 仓库可启用运行前快照回滚。",
            "選填。Claude 在此目錄內執行；填入 git 儲存庫可啟用執行前快照回滾。",
            "Optional. Claude runs inside this dir; a git repo enables a pre-run snapshot for rollback.",
            "任意。Claude はこのディレクトリ内で実行。git リポジトリなら実行前スナップショットで巻き戻し可能。",
            "선택 사항. Claude 가 이 디렉터리에서 실행됩니다. git 저장소이면 실행 전 스냅샷으로 되돌릴 수 있습니다.",
        ),
    },
    Msg {
        key: "settings.coding_console.workdir_placeholder",
        text: row(
            "留空则在临时目录运行",
            "留空則於暫存目錄執行",
            "Empty = run in a temp dir",
            "空欄なら一時ディレクトリで実行",
            "비우면 임시 디렉터리에서 실행",
        ),
    },
    Msg {
        key: "common.experimental",
        text: row("实验性", "實驗性", "Experimental", "実験的", "실험적"),
    },
    Msg {
        key: "hotkey.mode_auto_suffix",
        text: row(
            "（自动识别）",
            "（自動識別）",
            " (auto-detect)",
            "（自動判別）",
            "(자동 인식)",
        ),
    },
    Msg {
        key: "hotkey.mode_hold_suffix",
        text: row(
            "（按住说话）",
            "（按住說話）",
            " (push-to-talk)",
            "（押し続けて話す）",
            "(눌러서 말하기)",
        ),
    },
    Msg {
        key: "hotkey.mode_toggle_suffix",
        text: row(
            "（开始 / 停止）",
            "（開始 / 停止）",
            " (start / stop)",
            "（開始 / 停止）",
            "(시작 / 정지)",
        ),
    },
    Msg {
        key: "settings.coding_agent.voice_hotkey_desc",
        text: row(
            "按住说话、松开执行。支持 Ctrl/Option/Fn 等单键。功能说明参见「高级」设置页。",
            "按住說話、放開執行。支援 Ctrl/Option/Fn 等單鍵。功能說明參見「進階」設定頁。",
            "Hold to talk, release to run. Supports Ctrl/Option/Fn single keys. See the Advanced settings page for what it does.",
            "押して話す、離して実行。Ctrl/Option/Fn などの単キー対応。機能の説明は「詳細」設定ページを参照。",
            "누르고 말하고 놓으면 실행. Ctrl/Option/Fn 단일 키 지원. 기능 설명은 「고급」 설정 페이지 참조.",
        ),
    },
    Msg {
        key: "settings.recording.combo_conflict",
        text: row(
            "该快捷键组合不可用",
            "此快捷鍵組合不可用",
            "This shortcut combination is not available",
            "このショートカットの組み合わせは使用できません",
            "이 단축키 조합은 사용할 수 없습니다",
        ),
    },
    Msg {
        key: "settings.recording.combo_record_btn",
        text: row(
            "录制快捷键",
            "錄製快捷鍵",
            "Record shortcut",
            "ショートカットを記録",
            "단축키 녹화",
        ),
    },
    Msg {
        key: "settings.recording.combo_record_hint",
        text: row(
            "请按下快捷键组合…",
            "請按下快捷鍵組合…",
            "Press your shortcut combination…",
            "ショートカットの組み合わせを押してください…",
            "단축키 조합을 눌러 주세요…",
        ),
    },
    Msg {
        key: "settings.shortcuts.disable",
        text: row("停用", "停用", "Disable", "無効化", "비활성화"),
    },
    Msg {
        key: "settings.shortcuts.style_pack_disabled_suffix",
        text: row("（已停用）", "（已停用）", " (disabled)", "（無効）", " (비활성화됨)"),
    },
    Msg {
        key: "settings.shortcuts.style_pack_remove",
        text: row("移除", "移除", "Remove", "削除", "제거"),
    },
    Msg {
        key: "capsule.cancelled",
        text: row("已取消", "已取消", "Cancelled", "キャンセルしました", "취소됨"),
    },
    Msg {
        key: "capsule.error",
        text: row(
            "出错了",
            "出錯了",
            "Something went wrong",
            "エラーが発生しました",
            "오류 발생",
        ),
    },
    Msg {
        key: "capsule.inserted",
        text: row("已插入 {}", "已插入 {}", "Inserted {}", "{} 文字を入力しました", "{}자 입력됨"),
    },
    Msg {
        key: "capsule.thinking",
        text: row("thinking", "thinking", "thinking", "thinking", "thinking"),
    },
    Msg {
        key: "qa.close_tooltip",
        text: row("关闭", "關閉", "Close", "閉じる", "닫기"),
    },
    Msg {
        key: "qa.composer_placeholder",
        text: row(
            "输入问题，Enter 发送",
            "輸入問題，Enter 發送",
            "Type a question. Enter to send",
            "質問を入力。Enter で送信",
            "질문을 입력하세요. Enter로 보내기",
        ),
    },
    Msg {
        key: "qa.empty_desc",
        text: row(
            "选中任意文字后开始追问，或直接在下方输入问题。回答会显示在这里，可以连续多轮。",
            "選中任意文字後開始追問，或直接在下方輸入問題。回答會顯示在這裏，可以連續多輪。",
            "Select any text to ask about it, or just type your question below. Answers appear here — ask as many follow-ups as you like.",
            "テキストを選択して質問するか、下に直接入力してください。回答はここに表示され、続けて質問できます。",
            "텍스트를 선택해 질문하거나 아래에 직접 입력하세요. 답변이 여기에 표시되며 계속 이어서 질문할 수 있습니다.",
        ),
    },
    Msg {
        key: "qa.empty_title",
        text: row(
            "有什么可以帮你？",
            "有什麼可以幫你？",
            "How can I help?",
            "ご用件は？",
            "무엇을 도와드릴까요?",
        ),
    },
    Msg {
        key: "qa.error_retry_hint",
        text: row(
            "请再试一次。",
            "請再試一次。",
            "Please try again.",
            "もう一度お試しください。",
            "다시 시도해 주세요.",
        ),
    },
    Msg {
        key: "qa.header_hint",
        text: row("随时提问", "隨時提問", "Ask anytime", "いつでも質問", "언제든 질문하세요"),
    },
    Msg {
        key: "qa.selection_preview",
        text: row(
            "基于选中文本：",
            "基於選中文本：",
            "From selected text:",
            "選択テキスト：",
            "선택된 텍스트 기반:",
        ),
    },
    Msg {
        key: "qa.thinking",
        text: row("思考中…", "思考中…", "Thinking…", "思考中…", "생각 중…"),
    },
    Msg {
        key: "qa.title",
        text: row("划词追问", "劃詞追問", "Ask", "質問", "질문"),
    },
    Msg {
        key: "selection.polish_preview.cancel",
        text: row("取消", "取消", "Cancel", "キャンセル", "취소"),
    },
    Msg {
        key: "selection.polish_preview.confirm_replace",
        text: row(
            "确认并替换",
            "確認並替換",
            "Confirm & replace",
            "確認して置き換え",
            "확인 후 교체",
        ),
    },
    Msg {
        key: "selection.polish_preview.source_prefix",
        text: row("原文：", "原文：", "Original: ", "原文：", "원문: "),
    },
        Msg {
        key: "selection.polish_preview.subtitle",
        text: row(
            "只读结果；点「确认并替换」才会写回原选区。",
            "唯讀結果；點「確認並替換」才會寫回原選區。",
            "Read-only result; press “Confirm & replace” to write it back.",
            "読み取り専用の結果です。確定すると選択範囲に書き戻します。",
            "읽기 전용 결과입니다. 확인하면 선택 영역에 씁니다.",
        ),
    },
    Msg {
        key: "selection.polish_preview.title",
        text: row(
            "选区润色预览",
            "選區潤色預覽",
            "Selection Polish Preview",
            "選択範囲の推敲プレビュー",
            "선택 영역 다듬기 미리보기",
        ),
    },
    Msg {
        key: "capsule.translating",
        text: row("正在翻译", "正在翻譯", "Translating", "翻訳中", "번역 중"),
    },
    Msg {
        key: "qa.edit_apply_replace",
        text: row(
            "预览并确认插入",
            "確認並替換選區",
            "Preview and confirm insert",
            "プレビューして挿入を確認",
            "미리보기 후 삽입 확인",
        ),
    },
    Msg {
        key: "qa.edit_instruction_mode",
        text: row(
            "编辑指令",
            "編輯指令",
            "Edit instruction",
            "編集指示",
            "편집 지시",
        ),
    },
    Msg {
        key: "qa.edit_revert_previous",
        text: row(
            "保留上一版本",
            "保留上一版本",
            "Keep previous version",
            "前のバージョンを保持",
            "이전 버전 유지",
        ),
    },
    Msg {
        key: "qa.pin_tooltip",
        text: row(
            "固定（不自动关闭）",
            "固定（不自動關閉）",
            "Pin (stay open)",
            "ピン留め（自動で閉じない）",
            "고정(자동으로 닫히지 않음)",
        ),
    },
    Msg {
        key: "qa.unpin_tooltip",
        text: row("取消固定", "取消固定", "Unpin", "ピン留めを解除", "고정 해제"),
    },
    Msg {
        key: "less_computer.approval_rerun_warning",
        text: row(
            "注意：批准后将在已被修改的工作区上重新运行，可能对不可重入操作产生副作用",
            "注意：批准後將在已被修改的工作區上重新執行，可能對不可重入操作產生副作用",
            "Note: approving re-runs on an already-modified workspace and may have side effects on non-idempotent operations.",
            "注意：承認すると、すでに変更されたワークスペース上で再実行され、冪等でない操作に副作用が生じる可能性があります。",
            "주의: 승인하면 이미 수정된 작업 공간에서 다시 실행되어 멱등하지 않은 작업에 부작용이 생길 수 있습니다.",
        ),
    },
    Msg {
        key: "less_computer.approval_title",
        text: row(
            "执行被拦截的命令？",
            "執行被攔截的指令？",
            "Run blocked command?",
            "ブロックされたコマンドを実行？",
            "차단된 명령을 실행할까요?",
        ),
    },
    Msg {
        key: "less_computer.approve",
        text: row("允许", "允許", "Approve", "許可", "허용"),
    },
    Msg {
        key: "less_computer.compaction",
        text: row(
            "上下文已压缩",
            "上下文已壓縮",
            "Context compacted",
            "コンテキストを圧縮しました",
            "컨텍스트가 압축되었습니다",
        ),
    },
    Msg {
        key: "less_computer.cost",
        text: row("${}", "${}", "${}", "${}", "${}"),
    },
    Msg {
        key: "less_computer.deny",
        text: row("拒绝", "拒絕", "Deny", "拒否", "거부"),
    },
    Msg {
        key: "less_computer.input_placeholder",
        text: row(
            "输入指令，Enter 发送",
            "輸入指令，Enter 傳送",
            "Type a command, Enter to send",
            "指示を入力、Enter で送信",
            "명령을 입력하고 Enter로 전송",
        ),
    },
    Msg {
        key: "less_computer.send",
        text: row("发送", "傳送", "Send", "送信", "전송"),
    },
    Msg {
        key: "less_computer.subtitle",
        text: row(
            "想让电脑做什么？",
            "想讓電腦做什麼？",
            "What should your computer do?",
            "コンピュータに何をさせますか？",
            "컴퓨터로 무엇을 할까요?",
        ),
    },
    Msg {
        key: "less_computer.title",
        text: row(
            "Less Computer",
            "Less Computer",
            "Less Computer",
            "Less Computer",
            "Less Computer",
        ),
    },
    Msg {
        key: "less_computer.tool",
        text: row("调用了 {}", "呼叫了 {}", "Used {}", "{} を使用", "{} 사용"),
    },
    Msg {
        key: "less_computer.working",
        text: row("正在操控电脑…", "正在操控電腦…", "Operating…", "操作中…", "조작 중…"),
    },
    Msg {
        key: "onboarding.mic_no_device_hint",
        text: row(
            "未检测到麦克风，请连接并启用麦克风后重试。",
            "未偵測到麥克風，請連接並啟用麥克風後重試。",
            "No microphone detected. Connect and enable a microphone, then retry.",
            "マイクが検出されません。マイクを接続して有効にしてから、もう一度お試しください。",
            "마이크가 감지되지 않습니다. 마이크를 연결하고 활성화한 후 다시 시도하세요.",
        ),
    },
    Msg {
        key: "settings.recording.audio_cue_preview",
        text: row("试听", "試聽", "Preview", "試聴", "미리듣기"),
    },
    Msg {
        key: "settings.recording.capsule_desc",
        text: row(
            "录音 / 转写时显示屏幕底部胶囊。",
            "錄音 / 轉寫時在屏幕底部顯示半透明膠囊。",
            "Show a translucent capsule at the bottom of the screen while recording.",
            "録音 / 転写中、画面下部に半透明のカプセルを表示。",
            "녹음 / 전사 중 화면 하단에 반투명 캡슐을 표시합니다.",
        ),
    },
    Msg {
        key: "settings.recording.capsule_label",
        text: row(
            "录音胶囊",
            "錄音膠囊",
            "Recording capsule",
            "録音カプセル",
            "녹음 캡슐",
        ),
    },
    Msg {
        key: "settings.recording.capsule_style_classic",
        text: row(
            "Openless 默认风格",
            "Openless 預設風格",
            "OpenLess default style",
            "Openless デフォルトスタイル",
            "Openless 기본 스타일",
        ),
    },
    Msg {
        key: "settings.recording.capsule_style_label",
        text: row(
            "胶囊样式",
            "膠囊樣式",
            "Capsule style",
            "カプセルスタイル",
            "캡슐 스타일",
        ),
    },
    Msg {
        key: "settings.recording.capsule_style_siri",
        text: row(
            "流光 Siri 风格",
            "流光 Siri 風格",
            "Shimmer Siri style",
            "光条 Siri スタイル",
            "시리 광선 스타일",
        ),
    },
    Msg {
        key: "settings.recording.capsule_style_typeless",
        text: row(
            "Typeless 传统风格",
            "Typeless 傳統風格",
            "Typeless compact style",
            "Typeless コンパクトスタイル",
            "Typeless 컴팩트 스타일",
        ),
    },
    Msg {
        key: "settings.recording.microphone_load_error",
        text: row(
            "麦克风列表读取失败：{}",
            "麥克風列表讀取失敗：{}",
            "Failed to load microphones: {}",
            "マイクの読み込みに失敗：{}",
            "마이크 로드 실패: {}",
        ),
    },
    Msg {
        key: "settings.recording.stable_transcription_desc",
        text: row(
            "开启后，录音期间不连接 ASR，停止后才提交整段音频。结果出现更晚，但录音不受建连延迟和录音期间网络抖动影响。",
            "開啟後，錄音期間不連接 ASR，停止後才提交整段音訊。結果會較晚出現，但錄音不受連線延遲和錄音期間的網路波動影響。",
            "When enabled, ASR connects only after recording stops and receives the complete audio. Results arrive later, but connection delays and network instability during recording cannot interrupt capture.",
            "有効にすると、録音中は ASR に接続せず、停止後に音声全体を送信します。結果は遅くなりますが、接続遅延や録音中のネットワーク変動に録音が影響されません。",
            "켜면 녹음 중에는 ASR에 연결하지 않고 중지한 뒤 전체 오디오를 전송합니다. 결과는 늦게 표시되지만 연결 지연이나 녹음 중 네트워크 불안정이 녹음에 영향을 주지 않습니다.",
        ),
    },
    Msg {
        key: "settings.recording.stable_transcription_label",
        text: row(
            "稳定模式（先录音后识别）",
            "穩定模式（先錄音後辨識）",
            "Stable mode (record, then transcribe)",
            "安定モード（録音後に文字起こし）",
            "안정 모드 (녹음 후 전사)",
        ),
    },
    Msg {
        key: "capsule.selectionPolish.noSelection",
        text: row(
            "未选中内容",
            "未選中內容",
            "Nothing selected",
            "選択されていません",
            "선택된 내용 없음",
        ),
    },
    Msg {
        key: "settings.channels.availableModels",
        text: row(
            "可用模型",
            "可用模型",
            "Available models",
            "利用可能なモデル",
            "사용 가능한 모델",
        ),
    },
    Msg {
        key: "settings.channels.modelHint",
        text: row(
            "直接输入模型名称，或拉取并选择供应商的可用模型。",
            "直接輸入模型名稱，或取得並選擇供應商的可用模型。",
            "Enter a model name directly, or fetch and select a model from your provider.",
            "モデル名を直接入力するか、プロバイダーから一覧を取得して選択します。",
            "모델 이름을 직접 입력하거나 공급자의 모델 목록을 가져와 선택하세요.",
        ),
    },
    Msg {
        key: "settings.channels.modelTitle",
        text: row(
            "模型设置",
            "模型設定",
            "Model settings",
            "モデル設定",
            "모델 설정",
        ),
    },
    Msg {
        key: "settings.providers.customModelLabel",
        text: row(
            "自定义模型…",
            "自訂模型…",
            "Custom model…",
            "カスタムモデル…",
            "사용자 정의 모델…",
        ),
    },
    Msg {
        key: "settings.providers.fetchModels",
        text: row("拉取模型", "拉取模型", "Fetch models", "モデル一覧", "모델 가져오기"),
    },
    Msg {
        key: "settings.providers.loadingModels",
        text: row(
            "拉取模型中…",
            "拉取模型中…",
            "Fetching models…",
            "モデル取得中…",
            "모델 가져오는 중…",
        ),
    },
    Msg {
        key: "settings.providers.modelLabel",
        text: row("模型", "模型", "Model", "モデル", "모델"),
    },
    Msg {
        key: "settings.providers.modelSaved",
        text: row(
            "已保存模型 {}。",
            "已保存模型 {}。",
            "Saved model {}.",
            "モデル {} を保存しました。",
            "모델 {} 을(를) 저장했습니다.",
        ),
    },
    Msg {
        key: "settings.providers.modelsEmpty",
        text: row(
            "鉴权成功，但没有返回可用模型。",
            "鑑權成功，但沒有返回可用模型。",
            "Credentials are valid, but no models were returned.",
            "認証成功ですが、利用可能なモデルが返されませんでした。",
            "인증 성공이지만 사용 가능한 모델이 반환되지 않았습니다.",
        ),
    },
    Msg {
        key: "settings.providers.modelsLoaded",
        text: row(
            "已拉取 {} 个模型。",
            "已拉取 {} 個模型。",
            "Fetched {} models.",
            "{} 個のモデルを取得しました。",
            "{}개의 모델을 가져왔습니다.",
        ),
    },
    Msg {
        key: "settings.providers.noMatchingModels",
        text: row(
            "没有匹配的模型",
            "沒有符合的模型",
            "No matching models",
            "一致するモデルがありません",
            "일치하는 모델이 없습니다",
        ),
    },
    Msg {
        key: "settings.providers.planModelsHint",
        text: row(
            "打开套餐控制台，复制支持的文本模型 ID 并填写到模型栏。",
            "開啟方案控制台，複製支援的文字模型 ID 並填入模型欄位。",
            "Open the plan console, copy a supported text model ID, and enter it in the model field.",
            "プランのコンソールで対応するテキストモデル ID をコピーし、モデル欄に入力してください。",
            "요금제 콘솔에서 지원되는 텍스트 모델 ID를 복사해 모델 필드에 입력하세요.",
        ),
    },
    Msg {
        key: "settings.providers.presetListLabel",
        text: row(
            "返回预设列表",
            "返回預設清單",
            "Back to presets",
            "プリセットに戻る",
            "프리셋으로 돌아가기",
        ),
    },
    Msg {
        key: "settings.providers.searchModels",
        text: row(
            "搜索模型…",
            "搜尋模型…",
            "Search models…",
            "モデルを検索…",
            "모델 검색…",
        ),
    },
    Msg {
        key: "settings.providers.selectModel",
        text: row(
            "选择一个模型写入上方字段",
            "選擇一個模型寫入上方字段",
            "Select a model to fill the field above",
            "モデルを選んで上記欄に入力",
            "모델을 선택해 위 필드에 입력",
        ),
    },
    Msg {
        key: "settings.providers.viewModels",
        text: row(
            "查看支持的模型",
            "查看支援的模型",
            "View supported models",
            "対応モデルを確認",
            "지원 모델 보기",
        ),
    },
    Msg {
        key: "history.hide_raw",
        text: row(
            "隐藏原文",
            "隱藏原文",
            "Hide raw transcript",
            "原文を隠す",
            "원문 숨기기",
        ),
    },
    Msg {
        key: "history.play_recording",
        text: row(
            "播放录音",
            "播放錄音",
            "Play recording",
            "録音を再生",
            "녹음 재생",
        ),
    },
    Msg {
        key: "history.show_raw",
        text: row(
            "查看原文",
            "查看原文",
            "Show raw transcript",
            "原文を表示",
            "원문 보기",
        ),
    },
    Msg {
        key: "shell.beta_tag",
        text: row("BETA", "BETA", "BETA", "BETA", "BETA"),
    },
    Msg {
        key: "common.close",
        text: row("关闭", "關閉", "Close", "閉じる", "닫기"),
    },
    Msg {
        key: "vocab.newWordAddSelected",
        text: row("添加所选", "新增所選", "Add Selected", "選択を追加", "선택 추가"),
    },
    Msg {
        key: "vocab.newWordDesc",
        text: row(
            "直接输入新词，或从预设模板批量导入。",
            "直接輸入新詞，或從預設範本批次匯入。",
            "Type a word directly, or import preset templates in bulk.",
            "単語を直接入力、またはプリセットテンプレートから一括インポート。",
            "단어를 직접 입력하거나 프리셋 템플릿에서 일괄 가져오세요.",
        ),
    },
    Msg {
        key: "vocab.newWordInputPlaceholder",
        text: row(
            "输入词语，按 Enter 添加…",
            "輸入詞語，按 Enter 新增…",
            "Type a word, press Enter to add…",
            "単語を入力して Enter で追加…",
            "단어 입력 후 Enter로 추가…",
        ),
    },
    Msg {
        key: "vocab.newWordTemplates",
        text: row(
            "预设模板",
            "預設範本",
            "Preset Templates",
            "プリセットテンプレート",
            "프리셋 템플릿",
        ),
    },
    Msg {
        key: "vocab.newWordTitle",
        text: row(
            "添加新词",
            "新增新詞",
            "Add New Words",
            "新語を追加",
            "새 단어 추가",
        ),
    },
    Msg {
        key: "history.repolish.empty",
        text: row(
            "（模型返回了空结果）",
            "（模型返回了空結果）",
            "(the model returned an empty result)",
            "（モデルが空の結果を返しました）",
            "(모델이 빈 결과를 반환했습니다)",
        ),
    },
    Msg {
        key: "history.repolish.hint",
        text: row(
            "基于上面的原文再跑一次润色。结果只在本次查看时显示，不写回这条记录。原风格包已删除或旧记录时，重试将使用当前风格。",
            "基於上面的原文再跑一次潤色。結果只在本次查看時顯示，不寫回這條記錄。原風格包已刪除或舊記錄時，重試將使用當前風格。",
            "Run polish again on the transcript above. Results are shown for this visit only and are not written back to the record. When the original style pack was deleted or the record predates style packs, retry uses the current style.",
            "上の原文でもう一度整文を実行します。結果は今回の表示のみで、この記録には書き戻しません。元のスタイルパックが削除されているか、古い記録の場合は、再試行では現在のスタイルを使用します。",
            "위 원문으로 다듬기를 다시 실행합니다. 결과는 이번 조회에만 표시되며 기록에 반영되지 않습니다. 원래 스타일 팩이 삭제되었거나 오래된 기록인 경우, 다시 시도 시 현재 스타일을 사용합니다.",
        ),
    },
    Msg {
        key: "history.repolish.retry",
        text: row(
            "用原风格重试",
            "用原風格重試",
            "Retry with same style",
            "同じスタイルで再試行",
            "같은 스타일로 재시도",
        ),
    },
    Msg {
        key: "history.repolish.retryResultTitle",
        text: row("重试结果", "重試結果", "Retry result", "再試行の結果", "재시도 결과"),
    },
    Msg {
        key: "history.repolish.title",
        text: row("重新润色", "重新潤色", "Re-polish", "再整文", "다시 다듬기"),
    },
    Msg {
        key: "marketplace.myPacks.buttonLabel",
        text: row("我的发布", "我的發布", "My Packs", "自分の公開", "내 게시물"),
    },
    Msg {
        key: "marketplace.myPacks.searchPlaceholder",
        text: row(
            "搜索名称、标签",
            "搜尋名稱、標籤",
            "Search name or tags",
            "名前・タグを検索",
            "이름·태그 검색",
        ),
    },
    Msg {
        key: "marketplace.myPacks.summary",
        text: row("已发布 {} 个风格包", "已發布 {} 個風格包", "{} published", "公開済み {} 個", "게시 {}개"),
    },
    Msg {
        key: "style.pack.derivativeBadge",
        text: row(
            "衍生自 @{}",
            "衍生自 @{}",
            "Derived from @{}",
            "@{} から派生",
            "@{}에서 파생",
        ),
    },
    Msg {
        key: "style.pack.fieldDescription",
        text: row("描述", "描述", "Description", "説明", "설명"),
    },
    Msg {
        key: "style.pack.fieldName",
        text: row("名称", "名稱", "Name", "名前", "이름"),
    },
    Msg {
        key: "style.pack.fieldTags",
        text: row("标签", "標籤", "Tags", "タグ", "태그"),
    },
    Msg {
        key: "style.pack.fieldTagsPlaceholder",
        text: row(
            "用英文逗号分隔，例如 community, voiceover, formal",
            "用英文逗號分隔，例如 community, voiceover, formal",
            "Comma-separated tags, e.g. community, voiceover, formal",
            "カンマ区切り、例: community, voiceover, formal",
            "쉼표로 구분, 예: community, voiceover, formal",
        ),
    },
    Msg {
        key: "style.pack.selectionPromptHint",
        text: row(
            "用于用户主动选中的书面文字；不经过 ASR，不把内容当成转写，也不回答其中的问题。",
            "用於使用者主動選中的書面文字；不經過 ASR，不把內容當成轉寫，也不回答其中的問題。",
            "For user-selected written text; not ASR output. Do not treat it as a transcript or answer its questions.",
            "ユーザーが選択した書面テキスト用。ASRは経由せず、書き起こしとして扱わず、その中の質問にも答えません。",
            "사용자가 선택한 서면 텍스트용. ASR을 거치지 않으며, 받아쓰기로 취급하지 않고 그 안의 질문에도 답하지 않습니다.",
        ),
    },
    Msg {
        key: "style.pack.selectionPromptTitle",
        text: row(
            "选区润色 Prompt（无 ASR）",
            "選區潤色 Prompt（無 ASR）",
            "Selection polish prompt (no ASR)",
            "選択範囲の推敲プロンプト（ASRなし）",
            "선택 영역 다듬기 프롬프트(ASR 없음)",
        ),
    },
    Msg {
        key: "style.pack.voiceEditPromptTitle",
        text: row(
            "选区语音编辑 Prompt（EditPlan）",
            "選區語音編輯 Prompt（EditPlan）",
            "Selection voice edit prompt (EditPlan)",
            "選択範囲の音声編集プロンプト（EditPlan）",
            "선택 영역 음성 편집 프롬프트(EditPlan)",
        ),
    },
    Msg {
        key: "marketplace.myPacks.buttonTitleEmpty",
        text: row(
            "先在 Settings → 风格市场 填写发布身份",
            "先在 Settings → 風格市場 填寫發布身份",
            "Set publisher identity in Settings → Marketplace first",
            "先に 設定 → マーケット で公開者名を設定してください",
            "먼저 설정 → 마켓에서 게시자 이름을 입력하세요",
        ),
    },
    Msg {
        key: "marketplace.myPacks.notLoggedIn",
        text: row(
            "请先在 Settings → 风格市场 填写发布身份",
            "請先在 Settings → 風格市場 填寫發布身份",
            "Set publisher identity in Settings → Marketplace first",
            "先に 設定 → マーケット で公開者名を設定してください",
            "먼저 설정 → 마켓에서 게시자 이름을 입력하세요",
        ),
    },
    Msg {
        key: "style.pack.iconInvalid",
        text: row(
            "请选择不含外部资源的有效 SVG 图标（最大 256 KB）。",
            "請選擇不含外部資源的有效 SVG 圖示（最大 256 KB）。",
            "Choose a valid SVG icon with no external resources (up to 256 KB).",
            "外部リソースを含まない有効な SVG を選択してください（最大 256 KB）。",
            "외부 리소스가 없는 유효한 SVG 아이콘을 선택하세요(최대 256 KB).",
        ),
    },
    Msg {
        key: "style.pack.iconSaved",
        text: row("图标已保存", "圖示已儲存", "Icon saved", "アイコンを保存しました", "아이콘이 저장되었습니다"),
    },
    Msg {
        key: "style.pack.resetIcon",
        text: row(
            "恢复默认图标",
            "還原預設圖示",
            "Restore default icon",
            "既定のアイコンに戻す",
            "기본 아이콘 복원",
        ),
    },
    Msg {
        key: "style.pack.uploadIcon",
        text: row(
            "为「{}」上传 SVG 图标",
            "為「{}」上傳 SVG 圖示",
            "Upload an SVG icon for {}",
            "{} の SVG アイコンをアップロード",
            "{}의 SVG 아이콘 업로드",
        ),
    },
    Msg {
        key: "common.hide",
        text: row("隐藏", "隱藏", "Hide", "非表示", "숨기기"),
    },
    Msg {
        key: "quickNote.showShortcut",
        text: row(
            "显示速记快捷键",
            "顯示速記快捷鍵",
            "Show quick note shortcut",
            "速記ショートカットを表示",
            "속기 단축키 표시",
        ),
    },
    Msg {
        key: "style.pack.active",
        text: row("当前", "目前", "Active", "使用中", "사용 중"),
    },
    Msg {
        key: "style.pack.closeEditor",
        text: row("关闭", "關閉", "Close", "閉じる", "닫기"),
    },
    Msg {
        key: "style.pack.deleteImported",
        text: row("删除", "刪除", "Delete", "削除", "삭제"),
    },
    Msg {
        key: "style.pack.editorTitle",
        text: row("编辑风格", "編輯風格", "Edit Pack", "パック編集", "팩 편집"),
    },
    Msg {
        key: "style.pack.exportZip",
        text: row("导出 ZIP", "匯出 ZIP", "Export ZIP", "ZIP をエクスポート", "ZIP 내보내기"),
    },
    Msg {
        key: "style.pack.fieldAuthor",
        text: row("作者", "作者", "Author", "作者", "작성자"),
    },
    Msg {
        key: "style.pack.fieldAuthorPlaceholder",
        text: row(
            "可选，方便标注来源",
            "可選，方便標註來源",
            "Optional source label",
            "任意。ソース表示用",
            "선택. 출처 표시용",
        ),
    },
    Msg {
        key: "style.pack.fieldCompatibility",
        text: row(
            "兼容版本",
            "相容版本",
            "Compatible App Version",
            "互換アプリバージョン",
            "호환 앱 버전",
        ),
    },
    Msg {
        key: "style.pack.fieldCompatibilityPlaceholder",
        text: row(
            "可选，例如 >=1.3.0",
            "可選，例如 >=1.3.0",
            "Optional, e.g. >=1.3.0",
            "任意。例: >=1.3.0",
            "선택. 예: >=1.3.0",
        ),
    },
    Msg {
        key: "style.pack.fieldModel",
        text: row(
            "推荐模型（仅元数据）",
            "建議模型（僅元資料）",
            "Recommended Model (Metadata)",
            "推奨モデル（メタデータのみ）",
            "권장 모델(메타데이터)",
        ),
    },
    Msg {
        key: "style.pack.fieldModelHint",
        text: row(
            "仅作说明，不会切换实际模型。",
            "僅作說明，不會切換實際模型。",
            "Metadata only. Does not switch model.",
            "メタデータのみ。実際のモデルは切り替わりません。",
            "메타데이터일 뿐 실제 모델을 전환하지 않습니다.",
        ),
    },
    Msg {
        key: "style.pack.fieldModelPlaceholder",
        text: row(
            "可选，例如 gpt-4.1 / deepseek-v3",
            "可選，例如 gpt-4.1 / deepseek-v3",
            "Optional, e.g. gpt-4.1 / deepseek-v3",
            "任意。例: gpt-4.1 / deepseek-v3",
            "선택. 예: gpt-4.1 / deepseek-v3",
        ),
    },
    Msg {
        key: "style.pack.fieldVersion",
        text: row("版本", "版本", "Version", "バージョン", "버전"),
    },
    Msg {
        key: "style.pack.resetBuiltin",
        text: row("重置", "重設", "Reset", "リセット", "재설정"),
    },
    Msg {
        key: "style.pack.revert",
        text: row("撤销", "還原", "Revert", "元に戻す", "되돌리기"),
    },
    Msg {
        key: "style.pack.save",
        text: row("保存", "儲存", "Save", "保存", "저장"),
    },
    Msg {
        key: "style.pack.unsaved",
        text: row("未保存", "未儲存", "Unsaved", "未保存", "저장 안 됨"),
    },
    Msg {
        key: "style.pack.dictationPromptEditorDesc",
        text: row(
            "当前编辑录音 / ASR 风格 Prompt；输入对象是语音识别后的转写文本。",
            "目前編輯錄音 / ASR 風格 Prompt；輸入對象是語音辨識後的轉寫文本。",
            "Editing the recording / ASR style prompt; input is ASR transcript text after dictation.",
            "録音 / ASRスタイルのプロンプトを編集中。入力は音声認識後の書き起こしテキストです。",
            "녹음 / ASR 스타일 프롬프트를 편집 중입니다. 입력은 음성 인식 후 받아쓰기 텍스트입니다.",
        ),
    },
    Msg {
        key: "style.pack.selectionPromptEditorDesc",
        text: row(
            "当前编辑选区润色 Prompt；输入对象是用户主动选中的书面文字，不经过 ASR。",
            "目前編輯選區潤色 Prompt；輸入對象是使用者主動選中的書面文字，不經過 ASR。",
            "Editing the selection polish prompt; input is written text the user actively selected, without ASR.",
            "選択範囲の推敲プロンプトを編集中。入力はユーザーが選択した書面テキストで、ASRは経由しません。",
            "선택 영역 다듬기 프롬프트를 편집 중입니다. 입력은 사용자가 선택한 서면 텍스트이며 ASR을 거치지 않습니다.",
        ),
    },
    Msg {
        key: "style.pack.runtimeActive",
        text: row("当前生效", "目前生效", "Active", "有効", "활성"),
    },
    Msg {
        key: "style.pack.runtimeContextDesc",
        text: row(
            "来自语言与应用上下文",
            "來自語言與應用上下文",
            "From language and app context",
            "言語とアプリのコンテキストから",
            "언어·앱 컨텍스트에서",
        ),
    },
    Msg {
        key: "style.pack.runtimeContextEmpty",
        text: row(
            "当前不会附加",
            "目前不會附加",
            "Not added in the current preview.",
            "現在のプレビューでは付加されません。",
            "현재 미리보기에는 추가되지 않습니다.",
        ),
    },
    Msg {
        key: "style.pack.runtimeContextTitle",
        text: row(
            "上下文前提",
            "上下文前提",
            "Context premise",
            "コンテキスト前提",
            "컨텍스트 전제",
        ),
    },
    Msg {
        key: "style.pack.runtimeDesc",
        text: row(
            "只读的运行时辅助项。",
            "只讀的執行時輔助項。",
            "Read-only runtime helpers.",
            "読み取り専用の実行時ヘルパー。",
            "읽기 전용 런타임 보조.",
        ),
    },
    Msg {
        key: "style.pack.runtimeHistoryDesc",
        text: row(
            "仅用于实时多轮 polish",
            "僅用於即時多輪 polish",
            "Only for live multi-turn polish",
            "ライブのマルチターン polish のみで使用",
            "실시간 멀티턴 polish 전용",
        ),
    },
    Msg {
        key: "style.pack.runtimeHistoryEmpty",
        text: row(
            "只有存在 prior turns 时才会附加",
            "只有存在 prior turns 時才會附加",
            "Only added when prior turns exist.",
            "前のターンが存在する場合のみ付加。",
            "이전 턴이 있을 때만 추가됩니다.",
        ),
    },
    Msg {
        key: "style.pack.runtimeHistoryTitle",
        text: row(
            "多轮历史保护段",
            "多輪歷史保護段",
            "Multi-turn history guardrail",
            "マルチターン履歴ガード",
            "멀티턴 히스토리 가드",
        ),
    },
    Msg {
        key: "style.pack.runtimeHotwordDesc",
        text: row(
            "来自已启用热词",
            "來自已啟用熱詞",
            "From enabled hotwords",
            "有効なホットワードから",
            "활성화된 핫워드에서",
        ),
    },
    Msg {
        key: "style.pack.runtimeHotwordEmpty",
        text: row(
            "当前不会附加",
            "目前不會附加",
            "Not added in the current preview.",
            "現在のプレビューでは付加されません。",
            "현재 미리보기에는 추가되지 않습니다.",
        ),
    },
    Msg {
        key: "style.pack.runtimeHotwordTitle",
        text: row(
            "热词提示段",
            "熱詞提示段",
            "Hotword block",
            "ホットワードブロック",
            "핫워드 블록",
        ),
    },
    Msg {
        key: "style.pack.runtimeInactive",
        text: row("当前未生效", "目前未生效", "Inactive", "無効", "비활성"),
    },
    Msg {
        key: "style.pack.runtimePreviewOmittedFrontApp",
        text: row(
            "预览已省略前台 app 标签。",
            "預覽已省略前台 app 標籤。",
            "Preview omits the front-app label.",
            "プレビューはフロントアプリのラベルを省略しています。",
            "미리보기에서 프런트앱 라벨이 생략되었습니다.",
        ),
    },
    Msg {
        key: "style.pack.runtimeTitle",
        text: row(
            "OpenLess 运行时附加指令",
            "OpenLess 執行時附加指令",
            "OpenLess Runtime Directives",
            "OpenLess 実行時付加指令",
            "OpenLess 런타임 추가 지시",
        ),
    },
    Msg {
        key: "marketplace.installingBtn",
        text: row("安装中…", "安裝中…", "Installing…", "インストール中…", "설치 중…"),
    },
    Msg {
        key: "settings.about.update_dialog_available_desc",
        text: row(
            "发现 OpenLess {}，是否现在更新？",
            "發現 OpenLess {}，是否現在更新？",
            "OpenLess {} is available. Update now?",
            "OpenLess {} が見つかりました。今すぐ更新しますか？",
            "OpenLess {} 을(를) 발견했습니다. 지금 업데이트하시겠습니까?",
        ),
    },
    Msg {
        key: "settings.about.update_dialog_available_title",
        text: row(
            "发现新版本",
            "發現新版本",
            "Update available",
            "新しいバージョンがあります",
            "새 버전 발견",
        ),
    },
    Msg {
        key: "settings.about.update_dialog_downloaded_desc",
        text: row(
            "OpenLess {} 已安装完成。是否现在自动重启以应用更新？",
            "OpenLess {} 已安裝完成。是否現在自動重啓以應用更新？",
            "OpenLess {} has been installed. Restart automatically now to apply it?",
            "OpenLess {} のインストールが完了しました。今すぐ自動再起動して適用しますか？",
            "OpenLess {} 설치가 완료되었습니다. 지금 자동 재시작하여 적용하시겠습니까?",
        ),
    },
    Msg {
        key: "settings.about.update_dialog_downloaded_title",
        text: row("更新已准备好", "更新已準備好", "Update ready", "アップデートの準備完了", "업데이트 준비 완료"),
    },
    Msg {
        key: "settings.about.update_dialog_downloading_desc",
        text: row(
            "正在下载 OpenLess {}，请保持应用打开。",
            "正在下載 OpenLess {}，請保持應用打開。",
            "Downloading OpenLess {}. Keep the app open.",
            "OpenLess {} をダウンロード中です。アプリを開いたままにしてください。",
            "OpenLess {} 을(를) 다운로드 중입니다. 앱을 열어 두세요.",
        ),
    },
    Msg {
        key: "settings.about.update_dialog_downloading_title",
        text: row(
            "正在下载更新",
            "正在下載更新",
            "Downloading update",
            "アップデートをダウンロード中",
            "업데이트 다운로드 중",
        ),
    },
    Msg {
        key: "settings.about.update_dialog_install",
        text: row("现在更新", "現在更新", "Update now", "今すぐ更新", "지금 업데이트"),
    },
    Msg {
        key: "settings.about.update_dialog_install_error_desc",
        text: row(
            "自动更新没能完成：{}。你可以前往下载页手动下载安装最新版本。",
            "自動更新未能完成：{}。你可以前往下載頁手動下載安裝最新版本。",
            "The automatic update couldn't finish: {}. You can download and install the latest version manually.",
            "自動更新を完了できませんでした：{}。ダウンロードページから手動で最新版を入手できます。",
            "자동 업데이트를 완료하지 못했습니다: {}. 다운로드 페이지에서 최신 버전을 직접 받아 설치할 수 있습니다.",
        ),
    },
    Msg {
        key: "settings.about.update_dialog_install_error_title",
        text: row(
            "更新失败",
            "更新失敗",
            "Update failed",
            "更新に失敗しました",
            "업데이트 실패",
        ),
    },
    Msg {
        key: "settings.about.update_dialog_installing_desc",
        text: row(
            "正在安装 OpenLess {}，请保持应用打开。",
            "正在安裝 OpenLess {}，請保持應用打開。",
            "Installing OpenLess {}. Keep the app open.",
            "OpenLess {} をインストール中です。アプリを開いたままにしてください。",
            "OpenLess {} 을(를) 설치 중입니다. 앱을 열어 두세요.",
        ),
    },
    Msg {
        key: "settings.about.update_dialog_installing_title",
        text: row(
            "正在安装更新",
            "正在安裝更新",
            "Installing update",
            "アップデートをインストール中",
            "업데이트 설치 중",
        ),
    },
    Msg {
        key: "settings.about.update_dialog_later",
        text: row(
            "稍后手动重启",
            "稍後手動重啓",
            "Restart manually later",
            "後で手動再起動",
            "나중에 수동 재시작",
        ),
    },
    Msg {
        key: "settings.about.update_dialog_progress",
        text: row(
            "{}% · {} / {}",
            "{}% · {} / {}",
            "{}% · {} / {}",
            "{}% · {} / {}",
            "{}% · {} / {}",
        ),
    },
    Msg {
        key: "settings.about.update_dialog_progress_unknown",
        text: row(
            "已下载 {}",
            "已下載 {}",
            "{} downloaded",
            "ダウンロード済み {}",
            "다운로드됨 {}",
        ),
    },
    Msg {
        key: "settings.about.update_dialog_downloading_label",
        text: row("下载中…", "下載中…", "Downloading…", "ダウンロード中…", "다운로드 중…"),
    },
    Msg {
        key: "settings.about.update_dialog_installing_label",
        text: row("安装中…", "安裝中…", "Installing…", "インストール中…", "설치 중…"),
    },
];

fn lang_index(lang: Lang) -> usize {
    match lang {
        Lang::ZhCn => 0,
        Lang::ZhTw => 1,
        Lang::En => 2,
        Lang::Ja => 3,
        Lang::Ko => 4,
    }
}

fn find_entry<'a>(entries: &'a [Msg], key: &str) -> Option<&'a Msg> {
    entries.iter().find(|entry| entry.key == key)
}

/// Translate a catalog key into the chosen language, falling back to the
/// `zh-CN` source of truth for any key whose requested locale is untranslated,
/// and finally to the bare key when the key is not present at all.
pub fn tr<'a, L: IntoLang>(entries: &'a [Msg], lang: L, key: &'a str) -> &'a str {
    let lang = lang.into_lang();
    match find_entry(entries, key) {
        Some(entry) => {
            let value = entry.text[lang_index(lang)];
            if value.is_empty() && lang != Lang::ZhCn {
                entry.text[0]
            } else if value.is_empty() {
                // zh-CN is the source of truth and should never be empty.
                key
            } else {
                value
            }
        }
        None => key,
    }
}

/// Translate and substitute `{}` (sequential) and `{n}` (positional)
/// placeholders. Reuses the same fallback rules as [`tr`].
pub fn fmt<L: IntoLang>(entries: &[Msg], lang: L, key: &str, args: &[&dyn Display]) -> String {
    let template = tr(entries, lang, key);
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    let mut auto_index = 0usize;
    while let Some(open) = rest.find('{') {
        if let Some(relative_close) = rest[open + 1..].find('}') {
            let close = open + 1 + relative_close;
            let field = &rest[open + 1..close];
            let argument = if field.is_empty() {
                let index = auto_index;
                auto_index += 1;
                index
            } else if let Ok(index) = field.parse::<usize>() {
                index
            } else {
                // Not a placeholder we understand (e.g. `{name}`): keep it.
                out.push_str(&rest[..open]);
                out.push('{');
                out.push_str(field);
                out.push('}');
                rest = &rest[close + 1..];
                continue;
            };
            out.push_str(&rest[..open]);
            if let Some(value) = args.get(argument) {
                out.push_str(&value.to_string());
            }
            rest = &rest[close + 1..];
            continue;
        }
        // No closing brace: keep the `{` literally and make progress.
        out.push_str(&rest[..open]);
        out.push('{');
        rest = &rest[open + 1..];
    }
    out.push_str(rest);
    out
}

/// Global lookup against [`CATALOG`].
pub fn tr_catalog<L: IntoLang>(lang: L, key: &'static str) -> &'static str {
    tr(CATALOG, lang, key)
}

/// Global formatted lookup against [`CATALOG`].
pub fn fmt_catalog<L: IntoLang>(lang: L, key: &str, args: &[&dyn Display]) -> String {
    fmt(CATALOG, lang, key, args)
}

/// Ergonomic conversion for the call sites that already hold a concrete
/// [`Lang`] or a [`LocalePref`]. Kept tiny so UI code stays readable.
pub trait IntoLang {
    fn into_lang(self) -> Lang;
}

impl IntoLang for Lang {
    fn into_lang(self) -> Lang {
        self
    }
}

impl IntoLang for LocalePref {
    fn into_lang(self) -> Lang {
        self.resolve()
    }
}

impl IntoLang for &LocalePref {
    fn into_lang(self) -> Lang {
        self.resolve()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_CATALOG: &[Msg] = &[
        Msg {
            key: "k.full",
            text: row("完整", "完整", "Full", "完全", "전체"),
        },
        // `k.missing_en` deliberately leaves `en` empty to exercise fallback.
        Msg {
            key: "k.missing_en",
            text: row("来源", "來源", "", "ソース", "출처"),
        },
    ];

    #[test]
    fn fallback_uses_zh_cn_source_of_truth_when_locale_is_empty() {
        assert_eq!(tr(TEST_CATALOG, Lang::En, "k.missing_en"), "来源");
        assert_eq!(tr(TEST_CATALOG, Lang::ZhCn, "k.missing_en"), "来源");
        // Fully translated keys return their own locale.
        assert_eq!(tr(TEST_CATALOG, Lang::En, "k.full"), "Full");
    }

    #[test]
    fn unknown_key_returns_the_key_itself() {
        assert_eq!(tr(TEST_CATALOG, Lang::Ja, "k.unknown"), "k.unknown");
    }

    #[test]
    fn zh_cn_source_of_truth_is_fully_populated_and_complete() {
        for entry in CATALOG {
            assert!(
                !entry.text[0].is_empty(),
                "zh-CN must never be empty for key {}",
                entry.key
            );
        }
    }

    #[test]
    fn every_catalog_key_is_translated_in_all_supported_locales() {
        for entry in CATALOG {
            for lang in LANGS {
                assert!(
                    !entry.text[lang_index(lang)].is_empty(),
                    "{} is missing a translation for {}",
                    entry.key,
                    lang.tag()
                );
            }
        }
    }

    #[test]
    fn catalog_keys_are_unique() {
        for (i, left) in CATALOG.iter().enumerate() {
            for right in &CATALOG[i + 1..] {
                assert_ne!(left.key, right.key);
            }
        }
    }

    #[test]
    fn system_locale_detection_maps_common_locale_envs() {
        assert_eq!(Lang::parse("zh_CN.UTF-8"), Some(Lang::ZhCn));
        assert_eq!(Lang::parse("zh-Hant-TW"), Some(Lang::ZhTw));
        assert_eq!(Lang::parse("zh_TW"), Some(Lang::ZhTw));
        assert_eq!(Lang::parse("ja_JP.UTF-8"), Some(Lang::Ja));
        assert_eq!(Lang::parse("ko_KR"), Some(Lang::Ko));
        assert_eq!(Lang::parse("en_US"), Some(Lang::En));
        assert_eq!(Lang::parse("fr_FR"), None);
        assert_eq!(Lang::parse(""), None);
    }

    #[test]
    fn system_preference_resolves_from_the_host_locale() {
        for (variable, value, expected) in [
            ("LC_ALL", "zh_TW.UTF-8", Lang::ZhTw),
            ("LC_MESSAGES", "ko_KR.UTF-8", Lang::Ko),
            ("LANG", "ja_JP", Lang::Ja),
        ] {
            // LocalePref::System must route through env-based detection.
            let prev = std::env::var(variable).ok();
            std::env::set_var(variable, value);
            assert_eq!(LocalePref::System.resolve(), expected);
            match prev {
                Some(value) => std::env::set_var(variable, value),
                None => std::env::remove_var(variable),
            }
        }
    }

    #[test]
    fn locale_preference_roundtrips_through_wire_tags() {
        assert_eq!(LocalePref::System, LocalePref::from_tag("system"));
        assert_eq!(
            LocalePref::Lang(Lang::ZhTw),
            LocalePref::from_tag(Lang::ZhTw.tag())
        );
        // Unknown tags degrade to System (follow OS), never to a wrong guess.
        assert_eq!(LocalePref::from_tag("xx_YY"), LocalePref::System);
    }

    #[test]
    fn positional_placeholders_are_substituted_in_any_locale() {
        assert_eq!(
            fmt_catalog(Lang::ZhCn, "metric.near7", &[&3, &9]),
            "近7天 3 段 · 近30天 9 段"
        );
        assert_eq!(
            fmt_catalog(Lang::En, "metric.near7", &[&3, &9]),
            "3 in 7d · 9 in 30d"
        );
        assert_eq!(
            fmt_catalog(Lang::Ja, "metric.near7", &[&3, &9]),
            "直近7日 3 件 · 30日 9 件"
        );
        // Unknown keys fall back to the key text with no substitution.
        assert_eq!(fmt_catalog(Lang::En, "k.unknown", &[&1]), "k.unknown");
    }
}
