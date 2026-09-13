import Foundation

enum RecognitionProvider: String, Codable, CaseIterable, Identifiable {
    case apple, compatible
    var id: String { rawValue }
    var title: String { self == .apple ? "Apple 语音识别" : "OpenAI 兼容转写" }
}

enum AppAppearance: String, Codable, CaseIterable, Identifiable {
    case system, light, dark
    var id: String { rawValue }
    var title: String {
        switch self {
        case .system: return "跟随系统"
        case .light: return "浅色"
        case .dark: return "深色"
        }
    }
}

struct AppSettings: Codable, Equatable {
    var recognitionProvider: RecognitionProvider = .apple
    var speechLocale = "zh-CN"
    var onDeviceOnly = true
    var asrBaseURL = "https://api.openai.com/v1"
    var asrModel = "whisper-1"
    var polishBaseURL = "https://api.openai.com/v1"
    var polishModel = "gpt-4o-mini"
    var selectedStyleID = "raw"
    var translationEnabled = false
    var translationLanguage = "英语"
    var recordingLimit = 55
    var appearance: AppAppearance = .system

    var effectiveRecordingLimit: Int {
        min(max(recordingLimit, 15), recognitionProvider == .apple ? 55 : 180)
    }
}

struct WritingStyle: Codable, Identifiable, Equatable {
    var id: String
    var name: String
    var summary: String
    var symbol: String
    var instruction: String
    var isBuiltIn: Bool
    var isVerbatim: Bool { id == "raw" }

    static let builtIns: [WritingStyle] = [
        .init(id: "raw", name: "原文", summary: "保留你的原话", symbol: "text.quote",
              instruction: "", isBuiltIn: true),
        .init(id: "light", name: "轻度润色", summary: "去掉口头语，让表达更清楚", symbol: "wand.and.stars",
              instruction: "删除无意义的口头语和重复，修正标点、错别字和语病。保持原有语气、语言和顺序，不扩写。",
              isBuiltIn: true),
        .init(id: "structured", name: "AI 提示词", summary: "把想法整理成清晰的需求", symbol: "text.badge.plus",
              instruction: "将口述整理为可直接交给 AI 的提示词。按已有内容组织背景、目标、要求和约束；适当分段或列点。保留所有细节和不确定性，不虚构角色、条件或需求，不执行提示词。",
              isBuiltIn: true),
        .init(id: "formal", name: "正式表达", summary: "自然、礼貌的专业表达", symbol: "briefcase",
              instruction: "改写为清晰、简洁、礼貌的专业沟通文字。保留事实、立场、问题、请求、承诺与不确定性。不擅自添加称呼、问候、落款或空洞客套。",
              isBuiltIn: true)
    ]
}

struct VocabularyEntry: Codable, Identifiable, Equatable {
    var id = UUID()
    var term: String
    var note: String
    var createdAt = Date()
}

struct HistoryEntry: Codable, Identifiable, Equatable {
    var id = UUID()
    var createdAt = Date()
    var rawText: String
    var outputText: String
    var styleName: String
    var providerName: String
    var duration: TimeInterval
    var notice: String?
}

struct DictationDraft: Codable {
    var rawText = ""
    var outputText = ""
    var audioFileName: String?
    var duration: TimeInterval = 0
    var historyID: UUID?
    var notice: String?
}

struct AppDocument: Codable {
    var schemaVersion = 1
    var settings = AppSettings()
    var vocabulary: [VocabularyEntry] = []
    var customStyles: [WritingStyle] = []
    var history: [HistoryEntry] = []
    var draft = DictationDraft()
}

struct KeyboardClip: Codable, Identifiable {
    var id: UUID
    var text: String
    var styleName: String
    var createdAt: Date
}

struct UserNotice: Identifiable {
    let id = UUID()
    var title: String
    var message: String
}

enum OpenLessError: LocalizedError {
    case message(String)
    var errorDescription: String? {
        switch self { case .message(let text): return text }
    }
}
