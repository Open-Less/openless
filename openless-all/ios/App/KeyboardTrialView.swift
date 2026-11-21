import OpenLessShared
import SwiftUI
import UIKit

/// A separate native editing surface. SwiftUI's keyboard safe area handles
/// the available height; no notification-driven extra padding or scroll hacks.
struct KeyboardTrialView: View {
  @ObservedObject var controller: AppController
  @State private var focused = false
  @State private var selection = NSRange(location: 0, length: 0)
  @Environment(\.verticalSizeClass) private var verticalSizeClass
  var body: some View {
    VStack(alignment: .leading, spacing: 16) {
      if verticalSizeClass != .compact {
        HStack(spacing: 10) {
          OpenLessMark(size: 28)
          Text("长按地球键，选择 OpenLess。")
            .font(.footnote).foregroundStyle(.secondary)
        }
      }
      ZStack(alignment: .topLeading) {
        NativeTrialEditor(text: $controller.keyboardTestText, focused: $focused, selection: $selection)
          .padding(verticalSizeClass == .compact ? 4 : 12)
        if controller.keyboardTestText.isEmpty {
          Text("在这里输入一些文字…").font(.body).foregroundStyle(.tertiary)
            .padding(.horizontal, 17).padding(.top, 20).allowsHitTesting(false)
            .accessibilityHidden(true)
        }
      }.frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(AppDesign.surface, in: RoundedRectangle(cornerRadius: 20))
        .overlay(RoundedRectangle(cornerRadius: 20).strokeBorder(AppDesign.outline, lineWidth: 1))
    }.padding(.horizontal, 20).padding(.vertical, verticalSizeClass == .compact ? 8 : 20)
      .background(AppDesign.canvas)
      .navigationTitle("试用键盘").navigationBarTitleDisplayMode(.inline)
      .toolbar(.visible, for: .navigationBar).toolbar(.hidden, for: .tabBar)
      .toolbar {
        ToolbarItemGroup(placement: .topBarTrailing) {
          Button("完成") { focused = false }.accessibilityIdentifier("dismissTrialKeyboard")
        }
      }
      .onDisappear { focused = false }
  }
}

