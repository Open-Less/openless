import OpenLessShared
import SwiftUI

/// Observe presentation changes without replacing RootView or its active speech state.
struct InterfacePreferenceModifier: ViewModifier {
  @ObservedObject var preferences: InterfacePreferences
  func body(content: Content) -> some View {
    content
      .environmentObject(preferences)
      .environment(\.locale, preferences.locale)
      .preferredColorScheme(preferences.colorScheme)
  }
}

struct InterfacePreferencesView: View {
  @ObservedObject var preferences: InterfacePreferences

  var body: some View {
    Form {
      Section {
        Picker(selection: Binding(get: { preferences.language }, set: { preferences.language = $0 })) {
          Text(preferences.localizer.text("跟随系统")).tag(InterfaceLanguage.system)
          Text(verbatim: "简体中文").tag(InterfaceLanguage.simplifiedChinese)
          Text(verbatim: "English").tag(InterfaceLanguage.english)
        } label: { Text(preferences.localizer.text("界面语言")) }
          .pickerStyle(.navigationLink)
          .accessibilityIdentifier("interfaceLanguagePicker")
      } header: {
        Text(preferences.localizer.text("界面语言"))
      } footer: {
        Text(preferences.localizer.text("仅更改界面文字，不会更改语音识别方式、翻译目标或键盘布局。"))
      }
      Section {
        Picker(selection: Binding(get: { preferences.theme }, set: { preferences.theme = $0 })) {
          Text(preferences.localizer.text("跟随系统")).tag(InterfaceTheme.system)
          Text(preferences.localizer.text("浅色")).tag(InterfaceTheme.light)
          Text(preferences.localizer.text("深色")).tag(InterfaceTheme.dark)
        } label: { Text(preferences.localizer.text("应用与键盘外观")) }
          .pickerStyle(.navigationLink)
          .accessibilityIdentifier("interfaceThemePicker")
      } header: {
        Text(preferences.localizer.text("外观"))
      } footer: {
        Text(preferences.localizer.text("允许完全访问时，键盘使用相同的界面语言与深浅色偏好；否则跟随系统。基础打字不需要完全访问。"))
      }
      if preferences.sharingFailed {
        Section {
          Label(preferences.localizer.text("偏好已保存在应用中，但未能同步到键盘。"), systemImage: "exclamationmark.triangle")
          Button(preferences.localizer.text("重试同步外观偏好")) { preferences.retrySharing() }
        }
      }
    }
    .navigationTitle(preferences.localizer.text("界面语言与外观"))
    .navigationBarTitleDisplayMode(.inline)
    .environment(\.locale, preferences.locale)
    .preferredColorScheme(preferences.colorScheme)
  }
}
