import Foundation
import Combine

struct DeveloperConfiguration: Decodable {
  struct Preferences: Decodable { let pipelineMode: String; let multimodalPipelineEnabled: Bool }
  struct Channels: Decodable { let asr: [DeveloperChannel]; let llm: [DeveloperChannel] }
  struct Active: Decodable { let asr: String; let llm: String; let omni: String }
  struct Credentials: Decodable { let asrConfigured: Bool; let llmConfigured: Bool; let omniConfigured: Bool }
  let developerMode: Bool
  let preferences: Preferences
  let polishEnabled: Bool
  let channels: Channels
  let active: Active
  let providerDescriptors: [DeveloperProvider]
  let omniConfigurations: [DeveloperChannel]
  let credentialsStatus: Credentials
  var isOmni: Bool { preferences.multimodalPipelineEnabled && preferences.pipelineMode == "multimodal" }
  var activeASR: DeveloperChannel? { channels.asr.first { $0.id == active.asr } }
}
struct DeveloperProvider: Decodable, Identifiable, Hashable {
  let kind: String
  let providerType: String
  let labelKey: String
  let defaultEndpoint: String?
  let defaultModel: String?
  let staticModels: [String]?
  let supported: Bool
  var id: String { kind + ":" + providerType }
  var title: String {
    switch providerType {
    case "ios-sensevoice": "SenseVoice Small · 本地"
    case "apple-speech": "Apple Speech · 系统"
    case "custom", "openai-compatible": "自定义兼容 API"
    case "custom_responses": "自定义 Responses API"
    case "custom_messages": "自定义 Messages API"
    case "volcengine": "火山引擎"
    case "bailian", "dashscope-omni": "阿里云百炼"
    case "openai": "OpenAI"
    case "gemini": "Google Gemini"
    case "deepseek": "DeepSeek"
    default: providerType
    }
  }
}
struct DeveloperChannel: Decodable, Identifiable {
  let id: String
  let providerType: String
  let name: String?
  let endpoint: String
  let model: String
  let hasAPIKey: Bool
  let supported: Bool?
  var title: String { name.flatMap { $0.isEmpty ? nil : $0 } ?? providerType }
}

final class DeveloperSettings: ObservableObject {
  private weak var core: CoreBridge?
  @Published var configuration: DeveloperConfiguration?
  @Published var busy = false
  @Published var error: String?
  var onConfigurationChange: ((Bool) -> Void)?
  func connect(_ core: CoreBridge?) { self.core = core; configuration = nil; error = nil }
  func refresh() async {
    do { configuration = try await request(["action": "snapshot"]); error = nil }
    catch is CancellationError {}
    catch { configuration = nil; self.error = error.localizedDescription }
  }
  private func request<T: Decodable>(_ action: [String: Any]) async throws -> T {
    guard let core else { throw DeveloperError.unavailable }
    let data = try await core.control(action)
    guard self.core === core else { throw CancellationError() }
    return try JSONDecoder().decode(T.self, from: data)
  }
  func perform(_ action: [String: Any], changesRuntime: Bool = true) async -> Bool {
    guard !busy else { return false }
    let affectsSpeech = changesRuntime && Self.affectsSpeechPreparation(action, currentPipeline: configuration?.preferences.pipelineMode)
    busy = true; error = nil
    defer { busy = false }
    do {
      guard let core else { throw DeveloperError.unavailable }
      _ = try await core.control(action)
      await refresh()
      if changesRuntime { onConfigurationChange?(affectsSpeech) }
      return true
    } catch {
      let message = error.localizedDescription
      await refresh()
      if changesRuntime { onConfigurationChange?(affectsSpeech) }
      self.error = "配置可能部分保存，请核对刷新后的状态。\n" + message
      return false
    }
  }
  static func affectsSpeechPreparation(_ action: [String: Any], currentPipeline: String?) -> Bool {
    if let kind = action["kind"] as? String { return kind != "llm" }
    if action["action"] as? String == "updateSettings" {
      return action["pipelineMode"] as? String != currentPipeline
    }
    return true
  }
  func models(kind: String, channelID: String) async -> [String] {
    struct Models: Decodable { let models: [String] }
    do {
      let result: Models = try await request(["action": "listModels", "kind": kind, "channelID": channelID])
      return result.models
    } catch { self.error = error.localizedDescription; return [] }
  }
  func validate(kind: String, channelID: String) async -> Bool {
    struct Check: Decodable { let ok: Bool }
    do {
      let result: Check = try await request(["action": "validate", "kind": kind, "channelID": channelID])
      if !result.ok { error = "验证未通过，请检查已保存的配置。" }
      return result.ok
    } catch { self.error = error.localizedDescription; return false }
  }
}
enum DeveloperError: Error, LocalizedError {
  case unavailable, timedOut, rejected(String)
  var errorDescription: String? {
    switch self {
    case .unavailable: "共享核心未就绪，请稍后重试。"
    case .timedOut: "提供商请求超时，请检查地址、模型与网络。"
    case .rejected(let message): message
    }
  }
}
