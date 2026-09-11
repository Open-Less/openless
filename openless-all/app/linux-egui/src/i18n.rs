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
        key: "nav.overview",
        text: row("概览", "概覽", "Overview", "概要", "개요"),
    },
    Msg {
        key: "nav.history",
        text: row("历史", "歷史", "History", "履歴", "기록"),
    },
    Msg {
        key: "nav.vocab",
        text: row(
            "词汇与纠错",
            "詞彙與糾錯",
            "Vocabulary & Correction",
            "語彙と修正",
            "어휘 및 교정",
        ),
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
            "Marketplace",
            "Marketplace",
            "Marketplace",
            "マーケットプレイス",
            "마켓플레이스",
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
            "시스템 따르기",
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
    Msg { key: "update.available", text: row("可用版本：{}", "可用版本：{}", "Available version: {}", "利用可能なバージョン: {}", "사용 가능한 버전: {}") },
    Msg { key: "update.downloaded", text: row("已下载 {} 字节", "已下載 {} 位元組", "{} bytes downloaded", "{} バイトをダウンロード", "{}바이트 다운로드됨") },
    Msg { key: "update.manual_notice", text: row("deb/rpm 与开发构建由包管理器或发布页更新。", "deb/rpm 與開發建置由套件管理員或發布頁更新。", "deb/rpm and dev builds update via your package manager or the releases page.", "deb/rpm と開発ビルドはパッケージマネージャまたはリリースページで更新されます。", "deb/rpm 및 개발 빌드는 패키지 관리자 또는 릴리스 페이지로 업데이트됩니다.") },
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
    Msg { key: "history.empty", text: row("暂无历史记录", "暫無歷史紀錄", "No history yet", "履歴はありません", "기록이 없습니다") },
    Msg { key: "history.inserted", text: row("已插入", "已插入", "Inserted", "挿入済み", "삽입됨") },
    Msg { key: "history.copied_fallback", text: row("已复制", "已複製", "Copied", "コピー済み", "복사됨") },
    Msg { key: "history.paste_sent", text: row("已发送粘贴", "已傳送貼上", "Paste sent", "貼り付け送信", "붙여넣기 전송됨") },
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
