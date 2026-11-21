import SwiftUI
import UIKit

/// SwiftUI's URL value omits the system's sourceApplication. Its documented
/// app/scene delegate adaptor preserves that context for a keyboard round trip.
final class KeyboardVoiceAppDelegate: NSObject, UIApplicationDelegate {
  func application(_ application: UIApplication,
    configurationForConnecting session: UISceneSession,
    options: UIScene.ConnectionOptions) -> UISceneConfiguration {
    let configuration = UISceneConfiguration(name: nil, sessionRole: session.role)
    configuration.delegateClass = KeyboardVoiceSceneDelegate.self
    return configuration
  }
}

final class KeyboardVoiceSceneDelegate: NSObject, UIWindowSceneDelegate {
  func scene(_ scene: UIScene, willConnectTo session: UISceneSession,
    options: UIScene.ConnectionOptions) {
    receive(options.urlContexts)
  }
  func scene(_ scene: UIScene, openURLContexts contexts: Set<UIOpenURLContext>) {
    receive(contexts)
  }
  private func receive(_ contexts: Set<UIOpenURLContext>) {
    guard contexts.count == 1, let context = contexts.first,
      context.url.scheme == "openless-ios", context.url.host == "activate" else { return }
    AppController.shared.receiveHostActivation(context.url,
      sourceApplication: context.options.sourceApplication)
  }
}
