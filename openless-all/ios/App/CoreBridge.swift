import Foundation
import OpenLessShared

private nonisolated let delivery = NativeDelivery()
private nonisolated struct CallbackKind: Decodable { let kind: String }
private nonisolated let audioSinks = NativeAudioSinks()

/// The C callback must not access MainActor's receivers table for local PCM.
private nonisolated final class NativeAudioSinks: @unchecked Sendable {
  typealias Sink = @Sendable (String, Data) -> NativeAudioDisposition
  private let lock = NSLock()
  private var sinks: [UInt64: Sink] = [:]

  func set(_ sink: Sink?, token: UInt64) {
    lock.lock()
    defer { lock.unlock() }
    sinks[token] = sink
  }
  func get(token: UInt64) -> Sink? {
    lock.lock()
    defer { lock.unlock() }
    return sinks[token]
  }
}

struct NativePayload: Decodable { var text: String?; var provider: String? }
struct CoreResult: Decodable {
  let rawText: String
  let polishedText: String
  let durationMs: UInt64
  let inserted: String
}
struct CoreGeneratedPreview: Decodable {
  let sessionID: String
  let text: String
  let providerType: String
  let model: String?
  let promptKind: String
}
struct CoreEvent: Decodable {
  let kind: String
  let requestID: String?
  let operation: String?
  let sessionID: String?
  let clientSessionID: String?
  let payload: NativePayload?
  let result: CoreResult?
  let entries: [CoreHistoryEntry]?
  let level: Float?
  let elapsedMs: UInt64?
  let message: String?
  let ok: Bool?
  let json: String?
  let text: String?
  let stage: String?
  let intent: KeyboardVoiceIntent?
  let outputDisposition: String?
  let preview: CoreGeneratedPreview?
}

private nonisolated let eventCallback: OLCoreEvent = { token, bytes, length in
  guard let bytes, length > 0, length <= 2 * 1024 * 1024 else { return }
  let data = Data(bytes: bytes, count: length)
  if (try? JSONDecoder().decode(CallbackKind.self, from: data).kind) == "level" {
    guard delivery.offerLevel(token: token, data: data) else { return }
    DispatchQueue.main.async {
      guard let latest = delivery.takeLevel(token: token) else { return }
      MainActor.assumeIsolated { CoreBridge.receivers[token]?.receive(latest) }
    }
  } else {
    DispatchQueue.main.async {
      MainActor.assumeIsolated { CoreBridge.receivers[token]?.receive(data) }
    }
  }
}
private nonisolated let pcmCallback: OLCorePCM = { token, session, bytes, length in
  guard let bytes, let session, length > 0, length <= 64 * 1024 else { return }
  let id = String(cString: session)
  let data = Data(bytes: bytes, count: length)
  if let sink = audioSinks.get(token: token) {
    switch sink(id, data) {
    case .consumed: return
    case .overflow:
      DispatchQueue.main.async {
        MainActor.assumeIsolated {
          CoreBridge.receivers[token]?.onAudioFailure?(id, "录音超过本地识别容量，已停止本次录音。")
        }
      }
      return
    case .forwardToMain: break
    }
  }
  switch delivery.reservePCM(token: token, session: id) {
  case .discard: return
  case .overflow:
    DispatchQueue.main.async {
      MainActor.assumeIsolated {
        CoreBridge.receivers[token]?.onAudioFailure?(id, "界面处理音频不及时，已停止本次录音，请重试。")
      }
    }
    return
  case .accept: break
  }
  // Only the explicitly selected MainActor Apple backend needs this bounded hop.
  DispatchQueue.main.async {
    defer { delivery.releasePCM(token: token, session: id) }
    MainActor.assumeIsolated { CoreBridge.receivers[token]?.onAudio?(id, data) }
  }
}

final class CoreBridge {
  fileprivate static var receivers: [UInt64: CoreBridge] = [:]
  private static var nextToken: UInt64 = 1
  nonisolated let handle: UInt64
  private let token: UInt64
  private var controls: [String: CheckedContinuation<Data, any Error>] = [:]
  private var controlTimeouts: [String: Task<Void, Never>] = [:]
  private var queuedControls: [(id: String, command: [String: Any])] = []
  private var activeControlID: String?
  private var closed = false
  private var mutationRequests: Set<String> = []
  var hasPendingControls: Bool { !controls.isEmpty }
  var onConfigurationMutation: (() -> Void)?
  var onEvent: ((CoreEvent) -> Void)?
  var onAudio: ((String, Data) -> Void)?
  var onTransportFailure: ((String) -> Void)?
  var onAudioFailure: ((String, String) -> Void)?

  func setAudioSink(_ sink: (@Sendable (String, Data) -> NativeAudioDisposition)?) {
    audioSinks.set(sink, token: token)
  }

  private init(handle: UInt64, token: UInt64) {
    self.handle = handle
    self.token = token
  }
  static func open(directory: URL, developerMode: Bool) async throws -> CoreBridge {
    let token = nextToken
    nextToken += 1
    let config = try JSONSerialization.data(withJSONObject: ["dataDir": directory.path, "developerMode": developerMode])
    let handle = await Task.detached {
      config.withUnsafeBytes { bytes in
        ol_ios_create(
          bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count, eventCallback, pcmCallback,
          token)
      }
    }.value
    guard handle != 0 else {
      throw NSError(
        domain: "OpenLessCore", code: 1, userInfo: [NSLocalizedDescriptionKey: "共享业务核心初始化失败"])
    }
    let bridge = CoreBridge(handle: handle, token: token)
    receivers[token] = bridge
    return bridge
  }
  fileprivate func receive(_ data: Data) {
    guard let event = try? JSONDecoder().decode(CoreEvent.self, from: data) else { return }
    if event.kind == "controlResult", let id = event.requestID {
      if event.ok == true, let data = event.json?.data(using: .utf8) {
        completeControl(id, result: .success(data))
      } else {
        completeControl(id, result: .failure(DeveloperError.rejected(event.message ?? "核心请求未成功")))
      }
      return
    }
    onEvent?(event)
  }
  private func completeControl(_ id: String, result: Result<Data, any Error>) {
    controlTimeouts.removeValue(forKey: id)?.cancel()
    let continuation = controls.removeValue(forKey: id)
    // Multi-field Host operations may partially commit before failure/timeout.
    // Invalidate consumers on every terminal mutation result, not success only.
    if mutationRequests.remove(id) != nil { onConfigurationMutation?() }
    if activeControlID == id { activeControlID = nil; sendNextControl() }
    continuation?.resume(with: result)
  }
  private func sendNextControl() {
    guard !closed, activeControlID == nil, !queuedControls.isEmpty else { return }
    let next = queuedControls.removeFirst()
    activeControlID = next.id
    controlTimeouts[next.id] = Task { [weak self] in
      do { try await Task.sleep(for: .seconds(75)) } catch { return }
      self?.completeControl(next.id, result: .failure(DeveloperError.timedOut))
    }
    if !command(next.command) { completeControl(next.id, result: .failure(DeveloperError.unavailable)) }
  }
  func control(_ action: [String: Any]) async throws -> Data {
    try Task.checkCancellation()
    guard !closed, controls.count < 32 else { throw DeveloperError.unavailable }
    let id = UUID().uuidString
    return try await withCheckedThrowingContinuation { continuation in
      controls[id] = continuation
      let reads: Set<String> = ["snapshot", "library", "searchHistory", "historyDetail", "audioAsset", "vocabulary", "styleLibrary", "styleDetail", "capabilitySnapshot", "translationConfiguration", "marketplaceList", "marketplaceDetail", "listModels", "validate"]
      if let name = action["action"] as? String {
        let nested = (action["request"] as? [String: Any])?["command"] as? String
        let localRead = (name == "vocabularyBridge" && ["snapshot", "proposals"].contains(nested ?? ""))
          || (name == "accountSync" && nested == "status")
          || name == "rewrite" // Supplied-text processing does not mutate provider settings.
        if !reads.contains(name), !localRead { mutationRequests.insert(id) }
      }
      var command = action
      command["operation"] = "control"
      command["requestID"] = id
      queuedControls.append((id, command))
      sendNextControl()
    }
  }
  @discardableResult func command(_ object: [String: Any]) -> Bool {
    guard let data = try? JSONSerialization.data(withJSONObject: object) else { return false }
    return data.withUnsafeBytes { bytes in
      ol_ios_command(handle, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count) == 0
    }
  }
  func reply(_ request: String, value: [String: Any] = [:], error: String? = nil) {
    var object: [String: Any] = ["requestID": request, "ok": error == nil, "value": value]
    if let error { object["error"] = error }
    guard let data = try? JSONSerialization.data(withJSONObject: object) else { return }
    _ = data.withUnsafeBytes { bytes in
      ol_ios_reply(handle, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
    }
  }
  nonisolated func feed(_ data: Data, session: String) -> Int32 {
    session.withCString { id in
      data.withUnsafeBytes { bytes in
        ol_ios_feed_pcm(handle, id, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count)
      }
    }
  }
  func close() async {
    closed = true
    audioSinks.set(nil, token: token)
    queuedControls.removeAll()
    for id in Array(controls.keys) { completeControl(id, result: .failure(CancellationError())) }
    Self.receivers[token] = nil
    let handle = handle
    await Task.detached { ol_ios_destroy(handle) }.value
    delivery.remove(token: token)
  }
}
