@preconcurrency import KeyboardKit
import SwiftUI
import OpenLessShared
import UIKit

/// Renders the configured keyboard layout and delegates key gestures to its
/// input engine.
struct TypingKeyboard: View {
  let state: Keyboard.State
  let services: Keyboard.Services
  @ObservedObject var context: KeyboardContext
  var interfaceLanguage: InterfaceLanguage = .system
  var keyboardScale: Double = 1
  var experience: KeyboardExperience = .voiceFocused
  var spaceHoldRecording = false
  weak var owner: KeyboardViewController?

  var body: some View {
    GeometryReader { geometry in
      let layout = typingLayout
      let naturalHeight = layout.totalHeight
      let scale = min(1, max(1, geometry.size.height) / max(1, naturalHeight))
      keyboard(layout: layout)
        .frame(width: geometry.size.width / scale, height: naturalHeight, alignment: .top)
        .scaleEffect(scale, anchor: .topLeading)
    }
  }

  private var typingLayout: KeyboardLayout {
    var layout = services.layoutService.keyboardLayout(for: context)
    // Four portrait rows fill the shared viewport without shrinking keycaps.
    // Preserve the configured stagger, widths, and gesture behavior.
    if context.interfaceOrientation.isPortrait && layout.itemRows.count == 4 {
      for row in layout.itemRows.indices {
        for column in layout.itemRows[row].indices {
          layout.itemRows[row][column].size.height = CGFloat(KeyboardSizing.bodyHeight(scale: keyboardScale, experience: experience)) / 4
        }
      }
      layout.idealItemHeight = CGFloat(KeyboardSizing.bodyHeight(scale: keyboardScale, experience: experience)) / 4
    }
    if experience == .singleKeyboard, layout.itemRows.count == 4,
      let bottom = layout.itemRows.last, let template = bottom.first,
      let primary = bottom.first(where: { if case .primary = $0.action { return true }; return false }) {
      let toggle = bottom.first { if case .keyboardType = $0.action { return true }; return false }?.action
        ?? .keyboardType(.numeric)
      let actions: [(KeyboardAction, KeyboardLayout.ItemWidth)] = [
        (toggle, .percentage(0.123)), (.nextLocale, .percentage(0.123)),
        (.space, .available), (primary.action, .percentage(0.25)),
      ]
      layout.itemRows[3] = actions.map { action, width in
        .init(action: action, size: .init(width: width, height: template.size.height),
          edgeInsets: template.edgeInsets)
      }
    }
    return layout
  }

  private func keyboard(layout: KeyboardLayout) -> some View {
    KeyboardView(
      layout: layout, state: state, services: services,
      renderBackground: false,
      buttonContent: { item in
        if item.item.action == .nextLocale, experience == .singleKeyboard {
          Text(context.locale.identifier.hasPrefix("zh") ? "中" : "EN")
            .font(.system(size: 20, weight: .regular)).lineLimit(1)
        } else if item.item.action == .space, spaceHoldRecording {
          HStack(spacing: 4) {
            Image(systemName: "waveform")
            Text("按住说话").font(.system(size: 13, weight: .medium)).lineLimit(1).minimumScaleFactor(0.7)
          }
        } else if item.item.action == .space, context.isSpaceDragGestureActive {
          Text("移动光标").font(.callout)
        } else if case .primary = item.item.action {
          if let image = services.styleService.buttonImage(for: item.item.action) {
            image.font(.system(size: 24, weight: .semibold))
          } else if let title = services.styleService.buttonText(for: item.item.action), !title.isEmpty {
            Text(title)
              .font(.system(size: 20, weight: .semibold))
              .lineLimit(1)
              .minimumScaleFactor(0.8)
          } else {
            // The pinned library has empty primary-key text for some Chinese
            // locales. Keep a recognizable action instead of a blank blue key.
            Image(systemName: primarySymbol(for: item.item.action))
              .font(.system(size: 24, weight: .semibold))
          }
        } else if let text = fallbackTitle(for: item.item.action) {
          Text(text).font(.system(size: 16))
        } else { item.view }
      },
      buttonView: { item in
        if item.item.action == .space, spaceHoldRecording {
          // Keep the configured key hit-testing. The simultaneous observer only
          // supplies the terminal pointer; the action handler owns the result.
          item.view.keyboardButtonGestureConfiguration(.init(longPressDelay: 0.3))
            .simultaneousGesture(DragGesture(minimumDistance: 0, coordinateSpace: .global)
            .onChanged { owner?.spacePointerMoved($0.location) }
            .onEnded { owner?.spacePointerEnded($0.location) })
            .accessibilityIdentifier("keyboardSpace")
            .accessibilityLabel(owner?.spaceKeyAccessibilityLabel ?? "space")
        } else {
          Group {
            if case .primary = item.item.action {
              let enabled = owner?.primaryKeyEnabled ?? true
              item.view.disabled(!enabled).allowsHitTesting(enabled)
                .saturation(enabled ? 1 : 0).opacity(enabled ? 1 : 0.4)
            } else if case .shift = item.item.action {
              item.view.simultaneousGesture(TapGesture(count: 2).onEnded {
                services.actionHandler.handle(.capsLock)
              })
            } else { item.view }
          }
          .accessibilityIdentifier(identifier(for: item.item.action))
          .accessibilityLabel(label(for: item.item.action))
          .accessibilityValue(value(for: item.item.action))
        }
      },
      collapsedView: { $0.view },
      emojiKeyboard: { $0.view },
      toolbar: { _ in EmptyView() }
    )
    .keyboardState(state)
    .autocompleteToolbarStyle(.init(height: 0, padding: 0))
    .environment(\.locale, interfaceLanguage.locale)
  }
  /// Some bundled locale identifiers do not provide a primary-key title.
  /// Keep essential legends visible when the localized title is empty.
  private func fallbackTitle(for action: KeyboardAction) -> String? {
    guard action.standardButtonText(for: context)?.isEmpty != false else { return nil }
    switch action {
    case .space: return context.locale.identifier.hasPrefix("zh") ? "空格" : "space"
    case .keyboardType(.numeric): return "123"
    case .keyboardType(.symbolic): return "#+="
    case .keyboardType(.alphabetic): return "ABC"
    default: return nil
    }
  }
  private func identifier(for action: KeyboardAction) -> String {
    switch action {
    case .character(let value): "keyboardKey-" + value.lowercased()
    case .space: "keyboardSpace"
    case .nextLocale: "keyboardLanguageToggle"
    case .backspace: "keyboardBackspace"
    case .primary: "keyboardReturn"
    case .keyboardType(.numeric): "keyboardNumbers"
    case .keyboardType(.symbolic): "keyboardSymbols"
    case .keyboardType(.alphabetic): "keyboardLetters"
    case .shift: "keyboardShift"
    default: "keyboardAction-" + String(describing: action)
    }
  }
  private func primarySymbol(for action: KeyboardAction) -> String {
    guard case .primary(let type) = action else { return "return" }
    switch type {
    case .send: return "arrow.up"
    case .search: return "magnifyingglass"
    case .done, .ok: return "checkmark"
    case .next, .go, .continue: return "arrow.right"
    default: return "return"
    }
  }
  private func primaryTitle(for type: Keyboard.ReturnKeyType) -> String {
    switch type {
    case .send: return "发送"
    case .search: return "搜索"
    case .done, .ok: return "完成"
    case .next: return "下一项"
    case .go: return "前往"
    case .join: return "加入"
    case .continue: return "继续"
    case .custom(let title): return title
    default: return "换行"
    }
  }
  private func label(for action: KeyboardAction) -> String {
    let key: String = switch action {
    case .primary(let type): primaryTitle(for: type)
    case .backspace: "删除"
    case .nextLocale: "切换中英文"
    case .shift: "切换大小写"
    case .keyboardType(.numeric): "数字与标点"
    case .keyboardType(.symbolic): "更多符号"
    case .keyboardType(.alphabetic): "字母"
    default: action.accessibilityLabel ?? ""
    }
    return InterfaceLocalizer(language: interfaceLanguage).text(key)
  }
  private func value(for action: KeyboardAction) -> String {
    if action == .nextLocale {
      return InterfaceLocalizer(language: interfaceLanguage).text(context.locale.identifier.hasPrefix("zh") ? "中文全拼" : "English")
    }
    guard case .shift = action else { return "" }
    let current = context.keyboardCase == .auto ? context.preferredKeyboardCase : context.keyboardCase
    return InterfaceLocalizer(language: interfaceLanguage).text(current == .capsLocked ? "大写锁定" : current == .uppercased ? "大写" : "小写")
  }
}

/// Intercepts space recording and routes deletion to the voice panel timer.
/// The configured keyboard engine retains ownership of the other gestures.
nonisolated final class OpenLessKeyboardActionHandler: KeyboardAction.StandardActionHandler {
  private weak var owner: KeyboardViewController?
  @MainActor init(owner: KeyboardViewController) {
    self.owner = owner
    let services = owner.services
    let state = owner.state
    super.init(
      controller: owner, keyboardContext: state.keyboardContext,
      keyboardBehavior: services.keyboardBehavior,
      autocompleteContext: state.autocompleteContext,
      autocompleteService: services.autocompleteService,
      emojiContext: state.emojiContext, feedbackContext: state.feedbackContext,
      feedbackService: services.feedbackService,
      spaceDragGestureHandler: services.spaceDragGestureHandler
    )
  }
  nonisolated override func handle(_ gesture: Keyboard.Gesture, on action: KeyboardAction) {
    let controller = owner
    let handled = MainActor.assumeIsolated {
      guard let controller else { return false }
      return controller.handleLibraryDelete(gesture, action: action)
        || controller.handleSpaceGesture(gesture, action: action)
    }
    if !handled { super.handle(gesture, on: action) }
  }
  nonisolated override func shouldTriggerFeedback(for gesture: Keyboard.Gesture, on action: KeyboardAction) -> Bool {
    action != .dictation && super.shouldTriggerFeedback(for: gesture, on: action)
  }
  nonisolated override func shouldTriggerHapticFeedback(for gesture: Keyboard.Gesture, on action: KeyboardAction) -> Bool {
    feedbackContext.settings.isHapticFeedbackEnabled && keyboardContext.hasFullAccess
      && super.shouldTriggerHapticFeedback(for: gesture, on: action)
  }
  nonisolated override func handleDrag(on action: KeyboardAction, from start: CGPoint, to current: CGPoint) {
    let controller = owner
    let handled = MainActor.assumeIsolated { action == .space && controller?.spaceRecordingEnabled == true }
    if !handled { super.handleDrag(on: action, from: start, to: current) }
  }
}
