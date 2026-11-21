import Combine
import Foundation
import OpenLessShared
import SwiftUI

/// A single app-owned presentation model; no dependency on speech or Core settings.
@MainActor
final class InterfacePreferences: ObservableObject {
  @Published private(set) var value: SharedInterfacePreferences
  @Published private(set) var sharingFailed = false
  private let store: InterfacePreferenceStore
  private var mailbox: Mailbox?
  private var sharedStateChanged: (() -> Void)?

  init(defaults: UserDefaults = .standard) {
    store = InterfacePreferenceStore(defaults: defaults)
    value = store.load()
  }

  var language: InterfaceLanguage {
    get { value.language }
    set { update(language: newValue, theme: value.theme) }
  }
  var theme: InterfaceTheme {
    get { value.theme }
    set { update(language: value.language, theme: newValue) }
  }
  var locale: Locale { language.locale }
  var localizer: InterfaceLocalizer { InterfaceLocalizer(language: language) }
  var colorScheme: ColorScheme? {
    switch theme { case .system: nil; case .light: .light; case .dark: .dark }
  }

  /// Attach the existing app mailbox; opening shared storage is owned by AppController.
  func connect(mailbox: Mailbox?, stateChanged: @escaping () -> Void) {
    self.mailbox = mailbox
    sharedStateChanged = stateChanged
    publish()
  }

  /// Call on foreground if another app settings surface still writes AppStorage.
  func reload() {
    value = store.load()
    publish()
  }

  func retrySharing() { publish() }

  private func update(language: InterfaceLanguage, theme: InterfaceTheme) {
    let next = SharedInterfacePreferences(language: language, theme: theme)
    guard next != value else { return }
    value = next
    store.save(next)
    publish()
  }

  private func publish() {
    guard let mailbox else { sharingFailed = false; return }
    do {
      try mailbox.set(value, for: SharedInterfacePreferences.mailboxKey)
      sharingFailed = false
      sharedStateChanged?()
    } catch { sharingFailed = true }
  }
}
