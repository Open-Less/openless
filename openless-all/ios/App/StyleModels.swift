import Foundation

struct InstalledStyle: Decodable, Identifiable {
  let id: String
  let name: String
  let description: String
  let version: String
  let kind: String
  let baseMode: String
  let enabled: Bool
  let active: Bool
  let recommendedModel: String?
  let originPackId: String?
  let originAuthorLogin: String?
  var isRaw: Bool { id == "builtin.raw" }
  var canRemove: Bool { kind == "imported" }
}

struct StyleArchivePolicy: Decodable, Sendable {
  let format: String
  let maxCompressedBytes: Int
  let stagingDirectory: String
}

struct StyleLibrarySnapshot: Decodable {
  let packs: [InstalledStyle]
  let activeStylePackId: String
  let archive: StyleArchivePolicy
  let editorSupported: Bool
  let updateSupported: Bool
  let repeatImportBehavior: String
}

struct StyleDetail: Decodable {
  struct Pack: Decodable {
    struct Example: Decodable {
      let title: String?
      let input: String
      let output: String
    }
    let id: String
    let name: String
    let description: String
    let author: String?
    let version: String
    let baseMode: String
    let prompt: String
    let selectionPrompt: String
    let voiceEditPrompt: String
    let examples: [Example]
    let tags: [String]
    let recommendedModel: String?
    let compatibleAppVersion: String?
  }
  struct Runtime: Decodable {
    let packPrompt: String
    let singleTurnPrompt: String
    let multiTurnPrompt: String
    let workingLanguages: [String]
    let previewOmitsFrontApp: Bool
  }
  let pack: Pack
  let runtime: Runtime
}

struct StyleCapabilities: Decodable {
  struct LLM: Decodable {
    let enabled: Bool
    let configured: Bool
    let providerSupported: Bool
    let providerType: String?
    let model: String
    let availability: String
    let canAttemptPolish: Bool
    let canAttemptTranslation: Bool
    let verification: String
    let modelAvailability: String
    let entitlement: String
    var configurationDescription: String {
      switch availability {
      case "disabled": "尚未开启"
      case "requiresTraditionalPipeline": "使用音频模型"
      case "missingConfiguration": "尚未设置"
      case "unsupportedProvider": "暂不可用"
      case "configuredNotVerified": "已设置"
      default: "暂不可用"
      }
    }
  }
  let developerMode: Bool
  let pipelineMode: String
  let llm: LLM
  func canActivate(_ style: InstalledStyle) -> Bool { style.isRaw || developerMode }
  func canChangeEnabled(_ style: InstalledStyle) -> Bool { developerMode || !style.isRaw }
}

struct MarketStyle: Decodable, Identifiable {
  let id: String
  let name: String
  let description: String
  let authorLogin: String
  let version: String
  let baseMode: String
  let tags: [String]
  let likeCount: Int
  let downloadCount: Int
  let originPackId: String?
  let originAuthorLogin: String?
}

struct MarketStyleDetail: Decodable {
  let summary: MarketStyle
  let prompt: String
  let state: String
  private enum CodingKeys: String, CodingKey { case prompt, state }
  init(from decoder: any Decoder) throws {
    summary = try MarketStyle(from: decoder)
    let container = try decoder.container(keyedBy: CodingKeys.self)
    prompt = try container.decode(String.self, forKey: .prompt)
    state = try container.decode(String.self, forKey: .state)
  }
}

/// Replace this single-target consumer when lane T's Host wire contract lands.
/// workingLanguages is recognition context, never an ordered target collection.
struct StyleTranslationConfiguration: Decodable {
  struct Language: Decodable, Identifiable {
    let code: String
    let nativeName: String
    var id: String { code }
  }
  let catalog: [Language]
  let targetLanguage: String
  let targetLanguages: [String]?
  var orderedTargets: [String] { targetLanguages ?? (targetLanguage.isEmpty ? [] : [targetLanguage]) }
  let workingLanguages: [String]
  let maxTargets: Int
  let multipleTargetsSupported: Bool
  let targetOrderingSupported: Bool
  let providerLanguageSupport: String
  let effective: Bool
}
