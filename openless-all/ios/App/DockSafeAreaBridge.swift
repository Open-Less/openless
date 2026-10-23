import SwiftUI
import UIKit

/// The native tab controller owns the scroll-view safe areas. Reserve the custom
/// dock there, so content can scroll behind glass and its last row still clears it.
struct DockSafeAreaBridge: UIViewControllerRepresentable {
  let height: CGFloat
  func makeUIViewController(context: Context) -> DockSafeAreaAnchor { DockSafeAreaAnchor() }
  func updateUIViewController(_ controller: DockSafeAreaAnchor, context: Context) {
    controller.dockHeight = height
    DispatchQueue.main.async { [weak controller] in controller?.applyInset() }
  }
}

final class DockSafeAreaAnchor: UIViewController {
  var dockHeight: CGFloat = 0
  override func viewDidAppear(_ animated: Bool) {
    super.viewDidAppear(animated)
    applyInset()
  }
  func applyInset() {
    guard viewIfLoaded?.window != nil else { return }
    var root: UIViewController = self
    while let parent = root.parent { root = parent }
    guard let tabs = findTabs(in: root), tabs.additionalSafeAreaInsets.bottom != dockHeight else { return }
    tabs.additionalSafeAreaInsets.bottom = dockHeight
  }
  private func findTabs(in controller: UIViewController) -> UITabBarController? {
    if let tabs = controller as? UITabBarController { return tabs }
    for child in controller.children {
      if let tabs = findTabs(in: child) { return tabs }
    }
    return nil
  }
}
