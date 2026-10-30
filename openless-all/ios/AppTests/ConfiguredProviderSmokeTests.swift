import Foundation
import XCTest
@testable import OpenLess

/// Opt-in service smoke only: synthetic text -> configured LLM -> preview.
/// It does not exercise microphone, ASR, translation_requested or insertion.
nonisolated final class ConfiguredProviderSmokeTests: XCTestCase {
  @MainActor
  func testConfiguredProviderRoutingMetadata() async throws {
    guard ProcessInfo.processInfo.environment["OPENLESS_INSPECT_PROVIDER_ROUTE"] == "1" else {
      throw XCTSkip("Provider route inspection is disabled.")
    }
    let controller = AppController.shared
    let deadline = ProcessInfo.processInfo.systemUptime + 30
    while !controller.coreReady && ProcessInfo.processInfo.systemUptime < deadline {
      try await Task.sleep(nanoseconds: 100_000_000)
    }
    guard let config = controller.developer.configuration,
      let channel = config.channels.llm.first(where: { $0.id == config.active.llm }),
      let url = URL(string: channel.endpoint), let host = url.host else {
      XCTFail("No readable configured text service endpoint.")
      return
    }
    let local = host == "localhost" || host == "::1" || host.hasPrefix("127.")
      || host.hasSuffix(".localhost") || host.hasSuffix(".local")
    let fixtureName = [channel.id, channel.model, channel.name ?? ""].contains {
      $0.localizedCaseInsensitiveContains("fixture") || $0.localizedCaseInsensitiveContains("mock")
    }
    let attachment = XCTAttachment(string: "Endpoint scheme: \(url.scheme ?? "unknown")\nLoopback/local host: \(local)\nFixture/mock marker in channel metadata: \(fixtureName)")
    attachment.name = "Configured provider route classification"
    attachment.lifetime = .keepAlways
    add(attachment)
  }

  @MainActor
  func testConfiguredProviderTranslatesSyntheticTextToEnglish() async throws {
    guard ProcessInfo.processInfo.environment["OPENLESS_RUN_CONFIGURED_PROVIDER_SMOKE"] == "1" else {
      throw XCTSkip("Configured-provider smoke is disabled; explicit opt-in is required.")
    }
    executionTimeAllowance = 120
    let controller = AppController.shared
    let deadline = ProcessInfo.processInfo.systemUptime + 30
    var bridge: CoreBridge?
    while ProcessInfo.processInfo.systemUptime < deadline {
      if controller.coreReady, !controller.restarting,
        !controller.styleLibraryActivityBusy, !controller.developer.busy, !controller.library.busy,
        let current = controller.styleLibraryCore, !current.hasPendingControls {
        bridge = current
        break
      }
      try await Task.sleep(nanoseconds: 100_000_000)
    }
    guard let bridge else {
      XCTFail("The app Core did not become ready and idle within 30 seconds.")
      return
    }
    guard controller.developerMode else {
      XCTFail("The configured-provider smoke requires the app's existing developer configuration.")
      return
    }

    // Fixed test data only. Never read a text field, selection, history or clipboard.
    let input = "你好，世界。"
    let instruction = "Translate the supplied text into English only. Return only the English translation, without explanation, quotation marks, or any additional text."
    let data: Data
    do {
      data = try await bridge.control([
        "action": "rewrite", "text": input, "instruction": instruction,
      ])
    } catch {
      // Provider failures may contain server/endpoint details. Do not interpolate
      // the Error or response into XCTest output or attachments.
      XCTFail("Configured LLM rewrite failed. Check the provider, model and access in the app.")
      return
    }
    guard controller.styleLibraryCore === bridge else {
      XCTFail("The app Core changed while the synthetic request was running.")
      return
    }
    guard data.count <= 4096 else {
      XCTFail("The synthetic rewrite response exceeded its 4 KiB test bound.")
      return
    }
    struct Response: Decodable { let text: String }
    guard let response = try? JSONDecoder().decode(Response.self, from: data) else {
      XCTFail("The synthetic rewrite did not return the expected text response.")
      return
    }
    let result = response.text.trimmingCharacters(in: .whitespacesAndNewlines)
    guard !result.isEmpty, result.utf8.count <= 1024 else {
      XCTFail("The synthetic translation was empty or exceeded 1 KiB.")
      return
    }
    let letters = result.unicodeScalars.filter { CharacterSet.letters.contains($0) }
    guard !letters.isEmpty, letters.allSatisfy({ $0.isASCII }) else {
      XCTFail("The synthetic response was not English-only text.")
      return
    }
    let words = result.lowercased().split { !$0.isLetter }.map(String.init)
    guard words.contains("world"), words.contains(where: { ["hello", "hi", "greetings"].contains($0) }) else {
      XCTFail("The synthetic response did not preserve the expected greeting and world meaning.")
      return
    }

    // Optional evidence contains only fixed synthetic input and the bounded result.
    if ProcessInfo.processInfo.environment["OPENLESS_PROVIDER_SMOKE_ATTACH_RESULT"] == "1" {
      let attachment = XCTAttachment(string: "Input: \(input)\nResult: \(result)")
      attachment.name = "Synthetic configured-provider translation"
      attachment.lifetime = .keepAlways
      add(attachment)
    }
  }
}
