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
    Msg { key: "settings.audioDsp.eqPresets", text: row("均衡器预设", "均衡器預設", "Equalizer presets", "イコライザープリセット", "이퀄라이저 프리셋") },
    Msg { key: "settings.audioDsp.eqNormal", text: row("平直", "平直", "Flat", "フラット", "플랫") },
    Msg { key: "settings.audioDsp.eqBright", text: row("明亮人声", "明亮人聲", "Bright vocal", "明るい声", "밝은 음성") },
    Msg { key: "settings.audioDsp.eqDeep", text: row("低沉人声", "低沉人聲", "Deep voice", "低い声", "낮은 음성") },
    Msg { key: "settings.audioDsp.eqPodcast", text: row("播客", "播客", "Podcast", "ポッドキャスト", "팟캐스트") },
    Msg { key: "settings.audioDsp.eqClarity", text: row("清晰人声", "清晰人聲", "Vocal clarity", "明瞭な声", "선명한 음성") },
    Msg { key: "settings.audioDsp.eqWarm", text: row("温暖人声", "溫暖人聲", "Warm vocal", "温かい声", "따뜻한 음성") },
    Msg { key: "settings.audioDsp.disabledHint", text: row("开启上方语音增强开关后可调整，当前配置已保留。", "開啟上方語音增強開關後可調整，目前設定已保留。", "Enable speech enhancement above to adjust these settings. Your configuration is retained.", "上の音声強調を有効にすると調整できます。設定は保持されています。", "위에서 음성 향상을 켜면 조정할 수 있습니다. 설정은 유지됩니다.") },
    Msg { key: "settings.audioDsp.moveUp", text: row("上移处理阶段", "上移處理階段", "Move stage up", "処理を上へ移動", "처리 단계 위로 이동") },
    Msg { key: "settings.audioDsp.moveDown", text: row("下移处理阶段", "下移處理階段", "Move stage down", "処理を下へ移動", "처리 단계 아래로 이동") },
    Msg {
        key: "settings.audioDsp.aecStage",
        text: row("回声消除", "回聲消除", "Echo cancellation", "エコーキャンセル", "에코 제거"),
    },
    Msg {
        key: "settings.audioDsp.echoStage",
        text: row("回声消除与缓冲", "回聲消除與緩衝", "Echo cancellation and buffering", "エコーキャンセルとバッファ", "에코 제거 및 버퍼"),
    },
    Msg {
        key: "settings.audioDsp.aecOff",
        text: row("关闭", "關閉", "Off", "オフ", "끄기"),
    },
    Msg {
        key: "settings.audioDsp.aecLight",
        text: row("轻量（省 CPU）", "輕量（省 CPU）", "Light (low CPU)", "軽量（低 CPU）", "경량(저 CPU)"),
    },
    Msg {
        key: "settings.audioDsp.aecModel",
        text: row("模型（更强）", "模型（更強）", "Model (stronger)", "モデル（高精度）", "모델(강화)"),
    },
    Msg {
        key: "settings.audioDsp.aecModelCost",
        text: row(
            "模型版抑制更强，但约是轻量版的 3 倍 CPU；老机器建议用轻量。",
            "模型版抑制更強，但約是輕量版的 3 倍 CPU；舊機器建議用輕量。",
            "The model cancels more echo but costs about 3× the CPU; prefer Light on older machines.",
            "モデル版はより強く抑圧しますが CPU は約 3 倍です。古い端末では軽量版を推奨します。",
            "모델 버전은 더 강하게 제거하지만 CPU를 약 3배 사용합니다. 오래된 기기에서는 경량을 권장합니다.",
        ),
    },
    Msg {
        key: "settings.audioDsp.aecReferenceLabel",
        text: row("扬声器参考源", "揚聲器參考源", "Speaker reference", "スピーカー参照", "스피커 참조"),
    },
    Msg {
        key: "settings.audioDsp.aecReferenceDesc",
        text: row(
            "采集哪台扬声器的回环信号供回声消除使用；没有回环源时 AEC 只能在没有参考的情况下工作。",
            "擷取哪台揚聲器的回環訊號供回聲消除使用；沒有回環源時 AEC 只能在沒有參考的情況下運作。",
            "Which speaker loopback feeds echo cancellation; without a loopback source AEC runs without a reference.",
            "どのスピーカーのループバックをエコーキャンセルに使うか。ループバック元がない場合、AEC は参照なしで動作します。",
            "어떤 스피커의 루프백을 에코 제거에 사용할지 선택합니다. 루프백 소스가 없으면 AEC는 참조 없이 동작합니다.",
        ),
    },
    Msg {
        key: "settings.audioDsp.aecReferenceAuto",
        text: row("自动（默认扬声器）", "自動（預設揚聲器）", "Automatic (default speaker)", "自動（既定のスピーカー）", "자동(기본 스피커)"),
    },
    Msg {
        key: "settings.audioDsp.aecReferenceOff",
        text: row("不采集参考", "不擷取參考", "Do not capture a reference", "参照を取得しない", "참조를 수집하지 않음"),
    },
    Msg {
        key: "settings.audioDsp.acousticEchoCancellation",
        text: row("声学回声消除（AEC）", "聲學回聲消除（AEC）", "Acoustic echo cancellation (AEC)", "音響エコーキャンセル（AEC）", "음향 에코 제거(AEC)"),
    },
    Msg {
        key: "settings.audioDsp.AEC_needs_a_loudspeaker_reference",
        text: row(
            "外放时压掉扬声器回授；没有参考信号时不会起作用，戴耳机可以关掉。",
            "外放時壓掉揚聲器回授；沒有參考訊號時不會起作用，戴耳機可以關掉。",
            "Suppresses loudspeaker feedback while on speaker; it does nothing without a reference signal, so leave it off with headphones.",
            "スピーカー使用時の回り込みを抑えます。参照信号がないと効果はないため、ヘッドホンならオフのままで構いません。",
            "스피커 사용 시 되먹임을 줄입니다. 참조 신호가 없으면 효과가 없으므로 헤드폰이면 꺼 두세요.",
        ),
    },
    Msg { key: "settings.audioDsp.noiseStage", text: row("降噪", "降噪", "Noise reduction", "ノイズ低減", "잡음 감소") },
    Msg { key: "settings.audioDsp.roomStage", text: row("音色与房间", "音色與房間", "Tone and room", "音色と残響", "음색과 잔향") },
    Msg { key: "settings.audioDsp.combStage", text: row("梳状滤波（简易去混响）", "梳狀濾波（簡易去混響）", "Comb filter (simple dereverb)", "コムフィルター（簡易デリバーブ）", "콤 필터(간이 디버브)") },
    Msg { key: "settings.audioDsp.Speech_enhancement_settings", text: row("语音增强设置", "語音增強設置", "Speech enhancement settings", "音声強調設定", "음성 향상 설정") },
    Msg { key: "settings.audioDsp.RNNoise_is_lightweight_PureVox_uses_more_CPU_Changes_apply_to_the_next_recording", text: row("RNNoise 更轻量，PureVox 模型降噪耗时较高。设置在下一次录音生效。", "RNNoise 更輕量，PureVox 模型降噪耗時較高。設置在下一次錄音生效。", "RNNoise is lightweight; PureVox uses more CPU. Changes apply to the next recording.", "RNNoise は軽量、PureVox は CPU 使用量が多めです。設定は次の録音に適用されます。", "RNNoise는 가볍고 PureVox는 CPU를 더 사용합니다. 설정은 다음 녹음에 적용됩니다.") },
    Msg { key: "settings.audioDsp.Noise_reduction_off", text: row("关闭降噪", "關闭降噪", "Noise reduction off", "ノイズ低減オフ", "잡음 감소 끄기") },
    Msg { key: "settings.audioDsp.Spectral_subtraction", text: row("频谱减法", "頻譜減法", "Spectral subtraction", "スペクトル減算法", "스펙트럼 차감") },
    Msg { key: "settings.audioDsp.Off", text: row("关闭降噪", "關闭降噪", "Off", "オフ", "끄기") },
    Msg { key: "settings.audioDsp.Noise_reduction", text: row("降噪强度 %", "降噪強度 %", "Noise reduction %", "ノイズ低減 %", "잡음 감소 %") },
    Msg { key: "settings.audioDsp.Input_gain_dB", text: row("输入增益 dB", "輸入增益 dB", "Input gain dB", "入力ゲイン dB", "입력 게인 dB") },
    Msg { key: "settings.audioDsp.Automatic_gain", text: row("自动增益", "自動增益", "Automatic gain", "自動ゲイン", "자동 게인") },
    Msg { key: "settings.audioDsp.Target_level", text: row("目标电平 %", "目標電平 %", "Target level %", "目標レベル %", "목표 레벨 %") },
    Msg { key: "settings.audioDsp.Attack_speed", text: row("启动速度", "啟動速度", "Attack speed", "アタック速度", "어택 속도") },
    Msg { key: "settings.audioDsp.Release_speed", text: row("释放速度", "釋放速度", "Release speed", "リリース速度", "릴리스 속도") },
    Msg { key: "settings.audioDsp.Speech_gate_does_not_stop_recording", text: row("低于门限的片段被衰减，录音不会因此停止。", "低於門檻的片段被衰減，錄音不會因此停止。", "Passages below the threshold are attenuated; recording never stops for them.", "しきい値を下回る区間は減衰されますが、録音は停止しません。", "임계값 미만 구간은 감쇠되지만 녹음은 중단되지 않습니다.") },
    Msg { key: "settings.audioDsp.Threshold_dB", text: row("门限 dB", "門限 dB", "Threshold dB", "しきい値 dB", "임계값 dB") },
    Msg { key: "settings.audioDsp.A_high_threshold_can_remove_quiet_speech", text: row("门限过高可能丢失轻声。", "門限過高可能丟失輕聲。", "A high threshold can remove quiet speech.", "しきい値が高いと小さな声が失われる場合があります。", "임계값이 높으면 작은 목소리가 사라질 수 있습니다.") },
    Msg { key: "settings.audioDsp.Tone_and_room_adjustments", text: row("音色与房间调整", "音色與房間調整", "Tone and room adjustments", "音色と室内音響の調整", "음색 및 실내 음향 조정") },
    Msg { key: "settings.audioDsp.The_MicYou_comb_filter_can_color_speech_Start_at_0", text: row("把 10 ms 前的信号按比例减掉，能压掉一点早期反射，但会给音色染上金属感；从 0 开始试。真正的去混响需要频谱或模型方法，这里没做。", "把 10 ms 前的訊號按比例減掉，能壓掉一點早期反射，但會讓音色帶金屬感；從 0 開始試。真正的去混響需要頻譜或模型方法，這裡沒做。", "Subtracts the signal from 10 ms ago, trimming some early reflections at the cost of a metallic color; start at 0. Real dereverberation needs spectral or model-based methods, which this is not.", "10 ms 前の信号を差し引いて初期反射を少し抑えますが、金属的な音色になります。0 から試してください。本格的なデリバーブは周波数領域かモデル方式が必要で、ここでは未実装です。", "10 ms 전 신호를 비율로 빼서 초기 반사를 조금 줄이지만 금속성 음색이 생깁니다. 0부터 시작하세요. 실제 디버브는 주파수 영역이나 모델 방식이 필요하며 여기서는 구현되지 않았습니다.") },
    Msg { key: "settings.audioDsp.Comb_filter", text: row("梳状滤波 %", "梳狀濾波 %", "Comb filter %", "コムフィルター %", "콤 필터 %") },
    Msg { key: "settings.audioDsp.10_band_equalizer", text: row("十段均衡器", "十段均衡器", "10-band equalizer", "10 バンドイコライザー", "10밴드 이퀄라이저") },
    Msg { key: "settings.audioDsp.Preamp_dB", text: row("前置增益 dB", "前置增益 dB", "Preamp dB", "プリアンプ dB", "프리앰프 dB") },
    Msg { key: "settings.audioDsp.Processing_order", text: row("处理顺序", "處理順序", "Processing order", "処理順序", "처리 순서") },
    Msg { key: "settings.audioDsp.Equalizer", text: row("均衡器", "均衡器", "Equalizer", "イコライザー", "이퀄라이저") },
    Msg { key: "settings.audioDsp.Input_gain", text: row("输入增益", "輸入增益", "Input gain", "入力ゲイン", "입력 게인") },
    Msg { key: "settings.audioDsp.Speech_gate", text: row("噪声门限（不是人声检测）", "雜訊門檻（不是人聲偵測）", "Noise gate (not speech detection)", "ノイズゲート（音声検出ではありません）", "노이즈 게이트(음성 검출 아님)") },
    Msg { key: "settings.audioDsp.Reset_enhancement_settings", text: row("恢复增强默认设置", "恢復增強默認設置", "Reset enhancement settings", "強調設定を初期化", "향상 설정 초기화") },
    Msg { key: "settings.providers.presets.chatgptSiwc", text: row("ChatGPT Plan", "ChatGPT Plan", "ChatGPT Plan", "ChatGPT Plan", "ChatGPT Plan") },
    Msg { key: "siwc.description", text: row("符合条件的文本润色和翻译使用你的 ChatGPT 计划或积分余额。ASR 仍使用原渠道；额度不足时不会自动切换到 API Key。", "符合條件的文字潤飾和翻譯使用你的 ChatGPT 方案或點數。ASR 仍使用原管道；額度不足時不會自動切換至 API Key。", "Eligible text polishing and translation use your ChatGPT plan or credits. ASR keeps its existing provider. Usage limits never switch you to an API key automatically.", "対象の文章校正と翻訳には ChatGPT プランまたはクレジットを使用します。音声認識は従来のプロバイダーを使い、上限到達時に API キーへ自動切り替えしません。", "지원되는 텍스트 다듬기와 번역에 ChatGPT 요금제 또는 크레딧을 사용합니다. 음성 인식은 기존 서비스를 사용하며 한도에 도달해도 API 키로 자동 전환하지 않습니다.") },
    Msg { key: "siwc.account", text: row("ChatGPT 账号", "ChatGPT 帳號", "ChatGPT account", "ChatGPT アカウント", "ChatGPT 계정") },
    Msg { key: "siwc.cancel", text: row("取消登录", "取消登入", "Cancel sign-in", "ログインをキャンセル", "로그인 취소") },
    Msg { key: "siwc.enableUsage", text: row("授权计划用量", "授權方案用量", "Enable plan usage", "プラン利用を許可", "요금제 사용 허용") },
    Msg { key: "siwc.reauthorize", text: row("重新授权", "重新授權", "Sign in again", "再ログイン", "다시 로그인") },
    Msg { key: "siwc.addAccount", text: row("添加账号或工作区", "新增帳號或工作區", "Add account or workspace", "アカウント・ワークスペースを追加", "계정 또는 워크스페이스 추가") },
    Msg { key: "siwc.disconnect", text: row("断开连接", "中斷連線", "Disconnect", "接続を解除", "연결 해제") },
    Msg { key: "siwc.manageUsage", text: row("管理用量", "管理用量", "Manage usage", "使用量を管理", "사용량 관리") },
    Msg { key: "siwc.retry", text: row("重试计划用量", "重試方案用量", "Retry plan usage", "プラン利用を再試行", "요금제 사용 재시도") },
    Msg { key: "siwc.model", text: row("润色模型", "潤飾模型", "Polishing model", "校正モデル", "다듬기 모델") },
    Msg { key: "siwc.selectModel", text: row("选择账号可用模型", "選擇帳號可用模型", "Select an available model", "利用可能なモデルを選択", "사용 가능한 모델 선택") },
    Msg { key: "siwc.modelUnavailable", text: row("模型 {0} 当前不可用，请重新选择。", "模型 {0} 目前無法使用，請重新選擇。", "{0} is no longer available. Select another model.", "{0} は現在利用できません。モデルを選び直してください。", "{0} 모델을 사용할 수 없습니다. 다른 모델을 선택하세요.") },
    Msg { key: "siwc.noModels", text: row("该账号暂时没有可显示的模型。", "此帳號目前沒有可顯示的模型。", "No displayable models are available for this account.", "このアカウントには表示可能なモデルがありません。", "이 계정에서 표시할 수 있는 모델이 없습니다.") },
    Msg { key: "siwc.refreshModels", text: row("刷新模型", "重新整理模型", "Refresh models", "モデルを更新", "모델 새로고침") },
    Msg { key: "siwc.test", text: row("测试连接", "測試連線", "Test connection", "接続をテスト", "연결 테스트") },
    Msg { key: "siwc.testHint", text: row("获取模型列表不执行推理；测试连接会使用少量 ChatGPT 计划用量。", "取得模型清單不會執行推理；測試連線會使用少量 ChatGPT 方案用量。", "Listing models does not run inference. Testing uses a small amount of your ChatGPT plan.", "モデル一覧の取得では推論を実行しません。接続テストはプランの使用量を少量消費します。", "모델 목록 조회는 추론을 실행하지 않습니다. 연결 테스트는 요금제 사용량을 소량 소모합니다.") },
    Msg { key: "siwc.testSuccess", text: row("连接测试成功", "連線測試成功", "Connection test succeeded", "接続テストに成功しました", "연결 테스트 성공") },
    Msg { key: "siwc.remoteUnconfirmed", text: row("已在本机断开连接，但未确认远端撤销。请到 ChatGPT 设置中断开此应用。", "已在本機登出，但未確認遠端撤銷。請至 ChatGPT 設定中中斷此應用程式。", "Signed out locally, but remote revocation was not confirmed. Disconnect this app in ChatGPT settings.", "端末ではログアウトしましたが、サーバー側の失効を確認できません。ChatGPT 設定でこのアプリの接続を解除してください。", "이 기기에서는 로그아웃했지만 원격 취소는 확인되지 않았습니다. ChatGPT 설정에서 이 앱의 연결을 해제하세요.") },
    Msg { key: "siwc.welcome", text: row("正在使用你的 ChatGPT 计划", "正在使用你的 ChatGPT 方案", "You’re using your ChatGPT plan", "ChatGPT プランを使用しています", "ChatGPT 요금제를 사용하고 있습니다") },
    Msg { key: "siwc.gotIt", text: row("知道了", "知道了", "Got it", "了解", "확인") },
    Msg { key: "siwc.failed", text: row("请求未完成，请检查连接或重新授权。", "請求未完成，請檢查連線或重新授權。", "The request did not complete. Check your connection or sign in again.", "リクエストを完了できませんでした。接続を確認するか再ログインしてください。", "요청이 완료되지 않았습니다. 연결을 확인하거나 다시 로그인하세요.") },
    Msg { key: "siwc.states.signed_out", text: row("尚未连接 ChatGPT", "尚未連接 ChatGPT", "Not connected to ChatGPT", "ChatGPT に未接続", "ChatGPT에 연결되지 않음") },
    Msg { key: "siwc.states.authorizing", text: row("请在系统浏览器中完成授权", "請在系統瀏覽器完成授權", "Complete authorization in your system browser", "システムブラウザーで認証を完了してください", "시스템 브라우저에서 인증을 완료하세요") },
    Msg { key: "siwc.states.connected", text: row("正在使用 ChatGPT 计划", "正在使用 ChatGPT 方案", "Using ChatGPT plan", "ChatGPT プランを使用中", "ChatGPT 요금제 사용 중") },
    Msg { key: "siwc.states.usage_disabled", text: row("已登录，尚未授权计划用量", "已登入，尚未授權方案用量", "Signed in; plan usage is not enabled", "ログイン済み・プラン利用は未許可", "로그인됨 · 요금제 사용 허용 필요") },
    Msg { key: "siwc.states.usage_limited", text: row("已达到计划或此应用的用量限制", "已達方案或此應用程式的用量限制", "Plan or app usage limit reached", "プランまたはアプリの利用上限に達しました", "요금제 또는 앱의 사용 한도에 도달함") },
    Msg { key: "siwc.states.reauthorization_required", text: row("需要重新登录 ChatGPT", "需要重新登入 ChatGPT", "Sign in to ChatGPT again", "ChatGPT に再ログインしてください", "ChatGPT에 다시 로그인하세요") },
    Msg {
        key: "providers.extra_headers_hint",
        text: row(
            "不读取已保存的请求头。留空保留；清除密钥时一并移除。",
            "不讀取已儲存的請求標頭。留空保留；清除密鑰時一併移除。",
            "Saved headers are never read back. Blank keeps them; Clear Secret removes them.",
            "保存済みヘッダーは読み戻しません。空欄なら保持し、秘密情報の削除時に一緒に削除します。",
            "저장된 헤더는 다시 읽지 않습니다. 비워 두면 유지되며 비밀 정보 삭제 시 함께 제거됩니다.",
        ),
    },
    Msg {
        key: "selection.source_truncated",
        text: row(
            "选区包含截断占位，未替换，以免删除未读取的原文。",
            "選取範圍含有截斷佔位，未替換，以免刪除未讀取的原文。",
            "The selection contains a truncated stand-in. Nothing was replaced, to protect the omitted text.",
            "選択範囲に省略されたテキストが含まれています。未取得の原文を保護するため、置換しませんでした。",
            "선택 영역에 잘린 대체 텍스트가 포함되어 있습니다. 생략된 원문을 보호하기 위해 교체하지 않았습니다.",
        ),
    },
    Msg {
        key: "settings.recording.capsuleTranscriptLabel",
        text: row(
            "显示流式原文",
            "顯示串流原文",
            "Show live transcript",
            "文字起こしをリアルタイム表示",
            "실시간 원문 표시",
        ),
    },
    Msg {
        key: "settings.recording.capsuleTranscriptDesc",
        text: row(
            "在胶囊中显示语音识别原文，替换音频条；关闭后恢复原有显示。非流式识别将在结果返回后显示。",
            "在膠囊中顯示語音辨識原文，取代音訊條；關閉後恢復原有顯示。非串流辨識將於結果傳回後顯示。",
            "Show recognized speech in the capsule instead of audio bars. Turn off to restore the original display. Non-streaming recognition appears when results arrive.",
            "音声バーの代わりに認識した原文を表示します。オフにすると元の表示に戻ります。非ストリーミング認識は結果の受信後に表示します。",
            "오디오 막대 대신 인식된 원문을 표시합니다. 끄면 기존 표시로 돌아갑니다. 비스트리밍 인식은 결과 수신 후 표시됩니다.",
        ),
    },
    Msg {
        key: "settings.recording.capsuleTranscriptFontSize",
        text: row(
            "原文字号",
            "原文字級",
            "Transcript font size",
            "文字起こしの文字サイズ",
            "원문 글자 크기",
        ),
    },
    Msg {
        key: "settings.providers.requestFormatLabel",
        text: row(
            "请求格式",
            "請求格式",
            "Request format",
            "リクエスト形式",
            "요청 형식",
        ),
    },
    Msg {
        key: "settings.providers.responsesThinkingHint",
        text: row(
            "部分模型只能降低思考，不能完全关闭。推理请求不发送温度参数。",
            "部分模型只能降低思考，無法完全關閉。推理請求不傳送溫度引數。",
            "Some models can only reduce thinking, not turn it off. Reasoning requests omit temperature.",
            "一部のモデルでは思考を軽減できますが、完全には無効にできません。推論リクエストでは温度を送信しません。",
            "일부 모델은 사고를 줄일 수만 있으며 완전히 끌 수 없습니다. 추론 요청에는 온도를 보내지 않습니다.",
        ),
    },
    Msg {
        key: "settings.providers.serviceTierLabel",
        text: row(
            "方舟低延迟推理（fast）",
            "方舟低延遲推理（fast）",
            "Ark low-latency inference (fast)",
            "Ark 低レイテンシ推論（fast）",
            "Ark 저지연 추론(fast)",
        ),
    },
    Msg {
        key: "settings.providers.serviceTierHint",
        text: row(
            "仅适用于支持低延迟的方舟模型；不支持的模型可能返回 400。",
            "僅適用於支援低延遲的方舟模型；不支援的模型可能回傳 400。",
            "Only supported Ark models can use this option; unsupported models may return 400.",
            "対応する Ark モデルでのみ利用できます。非対応モデルでは 400 が返る場合があります。",
            "지원되는 Ark 모델에서만 사용할 수 있으며, 지원되지 않는 모델은 400을 반환할 수 있습니다.",
        ),
    },
    Msg {
        key: "settings.providers.messagesThinkingLabel",
        text: row(
            "思考方式",
            "思考方式",
            "Thinking mode",
            "思考方式",
            "사고 방식",
        ),
    },
    Msg {
        key: "settings.providers.thinkingAdaptive",
        text: row("自适应", "自適應", "Adaptive", "適応型", "적응형"),
    },
    Msg {
        key: "settings.providers.thinkingBudget",
        text: row("固定预算", "固定預算", "Fixed budget", "固定予算", "고정 예산"),
    },
    Msg {
        key: "settings.providers.maxTokensLabel",
        text: row(
            "最大输出 tokens",
            "最大輸出 tokens",
            "Maximum output tokens",
            "最大出力トークン数",
            "최대 출력 토큰",
        ),
    },
    Msg {
        key: "settings.providers.thinkingBudgetLabel",
        text: row(
            "思考预算 tokens",
            "思考預算 tokens",
            "Thinking budget tokens",
            "思考トークン予算",
            "사고 토큰 예산",
        ),
    },
    Msg {
        key: "settings.providers.messagesThinkingHint",
        text: row(
            "旧模型或兼容网关可能需要固定预算；思考预算必须小于最大输出。开启思考时不发送温度参数。",
            "舊模型或相容閘道可能需要固定預算；思考預算必須小於最大輸出。開啟思考時不傳送溫度引數。",
            "Older models or compatible gateways may need a fixed budget below the output limit. Thinking requests omit temperature.",
            "旧モデルや互換ゲートウェイでは固定予算が必要な場合があります。予算は最大出力未満にしてください。思考時は温度を送信しません。",
            "이전 모델이나 호환 게이트웨이는 고정 예산이 필요할 수 있습니다. 예산은 출력 한도보다 작아야 합니다. 사고 요청에는 온도를 보내지 않습니다.",
        ),
    },
    Msg {
        key: "settings.providers.llmServiceTierInvalid",
        text: row(
            "低延迟推理选项无效，请重新选择。",
            "低延遲推理選項無效，請重新選擇。",
            "Invalid low-latency inference option. Select it again.",
            "低レイテンシ推論の設定が無効です。もう一度選択してください。",
            "저지연 추론 옵션이 잘못되었습니다. 다시 선택하세요.",
        ),
    },
    Msg {
        key: "settings.providers.llmRequestFormatInvalid",
        text: row(
            "请求格式无效，请重新选择。",
            "請求格式無效，請重新選擇。",
            "Invalid request format. Select a supported format.",
            "リクエスト形式が無効です。選択し直してください。",
            "잘못된 요청 형식입니다. 다시 선택하세요.",
        ),
    },
    Msg {
        key: "settings.providers.llmThinkingModeInvalid",
        text: row(
            "思考方式无效，请重新选择。",
            "思考方式無效，請重新選擇。",
            "Invalid thinking mode. Select a supported mode.",
            "思考方式が無効です。選択し直してください。",
            "잘못된 사고 방식입니다. 다시 선택하세요.",
        ),
    },
    Msg {
        key: "settings.providers.llmTokenLimitInvalid",
        text: row(
            "Token 上限必须为正整数。",
            "Token 上限必須為正整數。",
            "Token limits must be positive integers.",
            "トークン上限は正の整数にしてください。",
            "토큰 한도는 양의 정수여야 합니다.",
        ),
    },
    Msg {
        key: "settings.providers.llmThinkingBudgetInvalid",
        text: row(
            "思考预算至少为 1024，且固定预算必须小于最大输出。",
            "思考預算至少為 1024，且固定預算必須小於最大輸出。",
            "Thinking budget must be at least 1024 and below the output limit in fixed-budget mode.",
            "思考予算は1024以上、固定予算では最大出力未満にしてください。",
            "사고 예산은 1024 이상이며 고정 예산 모드에서는 출력 한도보다 작아야 합니다.",
        ),
    },
    // ---- Shell / navigation ------------------------------------------------

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
        text: row("词典", "詞彙表", "Dictionary", "辞書", "사전"),
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
        text: row("问任何问题", "問任何問題", "Ask", "何でも質問", "무엇이든 질문"),
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
        key: "btn.close",
        text: row("关闭", "關閉", "Close", "閉じる", "닫기"),
    },

    // ---- Dictation / empty states ------------------------------------------


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

    // ---- Overview metrics ---------------------------------------------------

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
        text: row("暂无数据", "暫無資料", "No data", "データなし", "데이터 없음"),
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
            "ASR 語音",
            "Speech recognition",
            "音声認識",
            "음성 인식",
        ),
    },
    Msg {
        key: "overview.llm_kind",
        text: row(
            "文字处理",
            "LLM 模型",
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
        text: row("刷新", "重新整理", "Refresh", "更新", "새로고침"),
    },
    Msg {
        key: "common.clear",
        text: row("清空", "清空", "Clear", "クリア", "지우기"),
    },
            Msg {
        key: "common.loading",
        text: row("加载中…", "載入中…", "Loading…", "読み込み中…", "로딩 중…"),
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
        key: "status.copied",
        text: row("已复制", "已複製", "Copied", "コピー済み", "복사됨"),
    },
    // egui-only: the style page's pack counter and new-pack hint are not part of
    // the Tauri catalog (it renders the equivalent UI from Core data).

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
        text: row("历史记录", "HISTORY", "HISTORY", "履歴", "기록"),
    },
    Msg {
        key: "history.title",
        text: row("历史记录", "歷史記錄", "History", "履歴", "기록"),
    },
            Msg {
        key: "history.desc",
        text: row(
            "本机保存的识别记录。",
            "本機儲存的識別記錄。",
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
            "載入歷史失敗：{}",
            "Failed to load history: {}",
            "履歴の読み込みに失敗：{}",
            "기록 로드 실패: {}",
        ),
    },
            Msg {
        key: "history.select_hint",
        text: row(
            "左侧选一条查看详情。",
            "左側選一條檢視詳情。",
            "Select an entry on the left to see details.",
            "左側から 1 件選択して詳細を表示。",
            "왼쪽에서 하나를 선택하여 자세히 보기.",
        ),
    },
    Msg {
        key: "history.recorded",
        text: row("录音 {}", "錄音 {}", "Recorded {}", "録音 {}", "녹음 {}"),
    },
    // egui-only in-app player controls; Tauri opens the recording externally.
    Msg {
        key: "history.pause",
        text: row("暂停播放", "暫停播放", "Pause recording", "再生を一時停止", "재생 일시 정지"),
    },
    Msg {
        key: "history.resume",
        text: row("继续播放", "繼續播放", "Resume recording", "再生を再開", "재생 계속"),
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
        text: row("已尝试粘贴", "已嘗試貼上", "Paste sent", "貼り付けを試行", "붙여넣기 시도됨"),
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
            "確定清空全部 {} 條記錄？此操作不可還原。",
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

    // ---- Language selector --------------------------------------------------

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

    // ---- Appearance (settings) ---------------------------------------------

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
    Msg {
        key: "startup.input_method_unavailable_title",
        text: row(
            "输入法服务不可用",
            "輸入法服務無法使用",
            "Input method unavailable",
            "入力メソッドを利用できません",
            "입력기 서비스를 사용할 수 없음",
        ),
    },
    Msg {
        key: "startup.input_method_unavailable",
        text: row(
            "输入法服务当前不可用。应用仍会继续运行，但全局快捷键和划词功能暂不可用。请安装并启动 fcitx5、启用 OpenLess 插件，并确认当前桌面会话的 D-Bus 可用。详情：{}",
            "輸入法服務目前無法使用。應用程式仍會繼續執行，但全域快捷鍵和選取文字功能暫時無法使用。請安裝並啟動 fcitx5、啟用 OpenLess 插件，並確認目前桌面工作階段的 D-Bus 可用。詳情：{}",
            "The input method service is unavailable. The app will continue running, but global hotkeys and selection tools are temporarily unavailable. Install and start fcitx5, enable the OpenLess addon, and check the desktop session's D-Bus. Details: {}",
            "入力メソッドサービスを利用できません。アプリは起動を続けますが、グローバルショートカットと選択ツールは一時的に利用できません。fcitx5 と OpenLess アドオンを有効にし、デスクトップセッションの D-Bus を確認してください。詳細：{}",
            "입력기 서비스를 사용할 수 없습니다. 앱은 계속 실행되지만 전역 단축키와 선택 도구를 일시적으로 사용할 수 없습니다. fcitx5와 OpenLess 애드온을 설치·시작하고 데스크톱 세션의 D-Bus를 확인하세요. 세부 정보: {}",
        ),
    },
    Msg {
        key: "common.load_more",
        text: row("加载更多", "載入更多", "Load more", "もっと読み込む", "더 불러오기"),
    },
    Msg {
        key: "common.loaded_of_total",
        text: row(
            "已载入 {} / 共 {}",
            "已載入 {} / 共 {}",
            "Loaded {} of {}",
            "{} / {} 件を読み込み済み",
            "{} / {}개 불러옴",
        ),
    },
    Msg {
        key: "common.saving",
        text: row("保存中", "儲存中", "Saving…", "保存中", "저장 중"),
    },
    Msg { key: "startup.connecting", text: row("正在连接 OpenLess…", "正在連線 OpenLess…", "Connecting to OpenLess…", "OpenLess に接続中…", "OpenLess에 연결 중…") },
    // ---- Buttons (models / providers / vocab / styles / marketplace / history)

    Msg { key: "btn.add", text: row("添加", "新增", "Add", "追加", "추가") },

    Msg { key: "btn.edit", text: row("编辑", "編輯", "Edit", "編集", "편집") },

    Msg { key: "btn.save_fields", text: row("保存字段/Secret", "儲存欄位/Secret", "Save fields/Secret", "欄位/Secret を保存", "필드/Secret 저장") },
    Msg { key: "btn.clear_secret", text: row("清除 Secret", "清除 Secret", "Clear Secret", "Secret を消去", "Secret 지우기") },

    Msg { key: "btn.export_error_log", text: row("导出错误日志", "匯出錯誤日誌", "Export error log", "エラーログをエクスポート", "오류 로그 내보내기") },

    Msg { key: "btn.activate", text: row("激活", "啟用", "Activate", "アクティブ化", "활성화") },

    // ---- Page / section headings

    // ---- Update UI
    // ---- Settings / preferences

    Msg { key: "settings.microphone", text: row("麦克风", "麥克風", "Microphone", "マイク", "마이크") },
    Msg { key: "settings.system_default", text: row("系统默认", "系統預設", "System default", "システム既定", "시스템 기본") },

    // ---- Hotkey control labels

    Msg { key: "hotkey.translation", text: row("翻译修饰键", "翻譯修飾鍵", "Translate modifier", "翻訳修飾キー", "번역 수정자") },

    // ---- Remote input

    Msg { key: "lbl.new_style_default", text: row("新风格", "新風格", "New style", "新規スタイル", "새 스타일") },

    Msg { key: "providers.credentials", text: row("凭据渠道", "憑證管道", "Credential channels", "資格情報チャネル", "자격 증명 채널") },
    Msg { key: "providers.core_note", text: row("Provider 类型、默认 Endpoint/Model 与鉴权要求均来自 Core descriptor。", "Provider 類型、預設 Endpoint/Model 與鑑權要求皆來自 Core descriptor。", "Provider type, default Endpoint/Model and auth requirements come from the Core descriptor.", "Provider 種別・既定 Endpoint/Model・認証要件は Core descriptor 由来です。", "Provider 유형, 기본 Endpoint/Model 및 인증 요구 사항은 Core descriptor에서 옵니다.") },

    Msg { key: "providers.editing", text: row("编辑渠道 {}", "編輯管道 {}", "Edit channel {}", "チャネル {} を編集", "채널 {} 편집") },

    Msg { key: "providers.name", text: row("名称", "名稱", "Name", "名前", "이름") },

    Msg { key: "providers.no_cloud_note", text: row("此 Provider 不使用云凭据；模型由本地模型面板管理。", "此 Provider 不使用雲端憑證；模型由本機模型面板管理。", "This provider uses no cloud credentials; models are managed in Local Models.", "この Provider はクラウド資格情報を使いません。モデルはローカルモデルで管理します。", "이 프로바이더는 클라우드 자격 증명을 사용하지 않습니다. 모델은 로컬 모델에서 관리합니다.") },
    Msg { key: "providers.oauth_note", text: row("此 Provider 使用 OAuth；Linux egui 不读取或显示 OAuth token。", "此 Provider 使用 OAuth；Linux egui 不讀取或顯示 OAuth token。", "This provider uses OAuth; the Linux egui UI never reads or shows the OAuth token.", "この Provider は OAuth を使用します。Linux egui は OAuth トークンを読み取らず表示もしません。", "이 프로바이더는 OAuth를 사용합니다. Linux egui는 OAuth 토큰을 읽거나 표시하지 않습니다.") },
    Msg { key: "providers.api_key_hint", text: row("API Key（留空表示不修改）", "API Key（留空表示不修改）", "API Key (blank leaves unchanged)", "API Key（空欄なら変更しない）", "API Key(비우면 변경 안 함)") },

    // ---- Empty / info labels

    // ---- Status / toast messages

    Msg { key: "status.auto_stopped", text: row("录音已自动结束", "錄音已自動結束", "Recording auto-stopped", "録音を自動終了", "녹음 자동 종료") },

    Msg { key: "status.marketplace_loaded", text: row("Marketplace 已加载 {} 个风格包", "Marketplace 已載入 {} 個風格包", "Marketplace loaded {} style packs", "Marketplace が {} 個のスタイルパックを読込", "마켓플레이스 스타일 팩 {}개 로드됨") },

    Msg { key: "status.detail_loaded", text: row("已加载风格详情：{}", "已載入風格詳情：{}", "Loaded style details: {}", "スタイル詳細を読込: {}", "스타일 상세 로드됨: {}") },
    Msg { key: "status.my_publish_likes", text: row("我的发布 {} 个，喜欢 {} 个", "我的發布 {} 個，喜歡 {} 個", "{} of my uploads · {} liked", "マイ投稿 {} 件・いいね {} 件", "내 업로드 {}개 · 좋아요 {}개") },
    Msg { key: "status.settings_saved", text: row("设置已保存", "設定已儲存", "Settings saved", "設定を保存しました", "설정 저장됨") },
    Msg { key: "status.remote_updated", text: row("远程输入状态已更新", "遠端輸入狀態已更新", "Remote input updated", "リモート入力を更新しました", "원격 입력 업데이트됨") },

    Msg { key: "status.channel_active", text: row("active 渠道已更新", "active 管道已更新", "Active channel updated", "アクティブチャネルを更新しました", "활성 채널 업데이트됨") },
    Msg { key: "status.channel_enabled", text: row("渠道启用状态已更新", "管道啟用狀態已更新", "Channel enabled state updated", "チャネルの有効状態を更新しました", "채널 사용 상태 업데이트됨") },
    Msg { key: "status.channel_reordered", text: row("渠道顺序已更新", "管道順序已更新", "Channel order updated", "チャネルの順序を更新しました", "채널 순서 업데이트됨") },
    Msg { key: "status.channel_deleted", text: row("渠道已删除", "管道已刪除", "Channel deleted", "チャネルを削除しました", "채널 삭제됨") },
    Msg { key: "status.provider_type_updated", text: row("Provider 类型已更新", "Provider 類型已更新", "Provider type updated", "Provider 種別を更新しました", "Provider 유형 업데이트됨") },
    Msg { key: "status.channel_saved", text: row("渠道配置已保存", "管道設定已儲存", "Channel configuration saved", "チャネル設定を保存しました", "채널 설정 저장됨") },
    Msg { key: "status.secret_cleared", text: row("渠道 Secret 已清除", "管道 Secret 已清除", "Channel Secret cleared", "チャネルの Secret を消去しました", "채널 Secret 지워짐") },
    Msg { key: "status.provider_validated", text: row("Provider 验证通过（{} ms）", "Provider 驗證通過（{} ms）", "Provider validated ({} ms)", "Provider 検証成功（{} ms）", "Provider 검증 통과({}ms)") },
    Msg { key: "status.log_level_saved", text: row("日志级别已保存，重启 OpenLess 后生效", "日誌等級已儲存，重新啟動 OpenLess 後生效", "Log level saved; restart OpenLess to apply", "ログレベルを保存しました。OpenLess の再起動で反映されます", "로그 레벨을 저장했습니다. OpenLess를 다시 시작하면 적용됩니다") },
    Msg { key: "status.export_log_done", text: row("错误日志已导出", "錯誤日誌已匯出", "Error log exported", "エラーログをエクスポートしました", "오류 로그 내보냄") },
    Msg { key: "status.hotkey_handled", text: row("已处理快捷键", "已處理快速鍵", "Hotkey handled", "ホットキーを処理しました", "단축키 처리됨") },
    Msg { key: "status.launch_handled", text: row("已处理启动请求", "已處理啟動請求", "Launch request handled", "起動要求を処理しました", "시작 요청 처리됨") },
    Msg { key: "status.request_restart", text: row("请手动重启 OpenLess", "請手動重新啟動 OpenLess", "Please restart OpenLess manually", "OpenLess を手動で再起動してください", "OpenLess를 수동으로 재시작하세요") },
    Msg { key: "status.tray_stopped", text: row("系统托盘已停止：{}", "系統托盤已停止：{}", "System tray stopped: {}", "システムトレイを停止: {}", "시스템 트레이 중지됨: {}") },
    Msg { key: "status.style_switched", text: row("已切换风格：{}", "已切換風格：{}", "Switched style: {}", "スタイルを切替: {}", "스타일 전환됨: {}") },
    Msg { key: "status.no_previous_style", text: row("没有可切换的上一风格", "沒有可切換的上一風格", "No previous style to switch to", "切替可能な前スタイルがありません", "전환할 이전 스타일이 없습니다") },
    Msg { key: "status.mic_selected", text: row("已选择麦克风：{}", "已選擇麥克風：{}", "Microphone selected: {}", "マイクを選択: {}", "마이크 선택됨: {}") },
    Msg { key: "status.preset_updated", text: row("词汇预设已更新", "詞彙預設已更新", "Vocabulary preset updated", "語彙プリセットを更新しました", "어휘 프리셋 업데이트됨") },

    Msg { key: "status.vocab_saved", text: row("词汇已保存", "詞彙已儲存", "Vocabulary saved", "語彙を保存しました", "어휘 저장됨") },
    Msg { key: "status.vocab_updated", text: row("词汇已更新", "詞彙已更新", "Vocabulary updated", "語彙を更新しました", "어휘 업데이트됨") },
    Msg { key: "status.correction_saved", text: row("纠错规则已保存", "糾錯規則已儲存", "Correction rule saved", "修正ルールを保存しました", "교정 규칙 저장됨") },
    Msg { key: "status.correction_updated", text: row("纠错规则已更新", "糾錯規則已更新", "Correction rule updated", "修正ルールを更新しました", "교정 규칙 업데이트됨") },

    Msg { key: "status.style_imported", text: row("已导入风格包：{}", "已匯入風格包：{}", "Imported style pack: {}", "スタイルパックを読込: {}", "스타일 팩 가져옴: {}") },
    Msg { key: "status.style_saved", text: row("风格包已保存：{}", "風格包已儲存：{}", "Style pack saved: {}", "スタイルパックを保存: {}", "스타일 팩 저장됨: {}") },
    Msg { key: "status.style_updated", text: row("风格包已更新", "風格包已更新", "Style pack updated", "スタイルパックを更新しました", "스타일 팩 업데이트됨") },

    Msg { key: "status.marketplace_installed", text: row("已安装风格包：{}", "已安裝風格包：{}", "Installed style pack: {}", "スタイルパックをインストール: {}", "스타일 팩 설치됨: {}") },
    Msg { key: "status.marketplace_like", text: row("喜欢数：{}", "喜歡數：{}", "Likes: {}", "いいね数: {}", "좋아요 수: {}") },
    Msg { key: "status.marketplace_zip_saved", text: row("Marketplace ZIP 已保存", "Marketplace ZIP 已儲存", "Marketplace ZIP saved", "Marketplace ZIP を保存しました", "마켓플레이스 ZIP 저장됨") },

    Msg { key: "status.copy_failed", text: row("复制失败：{}", "複製失敗：{}", "Copy failed: {}", "コピー失敗: {}", "복사 실패: {}") },

    Msg { key: "status.history_deleted", text: row("历史记录已删除", "歷史紀錄已刪除", "History entry deleted", "履歴を削除しました", "기록 삭제됨") },

    Msg { key: "status.recording_exported", text: row("录音已导出：{}", "錄音已匯出：{}", "Recording exported: {}", "録音をエクスポート: {}", "녹음 내보냄: {}") },
    Msg { key: "status.retranscribed", text: row("重新转写完成：{}", "重新轉寫完成：{}", "Retranscription done: {}", "再文字起こし完了: {}", "다시 받아쓰기 완료: {}") },
    Msg { key: "status.dictation_phase", text: row("听写：{}", "聽寫：{}", "Dictation: {}", "ディクテーション: {}", "받아쓰기: {}") },
    Msg { key: "status.dictation_done", text: row("听写完成：{}", "聽寫完成：{}", "Dictation done: {}", "ディクテーション完了: {}", "받아쓰기 완료: {}") },

    Msg { key: "status.backlog_reset", text: row("事件积压 {} 条，已重置派生界面并重放可用事件", "事件積壓 {} 條，已重置衍生介面並重放可用事件", "{} events backlogged — reset derived UI and replayed available events", "{} 件のイベントが滞り、派生UIをリセットして再送しました", "이벤트 {}건 밀림 — 파생 UI 재설정 및 재생됨") },
    Msg { key: "status.backlog_replay", text: row("事件积压 {} 条，已从 Core 重放补齐", "事件積壓 {} 條，已從 Core 重放補齊", "{} events backlogged — replayed from Core", "{} 件のイベントが滞り、Core から再送しました", "이벤트 {}건 밀림 — Core에서 재생됨") },

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
            "開啟後在本地儲存問答記錄，預設關閉。",
            "Save Q&A records locally when enabled. Off by default.",
            "有効時、Q&A 記録をローカルに保存。デフォルト OFF。",
            "활성화 시 Q&A 기록을 로컬에 저장. 기본 OFF.",
        ),
    },
    Msg {
        key: "selection_ask.history_title",
        text: row("保存历史", "儲存歷史", "Save history", "履歴を保存", "기록 저장"),
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
        text: row("问任何问题", "問任何問題", "Ask anything", "何でも質問", "무엇이든 질문"),
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
            "在任意輸入框聚焦遊標。",
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
            "翻譯結果自動插入到遊標位置。",
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
        text: row("翻译", "TRANSLATION", "TRANSLATION", "翻訳", "번역"),
    },
    Msg {
        key: "translation.status_disabled",
        text: row("未启用", "未啟用", "Disabled", "無効", "비활성화됨"),
    },
    Msg {
        key: "translation.status_enabled",
        text: row("已启用", "已啟用", "Enabled", "有効", "활성화됨"),
    },
    Msg {
        key: "translation.style_desc",
        text: row(
            "自动继承「风格」页当前激活的风格包。",
            "自動沿用「風格」頁目前啟用的風格包。",
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
            "不啟用（Shift 按下不觸發翻譯）",
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
            "新增生詞或專業術語，提高識別準確率。",
            "Add terms or jargon to improve recognition accuracy.",
            "新語や専門用語を追加して認識精度を向上。",
            "새 단어나 전문 용어를 추가하여 인식 정확도 향상.",
        ),
    },
    Msg {
        key: "vocab.kicker",
        text: row("词典", "VOCABULARY", "DICTIONARY", "辞書", "사전"),
    },
    Msg {
        key: "vocabCard.title",
        text: row(
            "要记住这个词吗？",
            "要記住這個詞嗎？",
            "Remember this word?",
            "この語を覚えますか？",
            "이 단어를 기억할까요?",
        ),
    },
    Msg {
        key: "vocabCard.accept",
        text: row("记住", "記住", "Remember", "覚える", "기억하기"),
    },
    Msg {
        key: "vocabCard.reject",
        text: row("不用", "不用", "Skip", "不要", "건너뛰기"),
    },
    Msg {
        key: "vocab.learned_section",
        text: row(
            "确认收集（{}）",
            "確認收集（{}）",
            "Confirmed ({})",
            "確認して追加（{}）",
            "확인 후 수집 ({})",
        ),
    },
    Msg {
        key: "vocab.placeholder",
        text: row(
            "输入词语，按 Enter 或点添加…",
            "輸入詞語，按 Enter 或點新增…",
            "Type a word, press Enter or click Add…",
            "単語を入力し、Enter または追加をクリック…",
            "단어를 입력하고 Enter 또는 추가 클릭…",
        ),
    },
    Msg {
        key: "vocab.presets_apply",
        text: row(
            "启用所选",
            "啟用所選",
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
        text: row("保存预设", "儲存預設", "Save preset", "プリセットを保存", "프리셋 저장"),
    },
    Msg {
        key: "vocab.presets_tip",
        text: row(
            "可多选批量启用，支持编辑和新建。",
            "可多選批次啟用，支援編輯和新建。",
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
            "支援中英混合 · 數字開頭按字面識別 · 命中次數自動計數",
            "Mixed Chinese/English supported · numeric prefixes are matched literally · hits counted automatically",
            "日本語と英数の混在対応 · 数字始まりは字面通り認識 · ヒット回数を自動カウント",
            "한영 혼용 지원 · 숫자로 시작하면 그대로 인식 · 적중 횟수 자동 카운트",
        ),
    },
    Msg {
        key: "vocab.title",
        text: row("词典", "詞彙表", "Dictionary", "辞書", "사전"),
    },
    Msg {
        key: "style.custom_prompt_save",
        text: row("保存提示词", "儲存提示詞", "Save prompt", "プロンプトを保存", "프롬프트 저장"),
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
        text: row("风格", "STYLE", "STYLE", "スタイル", "스타일"),
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
            "瀏覽、安裝和分享社群風格包。",
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
        text: row("风格市场", "MARKETPLACE", "MARKETPLACE", "マーケット", "마켓"),
    },
    Msg {
        key: "marketplace.upload_btn",
        text: row("上传", "上傳", "Upload", "アップロード", "업로드"),
    },
    Msg {
        key: "marketplace.upload.title",
        text: row(
            "选择要上传的风格包",
            "選擇要上傳的風格包",
            "Pick a style pack to upload",
            "アップロードするパックを選択",
            "업로드할 팩 선택",
        ),
    },
    Msg {
        key: "marketplace.upload.updateTitle",
        text: row("更新「{}」", "更新「{}」", "Update \"{}\"", "「{}」を更新", "\"{}\" 업데이트"),
    },
    Msg {
        key: "marketplace.upload.hint",
        text: row("选择一个本地风格包发布到市场。", "選擇一個本機風格包發布到市場。", "Choose a local style pack to publish.", "公開するローカルスタイルパックを選択してください。", "게시할 로컬 스타일 팩을 선택하세요."),
    },
    Msg {
        key: "marketplace.upload.updateHint",
        text: row(
            "选中要上传的本地新版本风格包，下方点「确定上传」。同名包默认预选。",
            "選中要上傳的本機新版本風格包，下方點「確定上傳」。同名包預設預選。",
            "Pick the local newer version, then click \"Confirm upload\". A same-name pack is pre-selected.",
            "アップロードするローカルの新版を選んで「アップロード確定」を押してください。同名パックは自動選択されます。",
            "업로드할 로컬 최신본을 선택하고 \"업로드 확정\"을 누르세요. 동명 팩이 기본 선택됩니다.",
        ),
    },
    Msg {
        key: "marketplace.upload.noLocal",
        text: row(
            "本地没有可上传的风格包",
            "本機沒有可上傳的風格包",
            "No local style packs to upload",
            "アップロード可能なローカルパックがありません",
            "업로드 가능한 로컬 팩이 없습니다",
        ),
    },
    Msg {
        key: "marketplace.upload.confirm",
        text: row("确定上传", "確定上傳", "Upload", "アップロード", "업로드"),
    },
    Msg {
        key: "marketplace.upload.submitting",
        text: row("正在提交…", "正在提交…", "Submitting…", "送信中…", "제출 중…"),
    },
    Msg {
        key: "marketplace.withdraw.confirm",
        text: row("确定要下架“{}”吗？", "確定要下架「{}」嗎？", "Withdraw “{}” from the marketplace?", "「{}」をマーケットから取り下げますか？", "“{}”을(를) 마켓에서 내릴까요?"),
    },
    Msg {
        key: "marketplace.withdraw.confirmBtn",
        text: row("确认下架", "確認下架", "Withdraw", "取り下げる", "내리기"),
    },
    Msg {
        key: "marketplace.withdraw.success",
        text: row("已下架", "已下架", "Publication withdrawn", "公開を取り下げました", "게시물을 내렸습니다"),
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
            "釋出日誌",
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
            "OpenAI 相容",
            "OpenAI-compatible",
            "OpenAI 互換",
            "OpenAI 호환",
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
            "介面語言",
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
        text: row("布局", "佈局", "Layout", "レイアウト", "레이아웃"),
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
    // Linux-only settings: session services and the real data/credential split.
    Msg {
        key: "settings.linux_privacy.refresh",
        text: row("重新检查会话与网络", "重新檢查工作階段與網路", "Recheck session & network", "セッションとネットワークを再確認", "세션 및 네트워크 다시 확인"),
    },
    Msg {
        key: "settings.linux_privacy.not_checked",
        text: row("尚未检查", "尚未檢查", "Not checked", "未確認", "확인 전"),
    },
    Msg {
        key: "settings.linux_privacy.input_ready",
        text: row("OpenLess 插件接口可达", "OpenLess 外掛介面可達", "OpenLess addon responds", "OpenLess プラグイン応答あり", "OpenLess 플러그인 응답 확인"),
    },
    Msg {
        key: "settings.linux_privacy.input_unavailable",
        text: row("插件接口不可达", "外掛介面無法連線", "Addon interface unavailable", "プラグインに接続できません", "플러그인 인터페이스 사용 불가"),
    },
    Msg {
        key: "settings.linux_privacy.network_label",
        text: row("OpenLess 网络连接", "OpenLess 網路連線", "OpenLess network connection", "OpenLess ネットワーク接続", "OpenLess 네트워크 연결"),
    },
    Msg {
        key: "settings.linux_privacy.network_online",
        text: row("HTTP 可达（{} ms）", "HTTP 可達（{} ms）", "HTTP reachable ({} ms)", "HTTP 接続確認（{} ms）", "HTTP 연결 확인 ({} ms)"),
    },
    Msg {
        key: "settings.linux_privacy.network_unavailable",
        text: row("OpenLess 服务不可达", "OpenLess 服務無法連線", "OpenLess endpoint unreachable", "OpenLess サービスに接続できません", "OpenLess 서비스 연결 불가"),
    },
    Msg {
        key: "settings.linux_privacy.network_probe_desc",
        text: row(
            "与 Tauri 相同：GET OpenLess /packs，8 秒超时，收到 HTTP 响应才算可达。每 30 秒刷新；不代表 ASR/LLM 渠道鉴权有效，请在 AI 服务页验证渠道。",
            "與 Tauri 相同：GET OpenLess /packs，8 秒逾時，收到 HTTP 回應才算可達。每 30 秒更新；不代表 ASR/LLM 渠道驗證有效，請在 AI 服務頁驗證渠道。",
            "Same as Tauri: GET OpenLess /packs, with an 8-second timeout; an HTTP response proves reachability. Refreshes every 30 seconds. This does not validate ASR/LLM authentication; verify channels in AI Services.",
            "Tauri と同じく OpenLess /packs に GET し、8 秒でタイムアウトします。HTTP 応答で接続を確認し、30 秒ごとに更新します。ASR/LLM 認証は AI サービスで別途検証してください。",
            "Tauri와 동일하게 OpenLess /packs에 GET 요청하며 제한 시간은 8초입니다. HTTP 응답으로 연결을 확인하고 30초마다 갱신합니다. ASR/LLM 인증은 AI 서비스에서 별도로 확인하세요.",
        ),
    },
    Msg {
        key: "settings.linux_privacy.keyring_ready",
        text: row("默认保险库已解锁", "預設保管庫已解鎖", "Default vault unlocked", "既定保管庫は解除済み", "기본 보관소 잠금 해제됨"),
    },
    Msg {
        key: "settings.linux_privacy.keyring_locked",
        text: row("默认保险库已锁定", "預設保管庫已鎖定", "Default vault locked", "既定保管庫はロック中", "기본 보관소 잠김"),
    },
    Msg {
        key: "settings.linux_privacy.keyring_missing_service",
        text: row("Secret Service 未运行", "Secret Service 未執行", "Secret Service not running", "Secret Service 未起動", "Secret Service 실행 안 됨"),
    },
    Msg {
        key: "settings.linux_privacy.keyring_missing_collection",
        text: row("缺少默认保险库", "缺少預設保管庫", "No default vault", "既定保管庫がありません", "기본 보관소 없음"),
    },
    Msg {
        key: "settings.linux_privacy.keyring_temporary",
        text: row("默认保险库仅临时保存，原凭据可能在其他钱包中", "預設保管庫僅暫存，原憑據可能在其他錢包中", "Temporary default vault; existing credentials may be in another wallet", "既定保管庫は一時保存です。元の認証情報は別の保管庫にある可能性があります", "임시 기본 보관소입니다. 기존 자격 증명은 다른 보관소에 있을 수 있습니다"),
    },
    Msg {
        key: "settings.linux_privacy.keyring_unavailable",
        text: row("会话或保险库检查失败", "工作階段或保管庫檢查失敗", "Session/vault check failed", "セッション・保管庫の確認失敗", "세션/보관소 확인 실패"),
    },
    Msg {
        key: "settings.linux_privacy.keyring_restart",
        text: row("服务已恢复，需重启 OpenLess", "服務已恢復，需重新啟動 OpenLess", "Service restored; restart OpenLess", "サービス復旧済み・OpenLess 再起動が必要", "서비스 복구됨, OpenLess 재시작 필요"),
    },
    Msg {
        key: "settings.linux_privacy.keyring_probe_desc",
        text: row(
            "只检查会话 D-Bus、默认保险库的锁定状态与适配器初始化结果。不读取密钥、不自动解锁或写入测试密钥；实际保存权限仍以保存结果为准。",
            "僅檢查工作階段 D-Bus、預設保管庫的鎖定狀態與介面初始化結果。不讀取金鑰、不自動解鎖或寫入測試金鑰；實際儲存權限仍以儲存結果為準。",
            "Checks session D-Bus, default-vault lock state and adapter initialization only. No secret reads, automatic unlocks or test writes; actual write access is determined when saving.",
            "セッション D-Bus、既定保管庫のロック状態、接続初期化のみ確認します。キーの読み取り、自動解除、テスト書き込みは行いません。書き込み権限は実際の保存結果で確認してください。",
            "세션 D-Bus, 기본 보관소 잠금 상태 및 어댑터 초기화만 확인합니다. 키 읽기, 자동 잠금 해제, 테스트 쓰기는 하지 않습니다. 실제 쓰기 권한은 저장 결과로 확인하세요.",
        ),
    },
    Msg {
        key: "settings.linux_privacy.section_desc",
        text: row(
            "Linux 会话、Keyring / Secret Service，以及本地历史、录音和偏好。",
            "Linux 工作階段、Keyring / Secret Service，以及本機歷史、錄音和偏好。",
            "Linux session services, Keyring / Secret Service, local history, recordings and preferences.",
            "Linux セッション、Keyring / Secret Service、ローカル履歴、録音、設定。",
            "Linux 세션, Keyring / Secret Service, 로컬 기록, 녹음 및 설정.",
        ),
    },
    Msg {
        key: "settings.linux_privacy.intro_title",
        text: row("Linux 数据与隐私", "Linux 資料與隱私", "Linux data & privacy", "Linux データとプライバシー", "Linux 데이터 및 개인정보"),
    },
    Msg {
        key: "settings.linux_privacy.intro_desc",
        text: row(
            "记录保存在本机；使用云端服务时，相应音频、文本及已启用的上下文会发送至配置的服务商。",
            "記錄儲存在本機；使用雲端服務時，相關音訊、文字及已啟用的上下文會傳送至設定的服務商。",
            "Records are stored locally. Cloud services receive the relevant audio, text and enabled context at the providers you configure.",
            "記録はローカルに保存されます。クラウド利用時は、関連する音声、テキスト、有効なコンテキストが設定したプロバイダーに送信されます。",
            "기록은 로컬에 저장됩니다. 클라우드 사용 시 관련 오디오, 텍스트 및 활성화된 컨텍스트가 설정한 제공업체로 전송됩니다.",
        ),
    },
    Msg {
        key: "settings.linux_privacy.title",
        text: row("Linux 会话与访问", "Linux 工作階段與存取", "Linux session & access", "Linux セッションとアクセス", "Linux 세션 및 접근"),
    },
    Msg {
        key: "settings.linux_privacy.desc",
        text: row(
            "Linux 使用桌面会话服务；设备检测和后端就绪不代表每次录音或输入均获准。",
            "Linux 使用桌面工作階段服務；偵測到裝置及後端就緒不代表每次錄音或輸入皆獲准。",
            "Linux uses desktop session services. Device detection and backend initialization do not guarantee capture or input access.",
            "Linux はデスクトップのセッションサービスを使います。デバイス検出やバックエンド初期化は録音・入力アクセスの保証ではありません。",
            "Linux는 데스크톱 세션 서비스를 사용합니다. 장치 감지 및 백엔드 초기화가 녹음이나 입력 접근을 보장하지는 않습니다.",
        ),
    },
    Msg {
        key: "settings.linux_privacy.mic_desc",
        text: row(
            "经 PipeWire/PulseAudio/ALSA 录音；检测到设备不等于允许录音，访问限制需在音频或沙盒设置中处理。",
            "透過 PipeWire/PulseAudio/ALSA 錄音；偵測到裝置不等於允許錄音，存取限制需在音訊或沙箱設定中處理。",
            "Capture uses PipeWire/PulseAudio/ALSA. Detecting a device does not grant capture access; check audio or sandbox restrictions if recording fails.",
            "録音は PipeWire/PulseAudio/ALSA を使います。デバイス検出は録音許可ではありません。失敗時は音声やサンドボックスの制限を確認してください。",
            "녹음은 PipeWire/PulseAudio/ALSA를 사용합니다. 장치 감지는 녹음 허가가 아닙니다. 녹음 실패 시 오디오 또는 샌드박스 제한을 확인하세요.",
        ),
    },
    Msg {
        key: "settings.linux_privacy.mic_detected",
        text: row("已检测到设备", "已偵測到裝置", "Device detected", "デバイス検出済み", "장치 감지됨"),
    },
    Msg {
        key: "settings.linux_privacy.hotkey_desc",
        text: row(
            "Fcitx5/OpenLess 全局快捷键监听后端须初始化；窗口内按键生效不代表全局监听可用。",
            "Fcitx5/OpenLess 全域快捷鍵監聽後端須初始化；視窗內按鍵生效不代表全域監聽可用。",
            "Global shortcuts require the Fcitx5/OpenLess listener backend. Keys working inside the app do not prove global listening is available.",
            "グローバルショートカットには Fcitx5/OpenLess の監視バックエンドが必要です。アプリ内のキー動作だけではグローバル監視を確認できません。",
            "전역 단축키에는 Fcitx5/OpenLess 수신 백엔드가 필요합니다. 앱 내 키 동작만으로 전역 수신이 가능하다고 판단할 수 없습니다.",
        ),
    },
    Msg {
        key: "settings.linux_privacy.hotkey_ready",
        text: row("监听后端已初始化", "監聽後端已初始化", "Listener initialized", "監視バックエンド初期化済み", "수신 백엔드 초기화됨"),
    },
    Msg {
        key: "settings.linux_privacy.input_label",
        text: row("选区与文字插入", "選取範圍與文字插入", "Selection & text insertion", "選択範囲とテキスト挿入", "선택 영역 및 텍스트 삽입"),
    },
    Msg {
        key: "settings.linux_privacy.input_desc",
        text: row(
            "跨应用文本输入和选区服务由 Fcitx5/OpenLess 插件提供；Wayland 限制模拟按键和选区访问。后端不可用时可能只能复制到剪贴板。",
            "跨應用程式文字輸入和選取服務由 Fcitx5/OpenLess 外掛提供；Wayland 限制模擬按鍵和選取存取。後端無法使用時可能只能複製到剪貼簿。",
            "Fcitx5/OpenLess provides cross-app text and selection services. Wayland restricts synthetic keys and selection access; an unavailable backend may leave only clipboard copying.",
            "Fcitx5/OpenLess プラグインがアプリ間のテキスト入力と選択を提供します。Wayland はキー送信と選択へのアクセスを制限します。バックエンド不在時はコピーのみになる場合があります。",
            "Fcitx5/OpenLess 플러그인이 앱 간 텍스트 입력과 선택 서비스를 제공합니다. Wayland는 키 합성과 선택 접근을 제한합니다. 백엔드를 사용할 수 없으면 클립보드 복사만 가능할 수 있습니다.",
        ),
    },
    Msg {
        key: "settings.linux_privacy.network_desc",
        text: row(
            "云端 ASR/LLM 需能访问服务端；请在 AI 服务页验证渠道连接，而不是寻找统一的系统网络授权按钮。",
            "雲端 ASR/LLM 需能連線至服務端；請在 AI 服務頁驗證渠道連線，而不是尋找統一的系統網路授權按鈕。",
            "Cloud ASR/LLM needs access to the provider endpoint. Verify each channel in AI Services rather than looking for a generic system network permission button.",
            "クラウド ASR/LLM にはプロバイダーへの接続が必要です。汎用のネットワーク許可ボタンではなく、AI サービスで各チャネルを検証してください。",
            "클라우드 ASR/LLM은 제공업체 엔드포인트에 접근해야 합니다. 일반 시스템 네트워크 권한 버튼 대신 AI 서비스에서 각 채널을 확인하세요.",
        ),
    },
    Msg {
        key: "settings.linux_privacy.verify_channels",
        text: row("按渠道验证", "依渠道驗證", "Verify per channel", "チャネルごとに検証", "채널별 확인"),
    },
    Msg {
        key: "settings.linux_privacy.keyring_title",
        text: row("凭据保险库（Keyring）", "憑證保管庫（Keyring）", "Credential vault (Keyring)", "認証情報保管庫（Keyring）", "자격 증명 보관소 (Keyring)"),
    },
    Msg {
        key: "settings.linux_privacy.keyring_desc",
        text: row(
            "API 密钥和敏感请求头保存在用户会话的 Secret Service（org.freedesktop.secrets）。需启用 GNOME Keyring 或提供兼容接口的 KWallet，并解锁保险库。",
            "API 金鑰和敏感請求標頭儲存在使用者工作階段的 Secret Service（org.freedesktop.secrets）。需啟用 GNOME Keyring 或提供相容介面的 KWallet，並解鎖保管庫。",
            "API keys and sensitive headers are stored in your session's Secret Service (org.freedesktop.secrets). Enable GNOME Keyring or a compatible KWallet service and unlock the vault.",
            "API キーと機密ヘッダーはセッションの Secret Service（org.freedesktop.secrets）に保存されます。GNOME Keyring または互換の KWallet サービスを有効にして保管庫を解除してください。",
            "API 키와 민감한 헤더는 사용자 세션의 Secret Service (org.freedesktop.secrets)에 저장됩니다. GNOME Keyring 또는 호환 KWallet 서비스를 활성화하고 보관소 잠금을 해제하세요.",
        ),
    },
    Msg {
        key: "settings.linux_privacy.keyring_required",
        text: row("需运行并解锁", "需執行並解鎖", "Must run and be unlocked", "起動とロック解除が必要", "실행 및 잠금 해제 필요"),
    },
    Msg {
        key: "settings.linux_privacy.keyring_recovery",
        text: row(
            "会话 D-Bus/服务不可用或保险库锁定时，凭据保存可能失败。恢复服务并解锁后重启 OpenLess 再重试；错误不代表已有密钥被删除。",
            "工作階段 D-Bus/服務無法使用或保管庫鎖定時，憑證儲存可能失敗。恢復服務並解鎖後重新啟動 OpenLess 再重試；錯誤不代表既有金鑰被刪除。",
            "An unavailable session D-Bus/service or locked vault can prevent saving. Restore the service, unlock the vault, restart OpenLess and retry. An error does not mean existing keys were deleted.",
            "セッションの D-Bus・サービスが使えない場合や保管庫がロック中の場合、保存に失敗することがあります。サービスを復旧し、保管庫を解除して OpenLess を再起動してください。エラーは既存キーの削除を意味しません。",
            "세션 D-Bus/서비스를 사용할 수 없거나 보관소가 잠기면 저장이 실패할 수 있습니다. 서비스를 복구하고 잠금을 해제한 뒤 OpenLess를 다시 시작하여 재시도하세요. 오류가 기존 키 삭제를 의미하지는 않습니다.",
        ),
    },
    Msg {
        key: "settings.linux_privacy.storage_desc",
        text: row(
            "历史、偏好、词库与已保存的录音位于应用数据目录。API 密钥使用 Keyring 存储，不写入 credential-metadata.json；录音留存设置不清除保险库。",
            "歷史、偏好、詞庫與已儲存的錄音位於應用程式資料目錄。API 金鑰使用 Keyring 儲存，不寫入 credential-metadata.json；錄音保留設定不清除保管庫。",
            "History, preferences, vocabulary and saved recordings live in the app data directory. API keys use Keyring, not credential-metadata.json; recording retention does not clear the vault.",
            "履歴、設定、辞書、保存した録音はアプリのデータディレクトリにあります。API キーは credential-metadata.json ではなく Keyring に保存されます。録音の保持設定は保管庫を消去しません。",
            "기록, 설정, 어휘 및 저장된 녹음은 앱 데이터 디렉터리에 있습니다. API 키는 credential-metadata.json이 아닌 Keyring에 저장됩니다. 녹음 보존 설정은 보관소를 지우지 않습니다.",
        ),
    },
    Msg {
        key: "settings.linux_privacy.data_directory",
        text: row("应用数据目录", "應用程式資料目錄", "App data directory", "アプリデータの場所", "앱 데이터 디렉터리"),
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
            "定義全域錄音的快捷鍵與觸發方式。",
            "Global recording hotkey and trigger mode.",
            "グローバル録音のショートカットとトリガー方式を定義します。",
            "전역 녹음의 단축키와 트리거 방식을 정의합니다.",
        ),
    },
    Msg {
        key: "settings.recording.insert_group_title",
        text: row(
            "插入与剪贴板",
            "插入與剪貼簿",
            "Insertion & clipboard",
            "挿入とクリップボード",
            "삽입 및 클립보드",
        ),
    },
    Msg {
        key: "settings.recording.microphone_system_default",
        text: row(
            "系统默认",
            "系統預設",
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
        key: "settings.recording.audio_enhancement_label",
        text: row(
            "语音增强",
            "語音增強",
            "Speech enhancement",
            "音声強調",
            "음성 향상",
        ),
    },
    Msg {
        key: "settings.recording.echo_cancellation_label",
        text: row(
            "播放回声抑制",
            "播放回聲抑制",
            "Playback echo cancellation",
            "再生エコーキャンセル",
            "재생 에코 제거",
        ),
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
            "模擬貼上快捷鍵",
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
        text: row("监听端口", "監聽連線埠", "Port", "待ち受けポート", "수신 포트"),
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
            "關於",
            "About & updates",
            "バージョンと更新",
            "정보 및 업데이트",
        ),
    },
    Msg {
        key: "modal.sections.advanced",
        text: row(
            "实验与扩展",
            "高階",
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
            "通用",
            "Recording & input",
            "録音と入力",
            "녹음 및 입력",
        ),
    },
    Msg {
        key: "modal.sections.privacy",
        text: row(
            "权限与数据",
            "隱私",
            "Permissions & data",
            "権限とデータ",
            "권한 및 데이터",
        ),
    },
    Msg {
        key: "modal.sections.services",
        text: row(
            "AI 服务与模型",
            "服務",
            "AI services & models",
            "AI サービスとモデル",
            "AI 서비스 및 모델",
        ),
    },
    Msg {
        key: "modal.sections.shortcuts",
        text: row("快捷键", "快捷鍵", "Shortcuts", "ショートカットと選択", "단축키 및 선택"),
    },
    Msg {
        key: "settings.selection_workspace.hint",
        text: row(
            "有选区时按同一快捷键：关闭语音编辑则润色；开启后口述指令，可提问或编辑选区。无选区但光标在文本框时：可「帮我写」成稿或提问。",
            "選中文字後按同一快捷鍵：關閉語音編輯時直接潤色；開啟後口述指令，說完再選擇「提問」或「編輯選區」。",
            "With a selection, one shortcut polishes (voice edit off) or takes a spoken instruction to ask or edit. With no selection but focus in a text field, speak to Help me write a draft or ask a question.",
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
        text: row("确认收集", "確認收集", "Confirmed", "確認して追加", "확인 후 수집"),
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
        key: "settings.about.docs",
        text: row("文档", "檔案", "Docs", "ドキュメント", "문서"),
    },
    Msg {
        key: "settings.about.feedback",
        text: row("反馈", "反饋", "Feedback", "フィードバック", "피드백"),
    },
    Msg {
        key: "settings.about.links_title",
        text: row(
            "文档链接",
            "檔案連結",
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
            "社群 QQ 群",
            "QQ community group",
            "コミュニティ QQ グループ",
            "커뮤니티 QQ 그룹",
        ),
    },
    Msg {
        key: "settings.about.source",
        text: row("源码", "原始碼", "Source", "ソース", "소스"),
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
            "該平臺暫未支援本地 ASR 模型整合。",
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
            "還沒有渠道。點下面的「新增渠道」建立一個。",
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
        key: "settings.channels.connection_title",
        text: row(
            "服务连接",
            "服務連線",
            "Service connection",
            "サービス接続",
            "서비스 연결",
        ),
    },
    Msg {
        key: "settings.channels.edit_title",
        text: row(
            "编辑渠道",
            "編輯渠道",
            "Edit channel",
            "チャネルを編集",
            "채널 편집",
        ),
    },
    Msg {
        key: "settings.channels.create_title",
        text: row(
            "添加渠道",
            "新增渠道",
            "Add channel",
            "チャネルを追加",
            "채널 추가",
        ),
    },
    Msg {
        key: "settings.channels.auto_save_hint",
        text: row(
            "字段修改后自动保存；完成配置后，可手动验证连接。",
            "修改欄位後會自動儲存；完成設定後，可手動驗證連線。",
            "Changes save automatically. Once configured, you can check the connection manually.",
            "変更は自動保存されます。設定が終わったら、手動で接続を確認できます。",
            "변경 사항은 자동으로 저장됩니다. 설정을 마친 후 연결을 직접 확인할 수 있습니다.",
        ),
    },
    Msg {
        key: "settings.channels.name_label",
        text: row(
            "渠道名称（可选）",
            "名稱",
            "Channel name (optional)",
            "チャネル名（任意）",
            "채널 이름 (선택)",
        ),
    },
    Msg {
        key: "settings.channels.done",
        text: row("完成", "完成", "Done", "完了", "완료"),
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
            "拖曳排序，最上面的優先使用；關掉的渠道會自動排到末尾。",
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
            "切換 UI 顯示語言。當前會話即時生效，下次啟動自動沿用。",
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
            "部分原生選單（系統託盤等）可能需要重啟 App 才會切換。",
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
            "OpenLess 需要麥克風可用，並依賴全域快捷鍵監聽狀態判斷 native hook 是否正常工作。",
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
            "全域快捷鍵",
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
        text: row("网络", "網路", "Network", "ネットワーク", "네트워크"),
    },
    Msg {
        key: "settings.permissions.network_ok",
        text: row("可用", "可用", "Available", "利用可能", "사용 가능"),
    },
    Msg {
        key: "settings.permissions.open_system",
        text: row(
            "打开系统设置",
            "開啟系統設定",
            "Open System Settings",
            "システム設定を開く",
            "시스템 설정 열기",
        ),
    },
    Msg {
        key: "settings.providers.credential_storage_notice",
        text: row(
            "凭据保存在系统凭据库中。",
            "憑據儲存在系統憑據庫中。",
            "Credentials are stored in the OS credential vault.",
            "資格情報は OS の資格情報ストアに保存されます。",
            "자격 증명은 OS 자격 증명 저장소에 보관됩니다.",
        ),
    },
    Msg {
        key: "settings.channels.provider_label",
        text: row("供应商", "供應商", "Provider", "プロバイダー", "공급자"),
    },
    Msg {
        key: "settings.channels.drag_hint",
        text: row(
            "按住拖动可调整优先级",
            "按住拖曳可調整優先順序",
            "Drag to change priority",
            "ドラッグで優先順位を変更",
            "드래그해서 우선순위 변경",
        ),
    },
    Msg {
        key: "settings.channels.delete_confirm",
        text: row(
            "删除后该渠道保存的密钥也会一并清除。",
            "刪除後該渠道儲存的金鑰也會一併清除。",
            "Deleting also clears the keys stored for this channel.",
            "削除するとこのチャネルに保存された鍵も消去されます。",
            "삭제하면 이 채널에 저장된 키도 함께 지워집니다.",
        ),
    },
    Msg {
        key: "settings.channels.confirm_delete",
        text: row("确认删除", "確認刪除", "Delete", "削除する", "삭제"),
    },
    Msg {
        key: "settings.channels.last_check",
        text: row(
            "上次验证",
            "上次驗證",
            "Last check",
            "前回の接続確認",
            "마지막 확인",
        ),
    },
    Msg {
        key: "settings.channels.verifying",
        text: row(
            "正在验证…",
            "正在驗證…",
            "Checking…",
            "確認中…",
            "확인 중…",
        ),
    },
    Msg {
        key: "settings.channels.reverify",
        text: row("重新验证", "重新驗證", "Check again", "再確認", "다시 확인"),
    },
    Msg {
        key: "settings.channels.failed_plain",
        text: row("验证失败", "驗證失敗", "Check failed", "確認に失敗", "확인 실패"),
    },
    Msg {
        key: "settings.channels.failure_keeps_enabled",
        text: row(
            "验证失败不会自动停用。请求仍使用列表中第一个已启用的渠道，不会自动改用下一个。",
            "驗證失敗不會自動停用。請求仍使用列表中第一個已啟用的渠道，不會自動改用下一個。",
            "A failed check does not turn this service off. Requests still use the first enabled service and do not switch to the next one.",
            "確認に失敗しても、このサービスは自動では停止しません。リクエストは有効な一覧の先頭を使い、次へは自動で切り替わりません。",
            "확인에 실패해도 이 서비스는 자동으로 꺼지지 않습니다. 요청은 켜진 목록의 첫 항목을 쓰며, 다음 항목으로 자동 전환되지 않습니다.",
        ),
    },
    Msg {
        key: "settings.channels.stale_result",
        text: row(
            "结果已超过 24 小时",
            "結果已超過 24 小時",
            "Result is over 24 hours old",
            "24 時間以上前の結果",
            "24시간이 지난 결과",
        ),
    },
    Msg {
        key: "settings.channels.just_now",
        text: row("刚刚", "剛剛", "just now", "たった今", "방금"),
    },
    Msg {
        key: "settings.channels.minutes_ago",
        text: row("{} 分钟前", "{} 分鐘前", "{}m ago", "{}分前", "{}분 전"),
    },
    Msg {
        key: "settings.channels.hours_ago",
        text: row("{} 小时前", "{} 小時前", "{}h ago", "{}時間前", "{}시간 전"),
    },
    Msg {
        key: "settings.channels.days_ago",
        text: row("{} 天前", "{} 天前", "{}d ago", "{}日前", "{}일 전"),
    },
    Msg {
        key: "settings.channels.model_not_set",
        text: row(
            "模型未单独设置",
            "未單獨設定模型",
            "No model set explicitly",
            "モデルの個別設定なし",
            "모델을 별도로 설정하지 않음",
        ),
    },
    Msg {
        key: "settings.providers.pipeline_mode_label",
        text: row(
            "识别管线",
            "識別管線",
            "Pipeline mode",
            "認識パイプライン",
            "인식 파이프라인",
        ),
    },
    Msg {
        key: "settings.providers.pipeline_mode_hint",
        text: row(
            "传统 = ASR 转写 + LLM 润色两段式；多模态 = 单个多模态模型一次完成。",
            "傳統 = ASR 轉寫 + LLM 潤色兩段式；多模態 = 單一多模態模型一次完成。",
            "Traditional = ASR + LLM two-stage. Multimodal = a single audio-capable model in one pass.",
            "従来 = ASR 文字起こし + LLM 整形の2段式。マルチモーダル = 音声対応モデルが1回で完了。",
            "전통 = ASR 전사 + LLM 다듬기 2단계. 멀티모달 = 오디오 지원 모델이 한 번에 처리.",
        ),
    },
    Msg {
        key: "settings.providers.pipeline_mode_traditional",
        text: row(
            "传统模式",
            "傳統模式",
            "Traditional",
            "従来モード",
            "전통 모드",
        ),
    },
    Msg {
        key: "settings.providers.pipeline_mode_multimodal",
        text: row(
            "多模态模式",
            "多模態模式",
            "Multimodal",
            "マルチモーダルモード",
            "멀티모달 모드",
        ),
    },
    Msg {
        key: "settings.providers.pipeline_isolation_notice",
        text: row(
            "两种模式使用完全独立的凭据配置。切换模式不会删除另一套配置，只是暂时停用；切回即恢复。",
            "兩種模式使用完全獨立的憑證設定。切換模式不會刪除另一套設定，只是暫時停用；切回即還原。",
            "The two modes keep fully separate credentials. Switching modes keeps the other set stored but unused; switching back restores it.",
            "2つのモードは完全に独立した認証情報を使用します。切り替えてももう一方の設定は削除されず、切り戻せば復元されます。",
            "두 모드는 완전히 분리된 자격 증명을 사용합니다. 전환해도 다른 쪽 설정은 삭제되지 않으며, 다시 전환하면 복원됩니다.",
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
            "Ctrl+V（預設 / 多數應用）",
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
        key: "settings.debug.log_level_label",
        text: row("日志级别", "日誌等級", "Log level", "ログレベル", "로그 레벨"),
    },
    Msg {
        key: "settings.debug.log_level_desc",
        text: row(
            "debug 记录心跳与 IPC 细节，排查问题时更有用；选择后需重启 OpenLess 生效。",
            "debug 記錄心跳與 IPC 細節，排查問題時更有用；選擇後需重新啟動 OpenLess 生效。",
            "debug records heartbeats and IPC details, which is what support needs; restart OpenLess to apply.",
            "debug はハートビートと IPC の詳細を記録します（調査向け）。OpenLess の再起動で反映されます。",
            "debug는 하트비트와 IPC 세부 정보를 기록합니다(문제 해결용). OpenLess를 다시 시작해야 적용됩니다.",
        ),
    },
    Msg {
        key: "settings.recording.archive_processed_label",
        text: row("保存处理后的音频", "儲存處理後的音訊", "Keep processed audio", "処理後の音声を保存", "처리된 오디오 저장"),
    },
    Msg {
        key: "settings.recording.archive_processed_desc",
        text: row(
            "降噪与增益之后的版本，仍保存在原来的录制文件里（quick note 播放、本地模型回退都用它）。",
            "降噪與增益之後的版本，仍存於原本的錄製檔案（quick note 播放、本機模型回退都用它）。",
            "The version after noise reduction and gain, still stored in the existing recording file (used by quick-note playback and the local model fallback).",
            "ノイズ低減とゲイン適用後の音声で、従来の録音ファイルに保存されます（クイックノート再生とローカルモデルのフォールバックが使います）。",
            "노이즈 감소와 게인 적용 후의 오디오로, 기존 녹음 파일에 저장됩니다(퀵노트 재생과 로컬 모델 폴백이 사용).",
        ),
    },
    Msg {
        key: "settings.recording.archive_raw_label",
        text: row("另存一份处理前的音频", "另存一份處理前的音訊", "Also keep pre-processing audio", "処理前の音声も保存", "처리 전 오디오도 저장"),
    },
    Msg {
        key: "settings.recording.archive_raw_desc",
        text: row(
            "同一目录下另存 <会话>.raw.wav，可与上面那份逐段对照；只开这一项时，原始音频直接存进主档案。",
            "同一目錄下另存 <session>.raw.wav，可與上面那份逐段對照；只開這一項時，原始音訊直接存進主檔案。",
            "Writes <session>.raw.wav next to it so you can compare segment by segment; with only this enabled the raw audio goes into the main archive.",
            "同じフォルダに <session>.raw.wav を追加保存し、上記と区間ごとに比較できます。これだけを有効にすると元音声がメインファイルに保存されます。",
            "같은 폴더에 <session>.raw.wav를 함께 저장해 구간별로 비교할 수 있습니다. 이것만 켜면 원본 오디오가 기본 파일에 저장됩니다.",
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
            "插入後還原剪貼簿",
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
            "啟動時靜默執行",
            "Start minimized (no main window)",
            "起動時にメインウィンドウを表示しない",
            "시작 시 메인 창 숨기기",
        ),
    },
    Msg {
        key: "settings.recording.startup_at_boot",
        text: row(
            "开机自启",
            "開機自啟",
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
            "手機/平板瀏覽器連到電腦錄音，語音即時落到電腦遊標處（需 HTTPS，首次存取要信任憑證）",
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
            "點選切換",
            "Tap to toggle",
            "タップで切替",
            "탭하여 전환",
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
            "所有快捷鍵全域生效。若無響應，請在權限頁檢視全域快捷鍵監聽狀態。",
            "All shortcuts apply globally. If unresponsive, check the global hotkey status in Permissions.",
            "すべてのショートカットはグローバルで有効。応答がない場合は権限ページでグローバルショートカット監視の状態を確認してください。",
            "모든 단축키는 전역에서 작동. 응답이 없으면 권한 페이지에서 전역 단축키 감지 상태를 확인해 주세요.",
        ),
    },
    Msg {
        key: "settings.shortcuts.open_app",
        text: row(
            "打开 OpenLess",
            "開啟 OpenLess",
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
            "易讀佈局（防溢出換行）",
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
    Msg {
        key: "settings.providers.presets.siliconflow",
        text: row("硅基流动", "矽基流動", "SiliconFlow", "SiliconFlow", "SiliconFlow"),
    },
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
    Msg { key: "settings.providers.presets.apiRoute", text: row("API Route", "API Route", "API Route", "API Route", "API Route") },
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
            "矽基流動 SenseVoice",
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
            "OpenAI Whisper（相容）",
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
            "苹果识别（macOS）",
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
            "用於錄音轉寫後的 ASR 文字；這裡可以寫口語整理、ASR 錯字糾正和專有名詞還原規則。",
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
            "按需配置测试功能与调试工具。",
            "依需要設定測試功能與除錯工具。",
            "Configure testing features and debugging tools as needed.",
            "テスト機能とデバッグツールを設定します。",
            "테스트 기능과 디버깅 도구를 설정합니다.",
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
            "搜索、分类与自定义各功能的快捷键。",
            "搜尋、分類與自訂各功能的快捷鍵。",
            "Search, browse and customize keyboard shortcuts.",
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
            "選擇「跟隨系統」時按作業系統當前語言顯示。",
            "Choose \"Follow system\" to match the OS language at launch.",
            "「システムに従う」を選ぶと OS の言語に合わせます。",
            "\"시스템 따라가기\"를 선택하면 OS 언어를 따릅니다.",
        ),
    },
    Msg {
        key: "settings.network.use_system_proxy_desc",
        text: row(
            "开启时请求跟随系统代理；关闭后所有网络请求直连（国内服务延迟通常更低），GitHub 登录、更新等境外服务可能连不上。实时语音流不受此开关影响。",
            "開啟時請求跟隨系統代理；關閉後所有網路請求直連（國內服務延遲通常更低），GitHub 登入、更新等境外服務可能連不上。即時語音串流不受此開關影響。",
            "When on, requests follow the system proxy. When off, all requests connect directly (usually lower latency for domestic services), but overseas services such as GitHub sign-in and updates may fail. Realtime voice streams are unaffected.",
            "オンにするとリクエストはシステムプロキシを経由します。オフにするとすべて直接接続します（国内サービスの遅延が低くなる傾向）。GitHub ログインやアップデートなど海外サービスには接続できない場合があります。リアルタイム音声ストリームは影響を受けません。",
            "켜면 요청이 시스템 프록시를 따릅니다. 끄면 모든 요청이 직결됩니다(국내 서비스는 보통 더 빠름). GitHub 로그인·업데이트 등 해외 서비스는 연결되지 않을 수 있습니다. 실시간 음성 스트림은 영향을 받지 않습니다.",
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
            "核心快捷鍵不可停用，錄音必須繫結一個快捷鍵",
            "Core hotkey cannot be disabled — recording needs a hotkey",
            "コアショートカットは無効化できません（録音にはショートカットが必須です）",
            "핵심 단축키는 비활성화할 수 없습니다 (녹음에는 단축키가 필수입니다)",
        ),
    },
    Msg {
        key: "settings.recording.microphone_desc",
        text: row(
            "选择优先输入设备。设备断开时自动切到系统默认。",
            "選擇優先使用的輸入裝置。裝置暫時不可用時會使用系統預設麥克風，重新連線後自動切回首選裝置。",
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
            "錄音期間臨時靜音系統輸出，避免揚聲器迴音。",
            "Temporarily mute system output during voice input to avoid speaker echo.",
            "録音中にシステム出力を一時的にミュートし、スピーカーのエコーを防ぎます。",
            "녹음 중 시스템 출력을 일시적으로 음소거하여 스피커 에코를 방지합니다.",
        ),
    },
    Msg {
        key: "settings.recording.audio_enhancement_desc",
        text: row(
            "应用 RNNoise 或 PureVox 降噪、自动增益和限幅。默认关闭，设置在下一次录音生效。",
            "套用 RNNoise 或 PureVox 降噪、自動增益和限幅。預設關閉，設定於下一次錄音生效。",
            "Apply RNNoise or PureVox noise reduction, automatic gain and limiting. Off by default; changes apply to the next recording.",
            "RNNoise または PureVox のノイズ抑制、自動ゲイン、リミッターを適用します。既定ではオフ。変更は次の録音から有効です。",
            "RNNoise 또는 PureVox 소음 억제, 자동 게인 및 리미터를 적용합니다. 기본값은 꺼짐이며 다음 녹음부터 적용됩니다.",
        ),
    },
    Msg {
        key: "settings.recording.echo_cancellation_desc",
        text: row(
            "保持系统声音播放，使用本机播放参考抑制回声。macOS 需 14.6 或更新版本及系统音频录制权限；不支持的设备、蓝牙路径或 Linux ALSA 环境回退为普通麦克风录音。下次开始采集时生效。",
            "保持系統聲音播放，使用本機播放參考抑制回聲。macOS 需 14.6 或更新版本及系統音訊錄製權限；不支援的裝置、藍牙路徑或 Linux ALSA 環境改用一般麥克風錄音。下次開始擷取時生效。",
            "Uses a local playback reference while keeping playback audible. On macOS, requires 14.6+ and System Audio Recording permission. Unsupported devices, Bluetooth routes, or Linux ALSA use microphone-only recording. Applies to the next capture.",
            "再生音を止めずにローカルの再生音を参照してエコーを抑制します。macOS は 14.6 以降とシステム音声の収録許可が必要です。非対応デバイス、Bluetooth 経路、Linux ALSA では通常のマイク録音に戻ります。次の収録開始時に適用されます。",
            "재생을 유지하면서 로컬 재생음을 참조합니다. macOS는 14.6 이상 및 시스템 오디오 녹음 권한이 필요합니다. 지원하지 않는 장치, Bluetooth 경로 또는 Linux ALSA에서는 일반 마이크 녹음으로 전환합니다. 다음 캡처부터 적용됩니다.",
        ),
    },
    Msg {
        key: "settings.recording.paste_shortcut_desc",
        text: row(
            "插入时模拟按下的粘贴键，部分终端类应用需要 Ctrl+Shift+V（仅 Windows / Linux）。",
            "插入時模擬按下的貼上鍵，部分終端類應用需要 Ctrl+Shift+V（僅 Windows / Linux）。",
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
            "對話上下文視窗（分鐘）",
            "Polish context window (minutes)",
            "会話コンテキスト窓（分）",
            "대화 컨텍스트 윈도(분)",
        ),
    },
    Msg {
        key: "settings.recording.polish_skip_max_chars_desc",
        text: row(
            "转写不超过 N 个字（不计空白）时直接插入原文，不调用润色模型。0 = 关闭。翻译模式始终润色。",
            "轉寫不超過 N 個字（不計空白）時直接插入原文，不呼叫潤色模型。0 = 關閉。翻譯模式始終潤色。",
            "Transcripts with at most N characters (spaces not counted) are inserted as raw text without calling the LLM. 0 = off. Translation always polishes.",
            "空白を除いて N 文字以下の認識結果は、LLM を呼び出さずそのまま挿入します。0 = オフ。翻訳は常に整形します。",
            "공백을 제외하고 N자 이하의 인식 결과는 LLM 호출 없이 원문 그대로 삽입합니다. 0 = 끔. 번역은 항상 다듬습니다.",
        ),
    },
    Msg {
        key: "settings.recording.polish_skip_max_chars_label",
        text: row(
            "短句跳过润色（最大字数）",
            "短句跳過潤色（最大字數）",
            "Skip polish for short dictations (max characters)",
            "短い入力は整形をスキップ（最大文字数）",
            "짧은 입력은 다듬기 건너뛰기(최대 글자 수)",
        ),
    },
    Msg {
        key: "settings.recording.restore_clipboard_desc",
        text: row(
            "粘贴成功后恢复你原来的剪贴板内容（仅 Windows / Linux）。",
            "貼上成功後還原你原來的剪貼簿內容（僅 Windows / Linux）。",
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
            "輸入問題，Enter 傳送",
            "Type a question. Enter to send",
            "質問を入力。Enter で送信",
            "질문을 입력하세요. Enter로 보내기",
        ),
    },
    Msg {
        key: "qa.empty_desc",
        text: row(
            "选中任意文字后开始追问，或直接在下方输入问题。回答会显示在这里，可以连续多轮。",
            "選中任意文字後開始追問，或直接在下方輸入問題。回答會顯示在這裡，可以連續多輪。",
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
            "基於選中文字：",
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
        text: row("问任何问题", "問任何問題", "Ask", "質問", "질문"),
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
            "可直接编辑；点击确认后才会替换原选区。",
            "可直接編輯；點選確認後才會替換原選區。",
            "Editable; the original selection is replaced only after you confirm.",
            "編集可能です。確認後はじめて元の選択範囲を置き換えます。",
            "편집 가능합니다. 확인을 클릭한 뒤에만 원래 선택 영역을 교체합니다.",
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
        key: "onboarding.mic_no_device_hint",
        text: row(
            "未检测到麦克风，请连接并启用麦克风后重试。",
            "未偵測到麥克風，請連線並啟用麥克風後重試。",
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
            "錄音 / 轉寫時在螢幕底部顯示半透明膠囊。",
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
            "OpenLess 默认风格",
            "OpenLess 預設風格",
            "OpenLess default style",
            "OpenLess デフォルトスタイル",
            "OpenLess 기본 스타일",
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
            "已儲存模型 {}。",
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
            "選擇一個模型寫入上方欄位",
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
            "基於上面的原文再跑一次潤色。結果只在本次檢視時顯示，不寫回這條記錄。原風格包已刪除或舊記錄時，重試將使用當前風格。",
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
        key: "marketplace.myPacks.emptyTitle",
        text: row(
            "你还没有发布过风格包",
            "你還沒有發布過風格包",
            "You have not published any style packs yet",
            "まだ公開したパックはありません",
            "아직 게시한 팩이 없습니다",
        ),
    },
    Msg {
        key: "marketplace.myPacks.emptyHint",
        text: row(
            "在「风格」页面编辑后点「发布到风格市场」，或点击右上角上传本地风格包。",
            "在「風格」頁面編輯後點「發布到風格市場」，或點選右上角上傳本機風格包。",
            "Edit a pack in the Style page and click \"Publish to Marketplace\", or upload a local pack from the top-right.",
            "「スタイル」ページで編集して「マーケットに公開」をクリックするか、右上からローカルパックをアップロードしてください。",
            "\"스타일\" 페이지에서 편집 후 \"마켓에 게시\"를 누르거나, 오른쪽 위에서 로컬 팩을 업로드하세요.",
        ),
    },
    Msg {
        key: "marketplace.myPacks.noMatch",
        text: row(
            "没有匹配的风格包",
            "沒有符合的風格包",
            "No matching style packs",
            "一致するパックがありません",
            "일치하는 팩이 없습니다",
        ),
    },
    Msg {
        key: "marketplace.myPacks.loadingTitle",
        text: row("正在拉取，请稍后…", "正在拉取，請稍後…", "Loading…", "読み込み中…", "불러오는 중…"),
    },
    Msg {
        key: "marketplace.myPacks.loadingHint",
        text: row(
            "从风格市场获取你最新发布的风格包。",
            "從風格市場獲取你最新發布的風格包。",
            "Fetching your latest publications from the marketplace.",
            "マーケットからあなたの最新公開を取得しています。",
            "마켓에서 최신 게시물을 가져오는 중입니다.",
        ),
    },
    Msg {
        key: "marketplace.myPacks.loadErrorTitle",
        text: row("加载失败", "載入失敗", "Load failed", "読み込み失敗", "불러오기 실패"),
    },
    Msg {
        key: "marketplace.myPacks.loadErrorRetry",
        text: row("重试", "重試", "Retry", "再試行", "다시 시도"),
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
        key: "marketplace.modal.loggedInLabel",
        text: row("已登录", "已登入", "Signed in", "ログイン済み", "로그인됨"),
    },
    Msg {
        key: "marketplace.oauth.loginBtn",
        text: row("登录", "登入", "Sign in", "サインイン", "로그인"),
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
            "目前編輯錄音 / ASR 風格 Prompt；輸入物件是語音辨識後的轉寫文字。",
            "Editing the recording / ASR style prompt; input is ASR transcript text after dictation.",
            "録音 / ASRスタイルのプロンプトを編集中。入力は音声認識後の書き起こしテキストです。",
            "녹음 / ASR 스타일 프롬프트를 편집 중입니다. 입력은 음성 인식 후 받아쓰기 텍스트입니다.",
        ),
    },
    Msg {
        key: "style.pack.selectionPromptEditorDesc",
        text: row(
            "当前编辑选区润色 Prompt；输入对象是用户主动选中的书面文字，不经过 ASR。",
            "目前編輯選區潤色 Prompt；輸入物件是使用者主動選中的書面文字，不經過 ASR。",
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
        key: "style.pack.metaTitle",
        text: row(
            "安装信息",
            "安裝資訊",
            "Installation Info",
            "インストール情報",
            "설치 정보",
        ),
    },
    Msg {
        key: "style.pack.metaSource",
        text: row("来源", "來源", "Source", "ソース", "소스"),
    },
    Msg {
        key: "style.pack.metaBaseMode",
        text: row("基础模式", "基礎模式", "Base Mode", "ベースモード", "베이스 모드"),
    },
    Msg {
        key: "style.pack.metaUpdatedAt",
        text: row("更新时间", "更新時間", "Updated", "更新日時", "업데이트"),
    },
    Msg {
        key: "style.pack.examplesTitle",
        text: row(
            "效果示例",
            "效果範例",
            "Effect Examples",
            "効果例",
            "효과 예시",
        ),
    },
    Msg {
        key: "style.pack.examplesDesc",
        text: row(
            "会随风格包一起导出。",
            "會隨風格包一起匯出。",
            "Exported with the pack.",
            "パックと一緒にエクスポートされます。",
            "팩과 함께 내보내집니다.",
        ),
    },
    Msg {
        key: "style.pack.addExample",
        text: row("新增示例", "新增範例", "Add Example", "例を追加", "예시 추가"),
    },
    Msg {
        key: "style.pack.examplesEmpty",
        text: row(
            "还没有示例。",
            "還沒有範例。",
            "No examples yet.",
            "まだ例がありません。",
            "아직 예시가 없습니다.",
        ),
    },
    Msg {
        key: "style.pack.exampleTitlePlaceholder",
        text: row(
            "示例 {} 标题",
            "範例 {} 標題",
            "Example {} title",
            "例 {} のタイトル",
            "예시 {} 제목",
        ),
    },
    Msg {
        key: "style.pack.exampleInput",
        text: row("输入", "輸入", "Input", "入力", "입력"),
    },
    Msg {
        key: "style.pack.exampleOutput",
        text: row("输出", "輸出", "Output", "出力", "출력"),
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
            "預覽已省略前臺 app 標籤。",
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
        key: "vocab.newWordTemplateCount",
        text: row("{} 词", "{} 詞", "{} words", "{} 語", "{}개 단어"),
    },
    Msg {
        key: "style.pack.publishMarketplace",
        text: row(
            "发布到风格市场",
            "發布到風格市場",
            "Publish to Marketplace",
            "マーケットに公開",
            "마켓에 게시",
        ),
    },
    Msg {
        key: "marketplace.myPacks.actions.update",
        text: row("更新", "更新", "Update", "更新", "업데이트"),
    },
    Msg {
        key: "marketplace.myPacks.actions.withdraw",
        text: row("下架", "下架", "Withdraw", "取り下げ", "내리기"),
    },
    Msg {
        key: "marketplace.myPacks.stats",
        text: row("★ {} · ↓ {}", "★ {} · ↓ {}", "★ {} · ↓ {}", "★ {} · ↓ {}", "★ {} · ↓ {}"),
    },
    Msg {
        key: "marketplace.myPacks.versionDate",
        text: row("v{} · {}", "v{} · {}", "v{} · {}", "v{} · {}", "v{} · {}"),
    },
    Msg {
        key: "marketplace.oauth.browserHint",
        text: row(
            "在浏览器中打开 {} 并输入下方代码：",
            "在瀏覽器中開啟 {} 並輸入下方程式碼：",
            "Open {} in your browser and enter this code:",
            "ブラウザで {} を開き、このコードを入力してください：",
            "브라우저에서 {}을(를) 열고 아래 코드를 입력하세요:",
        ),
    },
    Msg {
        key: "marketplace.oauth.cancelBtn",
        text: row("取消", "取消", "Cancel", "キャンセル", "취소"),
    },
    Msg {
        key: "marketplace.oauth.copyBtn",
        text: row("复制", "複製", "Copy", "コピー", "복사"),
    },
    Msg {
        key: "marketplace.oauth.generating",
        text: row(
            "正在生成设备验证码…",
            "正在產生裝置驗證碼…",
            "Generating device code…",
            "デバイスコードを生成中…",
            "디바이스 코드 생성 중…",
        ),
    },
    Msg {
        key: "marketplace.oauth.openBrowserBtn",
        text: row("打开浏览器", "開啟瀏覽器", "Open browser", "ブラウザを開く", "브라우저 열기"),
    },
    Msg {
        key: "marketplace.oauth.retryBtn",
        text: row("重试", "重試", "Retry", "再試行", "다시 시도"),
    },
    Msg {
        key: "marketplace.oauth.title",
        text: row(
            "用 GitHub 登录",
            "用 GitHub 登入",
            "Sign in with GitHub",
            "GitHub でサインイン",
            "GitHub로 로그인",
        ),
    },
    Msg {
        key: "marketplace.oauth.waiting",
        text: row(
            "等待你在浏览器中授权…",
            "等待你在瀏覽器中授權…",
            "Waiting for browser authorization…",
            "ブラウザでの認可を待っています…",
            "브라우저에서 인증을 기다리는 중…",
        ),
    },
    Msg {
        key: "style.modes.raw.desc",
        text: row(
            "只补标点和必要分句，不改写不扩写。",
            "只補標點和必要分句，不改寫不擴寫。",
            "Only adds punctuation and natural breaks — no rewriting or expansion.",
            "句読点と必要な区切りのみ補い、書き換えや拡張はしません。",
            "구두점과 필요한 문장 구분만 보충하고 다시 쓰거나 확장하지 않습니다.",
        ),
    },
    Msg {
        key: "style.modes.light.desc",
        text: row(
            "去口癖、补标点，整理为可发送的自然文字。",
            "去口癖、補標點，整理為可傳送的自然文字。",
            "Drops fillers, adds punctuation, and produces sendable natural prose.",
            "口癖の除去、句読点の補完、自然な送信可能テキストへの整理。",
            "입버릇 제거, 구두점 보충, 자연스럽게 보낼 수 있는 텍스트로 정리합니다.",
        ),
    },
    Msg {
        key: "style.modes.structured.desc",
        text: row(
            "面向编程协作、技术排障和产品反馈，准确保留术语并梳理结构。",
            "多個主題或步驟時，自動組織為分點列表。",
            "Organize coding discussions, troubleshooting and product feedback with precise terminology.",
            "開発の相談、技術的な問題解決、製品への意見を、用語を正確に保って整理します。",
            "개발 협업, 기술 문제 해결, 제품 피드백을 정확한 용어와 명확한 구조로 정리합니다.",
        ),
    },
    Msg {
        key: "style.modes.formal.desc",
        text: row(
            "工作沟通和邮件场景，更专业更完整。",
            "工作溝通和郵件場景，更專業更完整。",
            "Email and workplace tone — more complete, more professional.",
            "業務コミュニケーションやメール用途向け。よりプロフェッショナルで完成度の高い文体。",
            "업무 커뮤니케이션과 메일에 적합. 더 전문적이고 완성도 높은 문체.",
        ),
    },
    Msg {
        key: "style.pack.builtinTags.minimalEdits",
        text: row("最小改写", "最小改寫", "Minimal edits", "最小限の修正", "최소 수정"),
    },
    Msg {
        key: "style.pack.builtinTags.strongCorrection",
        text: row("强纠错", "強糾錯", "Strong corrections", "誤認識を補正", "정확한 교정"),
    },
    Msg {
        key: "style.pack.builtinTags.communication",
        text: row("沟通", "溝通", "Communication", "コミュニケーション", "의사소통"),
    },
    Msg {
        key: "style.pack.builtinTags.natural",
        text: row("自然", "自然", "Natural", "自然な文章", "자연스러움"),
    },
    Msg {
        key: "style.pack.builtinTags.organized",
        text: row("条理", "條理", "Organized", "整理", "체계적 정리"),
    },
    Msg {
        key: "style.pack.builtinTags.workplaceCommunication",
        text: row("工作沟通", "工作溝通", "Work communication", "仕事のやり取り", "업무 소통"),
    },
    Msg {
        key: "style.pack.builtinTags.aiCoding",
        text: row("AI 编程", "AI 程式開發", "AI coding", "AIコーディング", "AI 코딩"),
    },
    Msg {
        key: "style.pack.builtinTags.technicalStructure",
        text: row("技术结构化", "技術結構化", "Technical structure", "技術内容の構造化", "기술 내용 구조화"),
    },
    Msg {
        key: "style.pack.writtenPolish",
        text: row("书面润色", "書面潤色", "Written polish", "書面の推敲", "서면 다듬기"),
    },

    Msg {
        key: "settings.channels.edit",
        text: row("编辑", "編輯", "Edit", "編集", "편집"),
    },
    Msg {
        key: "settings.providers.extraHeadersLabel",
        text: row(
            "额外 Headers",
            "額外 Headers",
            "Extra headers",
            "追加 Headers",
            "추가 Headers",
        ),
    },
    Msg {
        key: "settings.providers.extraHeadersPlaceholder",
        text: row(
            "{\"custom-head\":\"...\"}",
            "{\"custom-head\":\"...\"}",
            "{\"custom-head\":\"...\"}",
            "{\"custom-head\":\"...\"}",
            "{\"custom-head\":\"...\"}",
        ),
    },
    Msg {
        key: "settings.providers.temperatureLabel",
        text: row("Temperature", "Temperature", "Temperature", "Temperature", "Temperature"),
    },
    Msg {
        key: "settings.providers.temperaturePlaceholder",
        text: row(
            "留空则不发送；范围 0～2（含边界），例如 0.3",
            "留空則不傳送；範圍 0～2（含邊界），例如 0.3",
            "Leave empty to omit; range 0–2 inclusive, e.g. 0.3",
            "空欄なら送信しません。範囲は 0〜2（両端を含む）。例: 0.3",
            "비워 두면 보내지 않음. 범위 0~2(양 끝 포함), 예: 0.3",
        ),
    },
    Msg {
        key: "settings.providers.thinkingModeHint",
        text: row(
            "按所选请求格式和模型支持的参数启用、关闭或降低思考，不向提示词注入控制指令。",
            "依所選請求格式與模型支援的引數啟用、關閉或降低思考，不在提示詞注入控制指令。",
            "Enable, disable, or reduce thinking using parameters supported by the selected request format and model. No control instructions are injected into prompts.",
            "選択したリクエスト形式とモデルが対応するパラメータで思考を有効化、無効化または軽減します。プロンプトに制御指示は追加しません。",
            "선택한 요청 형식과 모델이 지원하는 매개변수로 사고를 켜거나 끄거나 줄입니다. 프롬프트에 제어 지시를 추가하지 않습니다.",
        ),
    },
    Msg {
        key: "settings.providers.thinkingModeLabel",
        text: row("思考", "思考", "Thinking", "思考", "사고"),
    },
    Msg {
        key: "common.operationFailed",
        text: row(
            "操作失败",
            "操作失敗",
            "Operation failed",
            "操作に失敗しました",
            "작업 실패",
        ),
    },
    Msg {
        key: "common.saved",
        text: row("已保存", "已儲存", "Saved", "保存しました", "저장됨"),
    },
    Msg {
        key: "modal.service_views.inactive",
        text: row(
            "当前模式未使用",
            "目前模式未使用",
            "Not used in this mode",
            "現在のモードでは未使用",
            "현재 모드에서 사용 안 함",
        ),
    },
    Msg {
        key: "settings.providers.apiKeyLabel",
        text: row("API 密钥", "API 金鑰", "API Key", "API キー", "API 키"),
    },
    Msg {
        key: "settings.providers.baseUrlLabel",
        text: row("接口地址", "介面地址", "Base URL", "エンドポイント", "엔드포인트"),
    },
    Msg {
        key: "settings.providers.omniTitle",
        text: row(
            "多模态模型",
            "多模態模型",
            "Multimodal model",
            "マルチモーダルモデル",
            "멀티모달 모델",
        ),
    },
    Msg {
        key: "settings.providers.providerLabel",
        text: row("供应商", "供應商", "Provider", "サプライヤー", "공급자"),
    },
    Msg {
        key: "settings.providers.validateFailed",
        text: row(
            "连接检查未通过。",
            "連線檢查未透過。",
            "Connection check failed.",
            "接続チェックに失敗しました。",
            "연결 확인에 실패했습니다.",
        ),
    },
    Msg {
        key: "settings.providers.validateSuccessOmni",
        text: row(
            "文本连通性通过。这不代表音频听写可用，请再做一次短录音确认。",
            "文字連通性通過。這不代表音訊聽寫可用，請再做一次短錄音確認。",
            "Text connectivity passed. This does not mean audio dictation works—please confirm with a short recording.",
            "テキスト疎通は成功しました。これは音声ディクテーションが使えることを意味しません。短い録音で確認してください。",
            "텍스트 연결은 통과했습니다. 오디오 받아쓰기가 된다는 뜻은 아닙니다. 짧은 녹음으로 확인하세요.",
        ),
    },
    Msg {
        key: "settings.providers.tencentCloudAppIdLabel",
        text: row(
            "腾讯云 AppID",
            "騰訊雲 AppID",
            "Tencent Cloud AppID",
            "Tencent Cloud AppID",
            "Tencent Cloud AppID",
        ),
    },
    Msg {
        key: "settings.providers.tencentCloudNote",
        text: row(
            "使用腾讯云「语音识别」服务的 API 密钥。默认 Hy-ASR-3.0-preview 支持中英与 20 种方言；Preview 仅支持 60 秒以内的 16kHz 单声道 PCM，暂不支持上下文或热词增强。",
            "使用騰訊雲「語音辨識」服務的 API 金鑰。預設 Hy-ASR-3.0-preview 支援中英與 20 種方言；Preview 僅支援 60 秒內的 16kHz 單聲道 PCM，暫不支援上下文或熱詞增強。",
            "Uses Tencent Cloud Speech Recognition API credentials. The default Hy-ASR-3.0-preview supports Chinese, English, and 20 dialects; Preview accepts only mono 16 kHz PCM up to 60 seconds and does not yet support context or hotword boosting.",
            "Tencent Cloud 音声認識 API の認証情報を使用します。既定の Hy-ASR-3.0-preview は中国語・英語・20 方言に対応します。Preview は 60 秒以内の 16kHz モノラル PCM のみ対応し、コンテキストとホットワード強化は未対応です。",
            "Tencent Cloud 음성 인식 API 자격 증명을 사용합니다. 기본 Hy-ASR-3.0-preview는 중국어·영어·20개 방언을 지원합니다. Preview는 60초 이내의 16kHz 모노 PCM만 지원하며, 컨텍스트와 핫워드 강화는 아직 지원하지 않습니다.",
        ),
    },
    Msg {
        key: "settings.providers.tencentCloudSecretIdLabel",
        text: row("SecretID", "SecretID", "SecretID", "SecretID", "SecretID"),
    },
    Msg {
        key: "settings.providers.tencentCloudSecretKeyLabel",
        text: row("SecretKey", "SecretKey", "SecretKey", "SecretKey", "SecretKey"),
    },
    Msg {
        key: "settings.providers.volcengineAccessKeyLabel",
        text: row("Access Token", "Access Token", "Access Token", "Access Token", "Access Token"),
    },
    Msg {
        key: "settings.providers.volcengineApiKeyLabel",
        text: row("API Key", "API Key", "API Key", "API Key", "API Key"),
    },
    Msg {
        key: "settings.providers.volcengineAppKeyLabel",
        text: row("APP ID", "APP ID", "APP ID", "APP ID", "APP ID"),
    },
    Msg {
        key: "settings.providers.volcengineAuthModeLabel",
        text: row("鉴权模式", "鑑權模式", "Auth mode", "認証モード", "인증 모드"),
    },
    Msg {
        key: "settings.providers.volcengineResourceIdLabel",
        text: row("Resource ID", "Resource ID", "Resource ID", "Resource ID", "Resource ID"),
    },
    Msg {
        key: "settings.providers.xfyunApiKeyLabel",
        text: row("API Key", "API Key", "API Key", "API Key", "API Key"),
    },
    Msg {
        key: "settings.providers.xfyunAppIdLabel",
        text: row("AppID", "AppID", "AppID", "AppID", "AppID"),
    },
    Msg {
        key: "settings.permissions.checking",
        text: row("检查中…", "檢查中…", "Checking…", "確認中…", "확인 중…"),
    },
    Msg {
        key: "settings.permissions.hotkey_failed",
        text: row(
            "监听失败",
            "監聽失敗",
            "Listener failed",
            "監視失敗",
            "감지 실패",
        ),
    },
    Msg {
        key: "settings.providers.apiKeyOptionalLabel",
        text: row(
            "API 密钥（可选）",
            "API 密鑰（選填）",
            "API Key (optional)",
            "API キー（任意）",
            "API 키 (선택 사항)",
        ),
    },
    Msg {
        key: "settings.providers.volcengineServiceLabel",
        text: row("服务", "服務", "Service", "サービス", "서비스"),
    },
    Msg {
        key: "cloudSyncE2ee.account",
        text: row("同步账号", "同步帳號", "Sync account", "同期アカウント", "동기화 계정"),
    },
    Msg {
        key: "cloudSyncE2ee.backupRetention",
        text: row(
            "删除后，服务备份副本最多保留 {} 天。",
            "刪除後，服務備份副本最多保留 {} 天。",
            "After deletion, service backup copies may be retained for up to {} days.",
            "削除後も、サービスのバックアップコピーは最長 {} 日間保持される場合があります。",
            "삭제 후에도 서비스의 백업 사본이 최대 {}일 동안 보관될 수 있습니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.cancelTask",
        text: row(
            "取消当前任务",
            "取消目前工作",
            "Cancel current task",
            "現在の処理をキャンセル",
            "현재 작업 취소",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.changePassword",
        text: row(
            "更改同步密码",
            "變更同步密碼",
            "Change sync password",
            "同期パスワードを変更",
            "동기화 비밀번호 변경",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.confirmDelete",
        text: row(
            "删除云端备份",
            "刪除雲端備份",
            "Delete cloud backup",
            "クラウドバックアップを削除",
            "클라우드 백업 삭제",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.confirmDisable",
        text: row(
            "关闭同步",
            "關閉同步",
            "Turn off sync",
            "同期をオフにする",
            "동기화 끄기",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.confirmPassword",
        text: row(
            "再次输入新密码",
            "再次輸入新密碼",
            "Confirm new password",
            "新しいパスワードを再入力",
            "새 비밀번호 확인",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.consentCheck",
        text: row(
            "我已了解完整范围，并同意加密同步这些资料。",
            "我已了解完整範圍，並同意加密同步這些資料。",
            "I understand the full scope and agree to sync this data in encrypted form.",
            "対象範囲を理解し、これらのデータの暗号化同期に同意します。",
            "전체 범위를 이해했으며 이 데이터를 암호화해 동기화하는 데 동의합니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.consentDescription",
        text: row(
            "同步内容包含服务凭据和私密文本历史，资料会在本机加密后上传。请先了解完整范围。",
            "同步內容包含服務憑證和私人文字歷史，資料會在本機加密後上傳。請先了解完整範圍。",
            "This sync includes service credentials and private text history. Data is encrypted on your device before upload. Review the full scope first.",
            "サービスの認証情報と非公開のテキスト履歴も同期します。アップロード前に端末で暗号化します。対象範囲を確認してください。",
            "서비스 자격 증명과 비공개 텍스트 기록도 동기화됩니다. 업로드 전에 기기에서 암호화합니다. 전체 범위를 확인하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.consentTitle",
        text: row(
            "启用端到端加密同步",
            "啟用端對端加密同步",
            "Enable end-to-end encrypted sync",
            "エンドツーエンド暗号化同期を有効にする",
            "종단 간 암호화 동기화 사용",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.continue",
        text: row("继续", "繼續", "Continue", "続ける", "계속"),
    },
    Msg {
        key: "cloudSyncE2ee.currentPassword",
        text: row(
            "当前同步密码",
            "目前同步密碼",
            "Current sync password",
            "現在の同期パスワード",
            "현재 동기화 비밀번호",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.customServerHint",
        text: row(
            "留空则使用官方同步服务器。地址必须是 https:// 开头的根地址。",
            "留空則使用官方同步伺服器。位址必須以 https:// 開頭的根位址。",
            "Leave blank to use the official sync server. The address must start with https:// and be a root URL.",
            "空欄の場合は公式の同期サーバーを使用します。アドレスは https:// で始まるルート URL である必要があります。",
            "비워두면 공식 동기화 서버를 사용합니다. 주소는 https://로 시작하는 루트 URL이어야 합니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.customServerSave",
        text: row("保存", "儲存", "Save", "保存", "저장"),
    },
    Msg {
        key: "cloudSyncE2ee.customServerTitle",
        text: row(
            "自建同步服务器（高级）",
            "自建同步伺服器（進階）",
            "Self-hosted sync server (advanced)",
            "自前の同期サーバー（上級者向け）",
            "자체 동기화 서버 (고급)",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.customServerToken",
        text: row(
            "登录 Token",
            "登入 Token",
            "Sign-in token",
            "サインイン Token",
            "로그인 Token",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.delete",
        text: row(
            "删除云端备份",
            "刪除雲端備份",
            "Delete cloud backup",
            "クラウドバックアップを削除",
            "클라우드 백업 삭제",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.deleteDescription",
        text: row(
            "仅删除当前账号的云端备份，本机资料保留。以后在其他设备恢复前，需要重新创建备份。",
            "僅刪除目前帳號的雲端備份，本機資料保留。之後在其他裝置還原前，需要重新建立備份。",
            "Only this account’s cloud backup is deleted. Local data is kept. You will need a new backup to restore on another device.",
            "このアカウントのクラウドバックアップのみ削除します。ローカルデータは残ります。別の端末で復元するには新しいバックアップが必要です。",
            "이 계정의 클라우드 백업만 삭제합니다. 로컬 데이터는 유지됩니다. 다른 기기에서 복원하려면 새 백업이 필요합니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.deleteTitle",
        text: row(
            "删除云端加密备份？",
            "刪除雲端加密備份？",
            "Delete the encrypted cloud backup?",
            "暗号化クラウドバックアップを削除しますか？",
            "암호화된 클라우드 백업을 삭제할까요?",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.description",
        text: row(
            "在本机加密后，通过 GitHub 账号或 Token 跨设备同步。",
            "在本機加密後，透過 GitHub 帳號或 Token 跨裝置同步。",
            "Encrypt on this device before syncing with your GitHub account.",
            "この端末で暗号化してから、GitHub アカウントで端末間を同期します。",
            "이 기기에서 암호화한 후 GitHub 계정으로 기기 간에 동기화합니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.disableDescription",
        text: row(
            "关闭后停止后续同步，云端备份和本机资料都会保留。删除云端备份是另一项独立操作。",
            "關閉後停止後續同步，雲端備份和本機資料都會保留。刪除雲端備份是另一項獨立操作。",
            "This stops further sync. Your cloud backup and local data are kept. Deleting the cloud backup is a separate action.",
            "以後の同期を停止します。クラウドのバックアップとローカルのデータは残ります。削除は別の操作です。",
            "이후 동기화를 중지합니다. 클라우드 백업과 로컬 데이터는 유지됩니다. 백업 삭제는 별도 작업입니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.disableTitle",
        text: row(
            "关闭加密同步？",
            "關閉加密同步？",
            "Turn off encrypted sync?",
            "暗号化同期をオフにしますか？",
            "암호화 동기화를 끌까요?",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.enable",
        text: row(
            "启用加密同步",
            "啟用加密同步",
            "Enable encrypted sync",
            "暗号化同期を有効にする",
            "암호화 동기화 사용",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.hasSnapshot",
        text: row(
            "云端已有加密备份。",
            "雲端已有加密備份。",
            "An encrypted cloud backup is available.",
            "暗号化されたクラウドバックアップがあります。",
            "암호화된 클라우드 백업이 있습니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.keyLocked",
        text: row(
            "此设备尚未解锁",
            "此裝置尚未解鎖",
            "Locked on this device",
            "この端末ではロック中",
            "이 기기에서 잠김",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.keyUnlocked",
        text: row(
            "此设备已解锁",
            "此裝置已解鎖",
            "Unlocked on this device",
            "この端末でロック解除済み",
            "이 기기에서 잠금 해제됨",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.loading",
        text: row(
            "正在读取同步状态…",
            "正在讀取同步狀態…",
            "Checking sync status…",
            "同期状態を確認中…",
            "동기화 상태 확인 중…",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.lock",
        text: row(
            "锁定此设备",
            "鎖定此裝置",
            "Lock this device",
            "この端末をロック",
            "이 기기 잠그기",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.noSnapshot",
        text: row(
            "云端尚无加密备份。",
            "雲端尚無加密備份。",
            "No encrypted cloud backup yet.",
            "暗号化クラウドバックアップはまだありません。",
            "아직 암호화된 클라우드 백업이 없습니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.passwordTitle",
        text: row(
            "更改同步密码",
            "變更同步密碼",
            "Change your sync password",
            "同期パスワードを変更",
            "동기화 비밀번호 변경",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.passwordWarning",
        text: row(
            "请妥善保管同步密码。密码无法找回，遗失后可能无法读取云端资料。",
            "請妥善保管同步密碼。密碼無法找回，遺失後可能無法讀取雲端資料。",
            "Keep your sync password safe. It cannot be recovered; losing it can make cloud data unreadable.",
            "同期パスワードは安全に保管してください。再発行できず、紛失するとクラウドのデータを読めなくなる場合があります。",
            "동기화 비밀번호를 안전하게 보관하세요. 복구할 수 없으며, 분실하면 클라우드 데이터를 읽지 못할 수 있습니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.protocolTitle",
        text: row(
            "云同步协议与隐私提醒",
            "雲端同步協議與隱私提醒",
            "Cloud sync agreement and privacy notice",
            "クラウド同期の同意事項とプライバシー",
            "클라우드 동기화 약관 및 개인정보 안내",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.refresh",
        text: row(
            "刷新状态",
            "重新整理狀態",
            "Refresh status",
            "状態を更新",
            "상태 새로고침",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.rememberKey",
        text: row(
            "在此设备的安全存储中记住解锁密钥",
            "在此裝置的安全儲存空間記住解鎖金鑰",
            "Remember the unlock key in this device’s secure storage",
            "この端末の安全なストレージに解除キーを記憶する",
            "이 기기의 보안 저장소에 잠금 해제 키 기억",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.restore",
        text: row(
            "审阅云端恢复",
            "檢閱雲端還原",
            "Review cloud restore",
            "クラウドからの復元を確認",
            "클라우드 복원 검토",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.restoreDescription",
        text: row(
            "请检查下列资料类别，敏感值不会展示。确认之前不会恢复任何内容。",
            "請檢查下列資料類別，敏感值不會顯示。確認之前不會還原任何內容。",
            "Review the categories below. Sensitive values stay hidden. Nothing is restored until you confirm.",
            "データの種類を確認してください。機密の値は表示しません。確認するまで復元しません。",
            "데이터 종류를 확인하세요. 민감한 값은 표시하지 않습니다. 확인 전에는 복원하지 않습니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.restoreTitle",
        text: row(
            "恢复前审阅",
            "還原前檢閱",
            "Review before restoring",
            "復元前に確認",
            "복원 전 검토",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.signIn",
        text: row(
            "使用 GitHub 登录",
            "使用 GitHub 登入",
            "Sign in with GitHub",
            "GitHub でログイン",
            "GitHub로 로그인",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.signOut",
        text: row("退出登录", "登出", "Sign out", "ログアウト", "로그아웃"),
    },
    Msg {
        key: "cloudSyncE2ee.snapshotUnknown",
        text: row(
            "尚未检查云端备份。",
            "尚未檢查雲端備份。",
            "Cloud backup has not been checked yet.",
            "クラウドバックアップは未確認です。",
            "클라우드 백업을 아직 확인하지 않았습니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.title",
        text: row(
            "加密云同步",
            "加密雲端同步",
            "Encrypted cloud sync",
            "暗号化クラウド同期",
            "암호화 클라우드 동기화",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.unlock",
        text: row("解锁", "解鎖", "Unlock", "ロック解除", "잠금 해제"),
    },
    Msg {
        key: "cloudSyncE2ee.working",
        text: row(
            "正在处理…",
            "正在處理…",
            "Working securely…",
            "処理中…",
            "처리 중…",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.conflictProgress",
        text: row(
            "已处理 {} / {} 项冲突",
            "已處理 {} / {} 項衝突",
            "{} of {} conflicts resolved",
            "競合を {} / {} 件解決済み",
            "충돌 {} / {}개 해결",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.deviceReview",
        text: row(
            "恢复后请核对 {} 项设备配置，包括快捷键、模型目录、麦克风及系统授权。",
            "還原後請核對 {} 項裝置設定，包括快捷鍵、模型目錄、麥克風及系統授權。",
            "After restoring, review {} device settings, including shortcuts, model paths, microphones and system permissions.",
            "復元後は、ショートカット、モデルの場所、マイク、システム権限など {} 件の端末設定を確認してください。",
            "복원 후 단축키, 모델 경로, 마이크, 시스템 권한 등 기기 설정 {}개를 확인하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.lastSync",
        text: row(
            "上次同步：{}",
            "上次同步：{}",
            "Last synced {}",
            "最終同期：{}",
            "마지막 동기화: {}",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.merge",
        text: row(
            "合并，并逐项处理冲突",
            "合併，並逐項處理衝突",
            "Merge and resolve conflicts",
            "マージして競合を解決",
            "병합하고 충돌 해결",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.passwordPolicy",
        text: row(
            "使用 12–128 个字符，包含大写字母、小写字母和数字，避免常见弱密码。",
            "使用 12–128 個字元，包含大寫字母、小寫字母和數字，避免常見弱密碼。",
            "Use 12–128 characters with uppercase and lowercase letters and a number. Avoid common passwords.",
            "12～128 文字で英大文字・英小文字・数字を含めてください。よくあるパスワードは避けてください。",
            "12~128자로 영문 대문자, 소문자, 숫자를 포함하세요. 흔한 비밀번호는 피하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.replace",
        text: row(
            "使用云端资料替换本机同步资料",
            "使用雲端資料取代本機同步資料",
            "Replace synced local data with cloud data",
            "同期対象のローカルデータをクラウドの内容で置換",
            "동기화 대상 로컬 데이터를 클라우드 데이터로 교체",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.restoreCheck",
        text: row(
            "我已检查恢复方式和冲突处理选择。",
            "我已檢查還原方式和衝突處理選擇。",
            "I have reviewed the restore mode and conflict choices.",
            "復元方法と競合の選択を確認しました。",
            "복원 방식과 충돌 선택을 확인했습니다.",
        ),
    },
    Msg {
        key: "uiRefinement.family",
        text: row("字体系列", "字型系列", "Font family", "フォントファミリー", "글꼴 모음"),
    },
    Msg {
        key: "uiRefinement.fontHint",
        text: row(
            "仅使用已安装字体，缺失时回退到系统字体。不下载、不扫描，也不发起字体网络请求。",
            "僅使用已安裝的字型，找不到時改用系統字型。不下載、不掃描，也不發出字型網路請求。",
            "Only installed fonts are used, with system fallbacks. No font downloads, scanning or network requests.",
            "インストール済みのフォントのみを使用し、見つからない場合はシステムフォントに切り替えます。フォントのダウンロード、スキャン、ネットワーク通信は行いません。",
            "설치된 글꼴만 사용하며, 없으면 시스템 글꼴을 사용합니다. 글꼴 다운로드, 스캔 또는 네트워크 요청은 하지 않습니다.",
        ),
    },
    Msg {
        key: "uiRefinement.fontSize",
        text: row(
            "界面大小",
            "介面大小",
            "Interface size",
            "インターフェースのサイズ",
            "인터페이스 크기",
        ),
    },
    Msg {
        key: "uiRefinement.storageError",
        text: row(
            "本地保存失败，本次会话仍使用当前选择。",
            "無法儲存至本機，本次工作階段仍會使用目前的選擇。",
            "Could not save locally. Your current session still uses this choice.",
            "このデバイスに保存できませんでした。現在のセッションではこの設定を引き続き使用します。",
            "이 기기에 저장하지 못했습니다. 현재 세션에서는 선택한 설정이 계속 적용됩니다.",
        ),
    },
    Msg {
        key: "uiRefinement.typography",
        text: row("字体与排版", "字型與排版", "Typography", "フォントと組版", "글꼴 및 조판"),
    },
    Msg {
        key: "uiRefinement.width",
        text: row(
            "阅读宽度",
            "閱讀寬度",
            "Reading width",
            "テキストの表示幅",
            "읽기 영역 너비",
        ),
    },
    Msg {
        key: "uiRefinement.back",
        text: row("返回应用", "返回應用程式", "Back to app", "アプリに戻る", "앱으로 돌아가기"),
    },
    Msg {
        key: "uiRefinement.noResults",
        text: row(
            "没有匹配的命令",
            "沒有符合的命令",
            "No matching commands",
            "一致するコマンドがありません",
            "일치하는 명령이 없습니다",
        ),
    },
    Msg {
        key: "uiRefinement.shortcutHint",
        text: row(
            "点击按键即可重新录制。Esc 取消；保存前会检查快捷键冲突。",
            "點選按鍵即可重新錄製。Esc 取消；儲存前會檢查快速鍵衝突。",
            "Select the keys to record a shortcut. Escape cancels; conflicts are checked before saving.",
            "キーを選択してショートカットを記録します。Esc でキャンセルします。保存前に競合を確認します。",
            "키를 선택해 단축키를 기록하세요. Esc 키로 취소하며, 저장 전에 충돌을 확인합니다.",
        ),
    },
    Msg {
        key: "uiRefinement.shortcutsSearch",
        text: row(
            "搜索命令或按键",
            "搜尋命令或按鍵",
            "Search commands or keys",
            "コマンドまたはキーを検索",
            "명령 또는 키 검색",
        ),
    },
    Msg {
        key: "uiRefinement.unassigned",
        text: row("未分配", "未指派", "Unassigned", "未割り当て", "미지정"),
    },
    Msg {
        key: "uiRefinement.all",
        text: row("全部", "全部", "All", "すべて", "전체"),
    },
    Msg {
        key: "uiRefinement.assigned",
        text: row("已分配", "已指派", "Assigned", "割り当て済み", "지정됨"),
    },
    Msg {
        key: "uiRefinement.category",
        text: row("分类", "分類", "Category", "カテゴリ", "분류"),
    },
    Msg {
        key: "uiRefinement.recording",
        text: row("语音与文本", "語音與文字", "Voice & text", "音声とテキスト", "음성 및 텍스트"),
    },
    Msg {
        key: "uiRefinement.status",
        text: row("分配状态", "指派狀態", "Assignment", "割り当て状況", "지정 상태"),
    },
    Msg {
        key: "uiRefinement.styles",
        text: row("风格", "風格", "Styles", "スタイル", "스타일"),
    },
    Msg {
        key: "uiRefinement.system",
        text: row("系统", "系統", "System", "システム", "시스템"),
    },
    Msg {
        key: "uiRefinement.comfortable",
        text: row("舒适", "舒適", "Comfortable", "ゆったり", "편안하게"),
    },
    Msg {
        key: "uiRefinement.narrow",
        text: row("聚焦", "聚焦", "Focused", "集中表示", "집중"),
    },
    Msg {
        key: "uiRefinement.native",
        text: row("系统默认", "系統預設", "System", "システム既定", "시스템 기본값"),
    },
    Msg {
        key: "uiRefinement.wide",
        text: row("宽阔", "寬闊", "Wide", "広め", "넓게"),
    },
    Msg {
        key: "settings.recording.auto_update_check_desc",
        text: row(
            "启动时及每 60 分钟自动检查更新。",
            "啟動時及每 60 分鐘自動檢查更新。",
            "Check for updates on launch and every 60 minutes.",
            "起動時および 60 分ごとに自動チェック。",
            "시작 시 및 60 분마다 자동 확인.",
        ),
    },
    Msg {
        key: "settings.recording.start_minimized_desc",
        text: row(
            "所有启动路径都不弹主窗口，仅菜单栏 / 托盘运行。",
            "所有啟動路徑都不彈主視窗，僅選單欄 / 託盤執行。",
            "No main window on any launch path — menu bar / tray only.",
            "どの起動経路でもメインウィンドウを開かず、メニューバー / トレイのみで動作。",
            "모든 시작 경로에서 메인 창을 열지 않고 메뉴 막대 / 트레이에서만 실행합니다.",
        ),
    },
    Msg {
        key: "settings.recording.startup_at_boot_desc",
        text: row(
            "登录系统时自动启动 OpenLess。",
            "登入系統時自動啟動 OpenLess。",
            "Start OpenLess automatically when you sign in.",
            "ログイン時に OpenLess を自動起動。",
            "로그인 시 OpenLess 자동 시작.",
        ),
    },
    Msg {
        key: "settings.selection_workspace.auto_intent",
        text: row(
            "自动判断意图",
            "自動判斷意圖",
            "Auto-classify intent",
            "意図を自動判定",
            "의도 자동 판별",
        ),
    },
    Msg {
        key: "settings.selection_workspace.auto_intent_desc",
        text: row(
            "开启后默认用服务配置的模型判断问句 vs 编辑；模型不可用或解析失败时回退到问句启发式。",
            "開啟後預設用服務配置的模型判斷問句 vs 編輯；模型不可用或解析失敗時回退到問句啟發式。",
            "When on, the configured model classifies question vs edit by default; falls back to question-word heuristics if the model fails.",
            "オン時は設定モデルが質問/編集を判定。モデル失敗時のみ？/疑問語ヒューリスティックにフォールバック。",
            "켜면 설정된 모델이 질문/편집을 판별합니다. 모델 실패 시에만 의문사 휴리스틱으로 폴백합니다.",
        ),
    },
    Msg {
        key: "settings.selection_workspace.edit_keywords",
        text: row(
            "额外问句线索",
            "額外問句線索",
            "Extra question cues",
            "追加の疑問手がかり",
            "추가 의문 단서",
        ),
    },
    Msg {
        key: "settings.selection_workspace.edit_keywords_desc",
        text: row(
            "关闭自动判断时生效；每行一个，指令中包含则视为提问，否则仍按问句启发式（？/吗/什么…）判定。",
            "關閉自動判斷時生效；每行一個，指令含則視為提問，否則仍按問句啟發式判定。",
            "Only when auto-classify is off; one cue per line forces Ask; otherwise use ? / question-word heuristics.",
            "自動判定オフ時のみ。1行1語で質問扱い。なければ？/疑問語ヒューリスティック。",
            "자동 판별 끔일 때만. 한 줄에 하나면 질문. 없으면 ?/의문사 휴리스틱.",
        ),
    },
    Msg {
        key: "settings.selection_workspace.edit_plan_format",
        text: row(
            "编辑方案格式",
            "編輯方案格式",
            "Edit plan format",
            "編集プラン形式",
            "편집 계획 형식",
        ),
    },
    Msg {
        key: "settings.selection_workspace.edit_plan_format_desc",
        text: row(
            "模型优先按所选格式输出 EditPlan；解析失败时再尝试另一种格式。",
            "模型優先按所選格式輸出 EditPlan；解析失敗時再嘗試另一種格式。",
            "Prefer this EditPlan format from the model; if parsing fails, try the other format.",
            "モデルはこの形式の EditPlan を優先出力。解析失敗時はもう一方を試します。",
            "모델이 선택한 형식으로 EditPlan을 우선 출력합니다. 파싱 실패 시 다른 형식을 시도합니다.",
        ),
    },
    Msg {
        key: "settings.selection_workspace.edit_plan_format_json",
        text: row("JSON", "JSON", "JSON", "JSON", "JSON"),
    },
    Msg {
        key: "settings.selection_workspace.edit_plan_format_xml",
        text: row("XML", "XML", "XML", "XML", "XML"),
    },
    Msg {
        key: "settings.selection_workspace.edit_system_prompt",
        text: row(
            "编辑方案系统提示词",
            "編輯方案系統提示詞",
            "Edit plan system prompt",
            "編集プランのシステムプロンプト",
            "편집 계획 시스템 프롬프트",
        ),
    },
    Msg {
        key: "settings.selection_workspace.edit_system_prompt_desc",
        text: row(
            "覆盖风格包 / 内置默认的 EditPlan system prompt。留空则按「自定义 → 风格包 → 内置默认」回退。",
            "覆蓋風格包 / 內建預設的 EditPlan system prompt。留空則按「自訂 → 風格包 → 內建預設」回退。",
            "Overrides the style-pack / built-in EditPlan system prompt. Leave empty to fall back: custom → pack → built-in.",
            "スタイルパック / 内蔵デフォルトの EditPlan システムプロンプトを上書き。空なら カスタム → パック → 内蔵 の順でフォールバック。",
            "스타일 팩 / 내장 기본 EditPlan 시스템 프롬프트를 덮어씁니다. 비우면 사용자 지정 → 팩 → 내장 순으로 폴백합니다.",
        ),
    },
    Msg {
        key: "settings.selection_workspace.edit_system_prompt_reset",
        text: row(
            "恢复默认",
            "恢復預設",
            "Reset to default",
            "デフォルトに戻す",
            "기본값으로 재설정",
        ),
    },
    Msg {
        key: "settings.selection_workspace.voice_delivery_desc",
        text: row(
            "语音编辑完成后：直接替换选区，或在华词面板中预览后再确认。",
            "語音編輯完成後：直接替換選區，或在華詞面板中預覽後再確認。",
            "After voice edit: replace selection directly, or preview in Ask panel then confirm.",
            "音声編集後：選択範囲を直接置換するか、Ask パネルで確認してから置換します。",
            "음성 편집 후: 선택 영역을 바로 교체하거나 Ask 패널에서 확인 후 교체합니다.",
        ),
    },
    Msg {
        key: "settings.selection_workspace.voice_enable_desc",
        text: row(
            "与上方同一快捷键；录音方式跟随全局设置（当前：{}）。",
            "與上方同一快捷鍵；錄音方式跟隨全域設定（目前：{}）。",
            "Uses the same shortcut above; recording follows global settings (current: {}).",
            "上と同じショートカットを使用。録音方式はグローバル設定に従います（現在：{}）。",
            "위와 같은 단축키 사용. 녹음 방식은 전역 설정을 따릅니다(현재: {}).",
        ),
    },
    Msg {
        key: "settings.selection_workspace.edit_system_prompt_placeholder",
        text: row(
            "留空 = 使用风格包或内置默认",
            "留空 = 使用風格包或內建預設",
            "Empty = use style pack or built-in default",
            "空 = スタイルパックまたは内蔵デフォルト",
            "비움 = 스타일 팩 또는 내장 기본값",
        ),
    },
    // 加密云同步的错误文案。键名与 Tauri `cloudSyncE2ee.errors.*` 对齐；Core 里
    // Linux 侧独有的代码（conflict / denied / expired / locked / server /
    // unsupported / service_unavailable…）在这里补齐，避免把裸 key 画到界面上。
    Msg {
        key: "cloudSyncE2ee.errors.accountChanged",
        text: row(
            "GitHub 账号已改变，请刷新后再继续。",
            "GitHub 帳號已變更，請重新整理後再繼續。",
            "The GitHub account changed. Refresh before continuing.",
            "GitHub アカウントが変わりました。更新してから続けてください。",
            "GitHub 계정이 변경되었습니다. 새로고침한 후 계속하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.busy",
        text: row(
            "已有同步任务在运行，请等待或先取消。",
            "已有同步工作在執行，請等待或先取消。",
            "Another sync task is running. Wait or cancel it first.",
            "別の同期処理が実行中です。待つか先にキャンセルしてください。",
            "다른 동기화 작업이 실행 중입니다. 기다리거나 먼저 취소하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.cancelled",
        text: row(
            "任务已取消。",
            "工作已取消。",
            "The task was cancelled.",
            "処理をキャンセルしました。",
            "작업이 취소되었습니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.changed",
        text: row(
            "审阅期间资料已改变。请关闭弹窗、刷新状态，并重新获取预览。",
            "檢閱期間資料已變更。請關閉對話框、重新整理狀態，並重新取得預覽。",
            "Data changed during review. Close this dialog, refresh, and review a new preview.",
            "確認中にデータが変わりました。閉じて状態を更新し、再度プレビューを取得してください。",
            "검토 중 데이터가 변경되었습니다. 창을 닫고 새로고침한 후 미리보기를 다시 확인하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.choicesRequired",
        text: row(
            "请为每项冲突选择本机或云端版本。",
            "請為每項衝突選擇本機或雲端版本。",
            "Choose local or cloud for every conflict.",
            "すべての競合でローカルかクラウドを選択してください。",
            "각 충돌에 대해 로컬 또는 클라우드를 선택하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.invalidData",
        text: row(
            "加密备份未通过校验，本机资料未被替换。",
            "加密備份未通過驗證，本機資料未被取代。",
            "The encrypted backup could not be verified. Local data was not replaced.",
            "暗号化バックアップを検証できませんでした。ローカルデータは置き換えていません。",
            "암호화 백업을 검증하지 못했습니다. 로컬 데이터는 교체되지 않았습니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.invalidPassword",
        text: row(
            "密码无法解锁这份备份，或加密资料校验未通过。",
            "密碼無法解鎖這份備份，或加密資料驗證未通過。",
            "The password could not unlock this backup, or the encrypted data could not be verified.",
            "このパスワードで解除できないか、暗号化データを検証できませんでした。",
            "비밀번호로 백업을 잠금 해제할 수 없거나 암호화 데이터를 검증하지 못했습니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.localData",
        text: row(
            "这台设备还没准备好本机资料。点「刷新状态」，然后再试一次。",
            "這台裝置還沒準備好本機資料。點「重新整理狀態」，然後再試一次。",
            "This device could not prepare its data. Choose Refresh status, then try again.",
            "このデバイスのデータを準備できませんでした。「状態を更新」を押して、もう一度試してください。",
            "이 기기의 데이터를 준비하지 못했습니다. 상태 새로고침을 누른 뒤 다시 시도하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.network",
        text: row(
            "暂时无法连接同步服务，请检查网络后重试。",
            "暫時無法連線同步服務，請檢查網路後重試。",
            "Could not reach the sync service. Check your connection and try again.",
            "同期サービスに接続できません。ネットワークを確認して再試行してください。",
            "동기화 서비스에 연결할 수 없습니다. 네트워크를 확인한 후 다시 시도하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.outcomeUnknown",
        text: row(
            "云端结果尚未确认。请先核对待确认结果，不要重复创建备份。",
            "雲端結果尚未確認。請先核對待確認結果，不要重複建立備份。",
            "The cloud result is not confirmed. Check the pending result before creating another backup.",
            "クラウドの結果が未確定です。新しいバックアップを作る前に未確定の結果を確認してください。",
            "클라우드 결과가 확인되지 않았습니다. 새 백업을 만들기 전에 대기 중인 결과를 확인하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.passwordMismatch",
        text: row(
            "两次输入的密码不一致。",
            "兩次輸入的密碼不一致。",
            "The two passwords do not match.",
            "パスワードが一致しません。",
            "두 비밀번호가 일치하지 않습니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.rateLimited",
        text: row(
            "请求过于频繁，请稍后重试。",
            "請求過於頻繁，請稍後重試。",
            "Too many requests. Wait before retrying.",
            "リクエストが多すぎます。しばらく待ってから再試行してください。",
            "요청이 너무 많습니다. 잠시 후 다시 시도하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.recovery",
        text: row(
            "本机同步状态需要恢复。请保留本机资料，不要覆盖云端备份。",
            "本機同步狀態需要復原。請保留本機資料，不要覆寫雲端備份。",
            "Local recovery is needed. Keep this device’s data and do not overwrite the cloud backup.",
            "ローカル状態の復旧が必要です。この端末のデータを保持し、クラウドを上書きしないでください。",
            "로컬 상태 복구가 필요합니다. 이 기기의 데이터를 보관하고 클라우드 백업을 덮어쓰지 마세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.reviewRequired",
        text: row(
            "请先确认完整同步范围，并审阅云端恢复预览。",
            "請先確認完整同步範圍，並檢閱雲端還原預覽。",
            "Review the full sync scope and the cloud restore preview before continuing.",
            "続ける前に同期対象と復元プレビューを確認してください。",
            "계속하기 전에 동기화 범위와 복원 미리보기를 확인하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.rolledBack",
        text: row(
            "恢复失败，本机更改已回滚。请刷新状态后重试。",
            "還原失敗，本機變更已回復。請重新整理狀態後重試。",
            "Restore failed and local changes were rolled back. Refresh before trying again.",
            "復元に失敗し、ローカルの変更を元に戻しました。更新してから再試行してください。",
            "복원에 실패해 로컬 변경을 되돌렸습니다. 새로고침한 후 다시 시도하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.secureStorage",
        text: row(
            "安全存储拒绝访问。请检查设备授权，或取消记住密钥后解锁。",
            "安全儲存空間拒絕存取。請檢查裝置授權，或取消記住金鑰後解鎖。",
            "Secure storage denied access. Check device permissions, or unlock without remembering the key.",
            "安全なストレージへのアクセスが拒否されました。権限を確認するか、キーを記憶せずに解除してください。",
            "보안 저장소 접근이 거부되었습니다. 권한을 확인하거나 키를 기억하지 않고 잠금 해제하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.signIn",
        text: row(
            "请重新登录 GitHub。",
            "請重新登入 GitHub。",
            "Sign in to GitHub again.",
            "GitHub に再ログインしてください。",
            "GitHub에 다시 로그인하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.tooLarge",
        text: row(
            "加密备份超出服务的大小限制。",
            "加密備份超出服務的大小限制。",
            "The encrypted backup exceeds the service’s size limit.",
            "暗号化バックアップがサービスのサイズ上限を超えています。",
            "암호화 백업이 서비스 크기 제한을 초과합니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.unavailable",
        text: row(
            "加密同步暂不可用，请使用原生客户端并稍后检查服务。",
            "加密同步暫不可用，請使用原生用戶端並稍後檢查服務。",
            "Encrypted sync is unavailable here. Use the native app and check the service later.",
            "暗号化同期は利用できません。ネイティブアプリで後ほどサービスを確認してください。",
            "암호화 동기화를 사용할 수 없습니다. 네이티브 앱에서 나중에 서비스를 확인하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.unknown",
        text: row(
            "操作未能完成，请刷新状态后重试。",
            "操作未能完成，請重新整理狀態後重試。",
            "The operation could not be completed. Refresh the status and try again.",
            "操作を完了できませんでした。状態を更新して再試行してください。",
            "작업을 완료하지 못했습니다. 상태를 새로고침한 후 다시 시도하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.unlock",
        text: row(
            "请先使用同步密码解锁此设备。",
            "請先使用同步密碼解鎖此裝置。",
            "Unlock this device with your sync password first.",
            "同期パスワードでこの端末を解除してください。",
            "먼저 동기화 비밀번호로 이 기기를 잠금 해제하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.weakPassword",
        text: row(
            "请使用满足长度和字符要求、且不常见的强密码。",
            "請使用符合長度和字元要求、且不常見的強密碼。",
            "Choose a stronger password meeting the length and character requirements.",
            "長さと文字種の条件を満たす、推測されにくいパスワードにしてください。",
            "길이와 문자 조건을 충족하는 더 강력한 비밀번호를 사용하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.conflict",
        text: row(
            "本地与云端的数据版本冲突，请刷新状态后再试。",
            "本機與雲端的資料版本衝突，請重新整理狀態後再試。",
            "Local and cloud data versions conflict — refresh the status and retry.",
            "ローカルとクラウドのデータ版が競合しています。状態を更新してから再試行してください。",
            "로컬과 클라우드 데이터 버전이 충돌합니다. 상태를 새로 고친 뒤 다시 시도하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.denied",
        text: row(
            "服务拒绝了这次请求（权限或访问受限）。",
            "服務拒絕了這次請求（權限或存取受限）。",
            "The service rejected the request (permission or access restriction).",
            "サービスが要求を拒否しました（権限またはアクセス制限）。",
            "서비스가 요청을 거부했습니다(권한 또는 접근 제한).",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.expired",
        text: row(
            "这次操作已过期，请重新发起。",
            "這次操作已過期，請重新發起。",
            "This operation expired — start it again.",
            "この操作は期限切れです。もう一度実行してください。",
            "이 작업이 만료되었습니다. 다시 시작하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.locked",
        text: row(
            "同步密钥处于锁定状态，请先解锁。",
            "同步金鑰處於鎖定狀態，請先解鎖。",
            "The sync key is locked — unlock it first.",
            "同期キーがロックされています。先にロック解除してください。",
            "동기화 키가 잠겨 있습니다. 먼저 잠금을 해제하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.server",
        text: row(
            "同步服务返回了错误，请稍后重试。",
            "同步服務回傳錯誤，請稍後重試。",
            "The sync service returned an error — retry later.",
            "同期サービスがエラーを返しました。後で再試行してください。",
            "동기화 서비스가 오류를 반환했습니다. 나중에 다시 시도하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.unsupported",
        text: row(
            "服务端协议不受支持，请更新 OpenLess 或改用自建服务器。",
            "伺服器端協定不受支援，請更新 OpenLess 或改用自建伺服器。",
            "The server protocol is unsupported — update OpenLess or use your own server.",
            "サーバー側プロトコルが未対応です。OpenLess を更新するか自前サーバーを使用してください。",
            "서버 프로토콜이 지원되지 않습니다. OpenLess를 업데이트하거나 자체 서버를 사용하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.invalidResponse",
        text: row(
            "服务返回的内容无法解析，请稍后重试。",
            "服務回傳的內容無法解析，請稍後重試。",
            "The service returned an unreadable response — retry later.",
            "サービスの応答を解析できませんでした。後で再試行してください。",
            "서비스 응답을 해석할 수 없습니다. 나중에 다시 시도하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.cloudDeleted",
        text: row(
            "云端备份已被删除；如需继续请重新启用同步。",
            "雲端備份已被刪除；如需繼續請重新啟用同步。",
            "The cloud backup was deleted — re-enable sync to continue.",
            "クラウドバックアップは削除されました。続けるには同期を再度有効にしてください。",
            "클라우드 백업이 삭제되었습니다. 계속하려면 동기화를 다시 활성화하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.consentRequired",
        text: row(
            "请先确认完整同步范围再继续。",
            "請先確認完整同步範圍再繼續。",
            "Confirm the full sync scope before continuing.",
            "続行する前に同期範囲全体を確認してください。",
            "계속하기 전에 전체 동기화 범위를 확인하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.cryptoWorkerFailed",
        text: row(
            "加密运算失败，请重试；若持续出现请导出日志。",
            "加密運算失敗，請重試；若持續出現請匯出日誌。",
            "Encryption failed — retry; export the logs if it keeps happening.",
            "暗号処理に失敗しました。再試行し、続く場合はログを書き出してください。",
            "암호화 연산이 실패했습니다. 다시 시도하고 계속되면 로그를 내보내세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.deleteConfirmationRequired",
        text: row(
            "删除云端备份需要再次确认。",
            "刪除雲端備份需要再次確認。",
            "Deleting the cloud backup needs another confirmation.",
            "クラウドバックアップの削除には再確認が必要です。",
            "클라우드 백업 삭제에는 다시 확인이 필요합니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.metadataRequired",
        text: row(
            "缺少同步元数据，请刷新状态后重试。",
            "缺少同步中繼資料，請重新整理狀態後重試。",
            "Sync metadata is missing — refresh the status and retry.",
            "同期メタデータがありません。状態を更新して再試行してください。",
            "동기화 메타데이터가 없습니다. 상태를 새로 고친 뒤 다시 시도하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.secureRandomUnavailable",
        text: row(
            "系统安全随机源不可用，无法生成加密密钥。",
            "系統安全隨機源不可用，無法產生加密金鑰。",
            "The system secure random source is unavailable; cannot derive keys.",
            "安全な乱数源が利用できず、鍵を生成できません。",
            "시스템 보안 난수원을 사용할 수 없어 키를 만들 수 없습니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.staleTask",
        text: row(
            "该同步任务已失效，请刷新状态。",
            "該同步任務已失效，請重新整理狀態。",
            "That sync task is no longer valid — refresh the status.",
            "その同期タスクは無効です。状態を更新してください。",
            "해당 동기화 작업이 더 이상 유효하지 않습니다. 상태를 새로 고치세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.errors.serviceUnavailable",
        text: row(
            "加密同步服务暂不可用，请稍后重试。",
            "加密同步服務暫時無法使用，請稍後重試。",
            "Encrypted sync is temporarily unavailable — retry later.",
            "暗号化同期は一時的に利用できません。後で再試行してください。",
            "암호화 동기화를 일시적으로 사용할 수 없습니다. 나중에 다시 시도하세요.",
        ),
    },

    // 加密同步卡片的步骤标题 / 状态 / 按钮文案。这些键在 `cloud_sync_ui.rs` 里
    // 是静态字符串常量，之前也整批缺失，界面上直接画出了 `cloudSyncE2ee.states.disabled`。
    Msg {
        key: "cloudSyncE2ee.checkPending",
        text: row(
            "核对待确认结果",
            "核對待確認結果",
            "Check pending result",
            "未確定の結果を確認",
            "확인 대기 결과 조회",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.conflictCloud",
        text: row(
            "使用云端",
            "使用雲端",
            "Use cloud",
            "クラウドを使用",
            "클라우드 사용",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.conflictLocal",
        text: row(
            "保留本机",
            "保留本機",
            "Keep local",
            "ローカルを使用",
            "로컬 유지",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.createTitle",
        text: row(
            "设置同步密码",
            "設定同步密碼",
            "Create a sync password",
            "同期パスワードを設定",
            "동기화 비밀번호 설정",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.protocolEncryption",
        text: row(
            "配置、服务 API 密钥和历史文本会先在本机加密，发往云端和保存在云端的备份内容均为密文。云同步服务无法据此读取或调用您的服务 API 密钥，也不会使用这些内容执行模型请求。",
            "設定、服務 API 金鑰及歷史文字會先在本機加密，傳送與儲存在雲端的備份內容均為密文。雲端同步服務無法據此讀取或使用您的 API 金鑰，也不會使用這些內容執行模型請求。",
            "Settings, service API keys and text history are encrypted locally. Backup content is sent and stored as ciphertext. The sync service cannot read or use your API keys from it, and does not use this content to run model requests.",
            "設定、API キー、テキスト履歴はこの端末で暗号化されます。バックアップは暗号文として送信・保存されます。同期サービスはその内容から API キーを読み取ったり使用したりせず、モデルへのリクエストにも利用しません。",
            "설정, API 키, 텍스트 기록은 이 기기에서 암호화됩니다. 백업은 암호문으로 전송되고 저장됩니다. 동기화 서비스는 이를 통해 API 키를 읽거나 사용하지 못하며 모델 요청에도 이용하지 않습니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.protocolEncryptionTitle",
        text: row(
            "先在本机加密，再上传",
            "先在本機加密，再上傳",
            "Encrypt on this device before uploading",
            "端末で暗号化してから送信",
            "기기에서 암호화한 후 업로드",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.protocolExcluded",
        text: row(
            "同步密码、解密密钥、OpenLess 管理的 GitHub/OAuth 登录状态、访问令牌及设备私钥不进入备份。账号认证会单独处理必要凭据；服务器仍可见账号标识、密文大小、版本及同步时间。",
            "同步密碼、解密金鑰、OpenLess 管理的 GitHub/OAuth 登入狀態、存取權杖及裝置私鑰不進入備份。帳號驗證會另外處理必要憑證；伺服器仍可見帳號識別碼、密文大小、版本及同步時間。",
            "The backup excludes your sync password, decryption keys, OpenLess-managed GitHub/OAuth sign-in state, access tokens and device private keys. Account authentication handles necessary credentials separately. The server can still see account identifiers, ciphertext size, versions and sync times.",
            "同期パスワード、復号キー、OpenLess が管理する GitHub/OAuth ログイン状態、アクセストークン、端末の秘密鍵はバックアップに含まれません。アカウント認証には必要な認証情報を別途使用します。サーバーにはアカウント識別子、暗号文のサイズ、版、同期日時が見えます。",
            "동기화 비밀번호, 복호화 키, OpenLess가 관리하는 GitHub/OAuth 로그인 상태, 액세스 토큰, 기기 개인 키는 백업에 포함되지 않습니다. 계정 인증에는 필요한 자격 증명을 별도로 사용합니다. 서버는 계정 식별자, 암호문 크기, 버전 및 동기화 시간을 볼 수 있습니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.protocolExcludedTitle",
        text: row(
            "登录状态与可见信息",
            "登入狀態與可見資訊",
            "Sign-in state and visible metadata",
            "ログイン状態と確認可能な情報",
            "로그인 상태와 확인 가능한 정보",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.protocolIntro",
        text: row(
            "OpenLess 及个人开发者尊重您的隐私，并致力于保护您的资料。请阅读以下说明，再决定是否继续。",
            "OpenLess 及個人開發者尊重您的隱私，並致力於保護您的資料。請閱讀以下說明，再決定是否繼續。",
            "OpenLess and its independent developers respect your privacy and work to protect your data. Read this notice before continuing.",
            "OpenLess と開発者はプライバシーを尊重し、データの保護に取り組んでいます。続行する前に、以下をご確認ください。",
            "OpenLess와 독립 개발자는 개인정보를 존중하고 데이터를 보호하기 위해 노력합니다. 계속하기 전에 아래 안내를 읽어 주세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.protocolPassword",
        text: row(
            "同步密码和解密密钥仅用于本机加密与解锁，不会发送给云同步服务。若密码遗失且没有仍可解锁的设备，我们无法找回加密内容。",
            "同步密碼及解密金鑰僅用於本機加密與解鎖，不會傳送給雲端同步服務。若密碼遺失且沒有仍可解鎖的裝置，我們無法找回加密內容。",
            "Your sync password and decryption key are used locally and are never sent to the sync service. If you lose the password and have no device that can still unlock the backup, we cannot recover its contents.",
            "同期パスワードと復号キーは端末内で使用され、同期サービスには送信されません。パスワードを紛失し、解除できる端末もない場合、内容を復元することはできません。",
            "동기화 비밀번호와 복호화 키는 기기에서만 사용되며 동기화 서비스로 전송되지 않습니다. 비밀번호를 잃고 백업을 잠금 해제할 기기도 없다면 내용을 복구할 수 없습니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.protocolPasswordTitle",
        text: row(
            "请妥善保存同步密码",
            "請妥善保存同步密碼",
            "Keep your sync password safe",
            "同期パスワードを大切に保管してください",
            "동기화 비밀번호를 안전하게 보관하세요",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.states.conflict",
        text: row(
            "请审阅冲突更改",
            "請檢閱衝突變更",
            "Review conflicting changes",
            "競合する変更を確認してください",
            "충돌하는 변경 검토 필요",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.states.disabled",
        text: row(
            "同步已关闭",
            "同步已關閉",
            "Sync is off",
            "同期はオフです",
            "동기화 꺼짐",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.states.failed",
        text: row(
            "同步需要处理",
            "同步需要處理",
            "Sync needs attention",
            "同期の確認が必要です",
            "동기화 확인 필요",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.states.outcome_unknown",
        text: row(
            "云端结果待确认",
            "雲端結果待確認",
            "Waiting to confirm the cloud result",
            "クラウドの結果は未確定です",
            "클라우드 결과 확인 대기 중",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.states.pending",
        text: row(
            "本机更改待同步",
            "本機變更待同步",
            "Local changes are waiting to sync",
            "ローカルの変更は同期待ちです",
            "로컬 변경 동기화 대기 중",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.states.ready",
        text: row(
            "已同步",
            "已同步",
            "Up to date",
            "同期済み",
            "최신 상태",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.states.recovery_required",
        text: row(
            "需要恢复本机同步状态",
            "需要復原本機同步狀態",
            "Local recovery is required",
            "ローカル状態の復旧が必要です",
            "로컬 상태 복구 필요",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.states.sign_in_required",
        text: row(
            "需要登录 GitHub",
            "需要登入 GitHub",
            "GitHub sign-in required",
            "GitHub ログインが必要です",
            "GitHub 로그인 필요",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.states.syncing",
        text: row(
            "正在同步",
            "正在同步",
            "Sync in progress",
            "同期中",
            "동기화 중",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.states.unlock_required",
        text: row(
            "请输入密码解锁",
            "請輸入密碼解鎖",
            "Enter your password to unlock",
            "パスワードを入力して解除してください",
            "비밀번호를 입력해 잠금을 해제하세요",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.stepEnableDetail",
        text: row(
            "打开右侧开关。确认协议之后，下一步是设置同步密码。",
            "打開右側開關。確認協議之後，下一步是設定同步密碼。",
            "Use the switch. After you confirm the notice, the next step is a sync password.",
            "スイッチを使います。確認のあと、次は同期パスワードの設定です。",
            "스위치를 사용하세요. 확인 후 다음 단계는 동기화 비밀번호입니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.stepEnableTitle",
        text: row(
            "第 1 步，共 3 步：开启同步",
            "第 1 步，共 3 步：開啟同步",
            "Step 1 of 3: Turn sync on",
            "手順 1/3：同期をオンにする",
            "1/3단계: 동기화 켜기",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.stepFollowDetail",
        text: row(
            "按这一行旁边的按钮继续。",
            "按這一列旁邊的按鈕繼續。",
            "Use the button on this row to continue.",
            "この行のボタンで続けてください。",
            "이 줄의 버튼으로 계속하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.stepUnlockDetail",
        text: row(
            "还差一步。输入同步密码，解锁之后才能同步。",
            "還差一步。輸入同步密碼，解鎖之後才能同步。",
            "One step left. Enter the sync password to unlock this device.",
            "あと 1 ステップです。同期パスワードを入力して、このデバイスのロックを解除します。",
            "한 단계 남았습니다. 동기화 비밀번호로 이 기기의 잠금을 해제하세요.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.stepUnlockTitle",
        text: row(
            "第 2 步，共 3 步：解锁这台设备",
            "第 2 步，共 3 步：解鎖這台裝置",
            "Step 2 of 3: Unlock this device",
            "手順 2/3：このデバイスのロックを解除",
            "2/3단계: 이 기기 잠금 해제",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.syncNow",
        text: row(
            "立即同步",
            "立即同步",
            "Sync now",
            "今すぐ同期",
            "지금 동기화",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.unlockTitle",
        text: row(
            "解锁云端备份",
            "解鎖雲端備份",
            "Unlock your cloud backup",
            "クラウドバックアップを解除",
            "클라우드 백업 잠금 해제",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.stepRestoreTitle",
        text: row(
            "第 3 步，共 3 步：恢复云端备份",
            "第 3 步，共 3 步：還原雲端備份",
            "Step 3 of 3: restore the cloud backup",
            "ステップ 3/3：クラウドバックアップを復元",
            "3/3단계: 클라우드 백업 복원",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.stepRestoreDetail",
        text: row(
            "选择一份云端备份并先预览；确认后才会替换本机资料。",
            "選擇一份雲端備份並先預覽；確認後才會取代本機資料。",
            "Pick a cloud backup and preview it first; local data is only replaced after you confirm.",
            "クラウドバックアップを選んでプレビューし、確認後にのみローカルを置き換えます。",
            "클라우드 백업을 선택해 먼저 미리 본 뒤 확인하면 로컬 데이터가 교체됩니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.noticeDone",
        text: row(
            "已完成。",
            "已完成。",
            "Done.",
            "完了しました。",
            "완료되었습니다.",
        ),
    },
    Msg {
        key: "cloudSyncE2ee.noticeRestored",
        text: row(
            "云端备份已恢复到本机。",
            "雲端備份已還原到本機。",
            "The cloud backup was restored to this device.",
            "クラウドバックアップをこの端末に復元しました。",
            "클라우드 백업을 이 기기로 복원했습니다.",
        ),
    },

    // 提供方设置里以字面量引用的两个键；egui 目录此前缺失，界面上会画成裸键。
    Msg {
        key: "settings.providers.fillDefault",
        text: row(
            "填入默认值",
            "填入預設值",
            "Fill default value",
            "デフォルト値を入力",
            "기본값 입력",
        ),
    },
    Msg {
        key: "settings.providers.llmProtocolHeaderConflict",
        text: row(
            "Messages 已自动设置鉴权和版本请求头，请移除额外 Headers 中的 x-api-key 和 anthropic-version。",
            "Messages 已自動設定驗證與版本標頭，請移除額外 Headers 中的 x-api-key 與 anthropic-version。",
            "Messages sets authentication and version headers automatically. Remove x-api-key and anthropic-version from extra headers.",
            "Messages の認証とバージョンヘッダーは自動設定されます。追加ヘッダーから x-api-key と anthropic-version を削除してください。",
            "Messages 인증 및 버전 헤더는 자동 설정됩니다. 추가 헤더에서 x-api-key와 anthropic-version을 제거하세요.",
        ),
    },
    Msg {
        key: "settings.providers.showSecret",
        text: row("显示", "顯示", "Show", "表示", "표시"),
    },
    Msg {
        key: "settings.providers.hideSecret",
        text: row("隐藏", "隱藏", "Hide", "隠す", "숨기기"),
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
