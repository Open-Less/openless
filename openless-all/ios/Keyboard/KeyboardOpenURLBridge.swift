import SwiftUI
import UIKit

/// A mounted SwiftUI environment supplies the public OpenURLAction. UIKit's
/// existing buttons keep their own visuals, accessibility and touch handling.
@MainActor
final class KeyboardOpenURLBridge {
  private var host: UIHostingController<OpenURLEnvironmentCapture>?
  private var environmentAction: OpenURLAction?

  /// Call once from KeyboardViewController.viewDidLoad, after its main layout.
  /// Keep this bridge as a controller property for the keyboard's lifetime.
  func install(in parent: UIViewController) {
    guard host == nil else { return }
    let content = OpenURLEnvironmentCapture(
      appeared: { [weak self] action in self?.environmentAction = action },
      disappeared: { [weak self] in self?.environmentAction = nil })
    let host = UIHostingController(rootView: content)
    parent.addChild(host)
    host.view.backgroundColor = .clear
    host.view.translatesAutoresizingMaskIntoConstraints = false
    host.view.isUserInteractionEnabled = false
    host.view.accessibilityElementsHidden = true
    parent.view.addSubview(host.view)
    NSLayoutConstraint.activate([
      host.view.leadingAnchor.constraint(equalTo: parent.view.leadingAnchor),
      host.view.topAnchor.constraint(equalTo: parent.view.topAnchor),
      host.view.widthAnchor.constraint(equalToConstant: 1),
      host.view.heightAnchor.constraint(equalToConstant: 1),
    ])
    host.didMove(toParent: parent)
    self.host = host
  }

  /// Call synchronously from the existing UIButton/UIControl user action,
  /// after the HostVoiceActivation request has been durably saved, if needed.
  /// Returns false when the SwiftUI environment is not mounted/visible yet.
  @discardableResult
  func open(_ url: URL, completion: @escaping @MainActor (Bool) -> Void) -> Bool {
    guard url.scheme == "openless-ios", ["settings", "styles", "activate"].contains(url.host ?? ""),
      let environmentAction else { return false }
    environmentAction(url) { accepted in
      Task { @MainActor in completion(accepted) }
    }
    return true
  }

  /// Explicitly tear down if the containing controller replaces its UI tree.
  func uninstall() {
    environmentAction = nil
    guard let host else { return }
    host.willMove(toParent: nil)
    host.view.removeFromSuperview()
    host.removeFromParent()
    self.host = nil
  }
}

private struct OpenURLEnvironmentCapture: View {
  @Environment(\.openURL) private var openURL
  let appeared: @MainActor (OpenURLAction) -> Void
  let disappeared: @MainActor () -> Void

  var body: some View {
    Color.clear
      .allowsHitTesting(false)
      .accessibilityHidden(true)
      .onAppear { appeared(openURL) }
      .onDisappear { disappeared() }
  }
}
