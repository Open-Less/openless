import UIKit
import XCTest
@testable import OpenLess

nonisolated final class SpeakButtonVisualTests: XCTestCase {
  @MainActor
  func testRecordingAndThinkingAppearance() async throws {
    guard let scene = UIApplication.shared.connectedScenes.first as? UIWindowScene else {
      throw XCTSkip("A foreground window scene is required")
    }
    let previous = scene.windows.first(where: \.isKeyWindow)
    let window = UIWindow(windowScene: scene)
    let controller = UIViewController()
    window.rootViewController = controller
    window.overrideUserInterfaceStyle = .dark
    controller.view.backgroundColor = .secondarySystemBackground
    let button = SpeakButton()
    button.presentation = .video
    button.translatesAutoresizingMaskIntoConstraints = false
    controller.view.addSubview(button)
    NSLayoutConstraint.activate([
      button.widthAnchor.constraint(equalToConstant: 176),
      button.heightAnchor.constraint(equalToConstant: 140),
      button.centerXAnchor.constraint(equalTo: controller.view.centerXAnchor),
      button.centerYAnchor.constraint(equalTo: controller.view.centerYAnchor),
    ])
    window.makeKeyAndVisible()
    defer { window.isHidden = true; previous?.makeKey() }
    controller.view.layoutIfNeeded()
    button.configure(phase: .recording, level: 0, available: true)
    try await Task.sleep(for: .milliseconds(300))
    capture(controller.view, name: "recording-silence")
    // Synthetic RMS values exercise presentation without opening the microphone.
    for _ in 0..<3 {
      for level: Float in [0, 0.004, 0.01, 0.025, 0.06, 0.1, 0.025, 0.01] {
        button.configure(phase: .recording, level: level, available: true)
        try await Task.sleep(for: .milliseconds(160))
      }
    }
    capture(controller.view, name: "recording-speech")
    XCTAssertEqual(button.accessibilityLabel, "结束录音")
    button.configure(phase: .processing, level: 0, available: false)
    try await Task.sleep(for: .milliseconds(350))
    XCTAssertEqual(button.accessibilityLabel, "thinking")
    capture(controller.view, name: "thinking")
  }

  @MainActor private func capture(_ view: UIView, name: String) {
    let image = UIGraphicsImageRenderer(bounds: view.bounds).image { context in
      view.layer.render(in: context.cgContext)
    }
    let attachment = XCTAttachment(image: image)
    attachment.name = name
    attachment.lifetime = .keepAlways
    add(attachment)
  }
}
