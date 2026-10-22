import SwiftUI
import UIKit

enum AppTheme: String, CaseIterable {
  case system, light, dark
  var title: String { switch self { case .system: "跟随系统"; case .light: "浅色"; case .dark: "深色" } }
  var scheme: ColorScheme? { switch self { case .system: nil; case .light: .light; case .dark: .dark } }
}

/// Shared colors for product screens and controls.
enum AppDesign {
  static let canvas = Color(uiColor: UIColor { traits in
    traits.userInterfaceStyle == .dark
      ? UIColor(red: 0.075, green: 0.079, blue: 0.09, alpha: 1)
      : UIColor(red: 0.975, green: 0.978, blue: 0.988, alpha: 1)
  })
  static let surface = Color(uiColor: .secondarySystemGroupedBackground)
  static let accent = Color(uiColor: UIColor { traits in
    traits.userInterfaceStyle == .dark
      ? UIColor(red: 0.44, green: 0.62, blue: 1, alpha: 1)
      : UIColor(red: 0.17, green: 0.37, blue: 0.93, alpha: 1)
  })
  static let outline = Color.primary.opacity(0.06)
  static let accentSoft = accent.opacity(0.10)
  static let success = Color(uiColor: .systemGreen)
  static let danger = Color(uiColor: .systemRed)
  /// 62pt dock + 8pt above + 8pt below.
  static let dockClearance: CGFloat = 78
  enum Space {
    static let page: CGFloat = 20
    static let section: CGFloat = 24
    static let card: CGFloat = 16
  }
  enum Radius {
    static let card: CGFloat = 18
    static let tile: CGFloat = 12
    static let icon: CGFloat = 10
  }
}

/// Shared motion tokens. Reduce Motion keeps only a short opacity change.
enum ProductMotion {
  static let quick = Animation.easeOut(duration: 0.16)
  static let standard = Animation.spring(response: 0.34, dampingFraction: 0.88)
  static let reduced = Animation.easeOut(duration: 0.12)
  static func resolve(_ animation: Animation, reduce: Bool) -> Animation { reduce ? reduced : animation }
}

/// Fades page content out behind the floating dock instead of letting it show through the glass.
struct DockScrim: View {
  var body: some View {
    LinearGradient(colors: [AppDesign.canvas.opacity(0), AppDesign.canvas.opacity(0.94), AppDesign.canvas],
      startPoint: .top, endPoint: .bottom)
      .frame(height: AppDesign.dockClearance + 28)
      .allowsHitTesting(false).accessibilityHidden(true)
      .ignoresSafeArea(edges: .bottom)
  }
}

struct IconTile: View {
  let symbol: String
  var size: CGFloat = 28
  var tint: Color = AppDesign.accent
  var body: some View {
    Image(systemName: symbol).font(.system(size: size * 0.5, weight: .medium))
      .foregroundStyle(tint).frame(width: size, height: size)
      .background(tint.opacity(0.10), in: RoundedRectangle(cornerRadius: size > 32 ? 12 : AppDesign.Radius.icon, style: .continuous))
      .accessibilityHidden(true)
  }
}

struct EmptyState: View {
  let symbol: String
  let title: String
  let message: String
  var body: some View {
    VStack(spacing: 10) {
      Image(systemName: symbol).font(.system(size: 24, weight: .regular))
        .foregroundStyle(AppDesign.accent).frame(width: 56, height: 56)
        .background(AppDesign.accentSoft, in: Circle()).accessibilityHidden(true)
      Text(LocalizedStringKey(title)).font(.headline)
      Text(LocalizedStringKey(message)).font(.footnote).foregroundStyle(.secondary)
        .multilineTextAlignment(.center).lineSpacing(3)
    }.frame(maxWidth: .infinity).padding(.vertical, 48)
  }
}

extension View {
  func productCard(padding: CGFloat = AppDesign.Space.card) -> some View {
    self.padding(padding)
      .background(AppDesign.surface, in: RoundedRectangle(cornerRadius: AppDesign.Radius.card, style: .continuous))
      .overlay(RoundedRectangle(cornerRadius: AppDesign.Radius.card, style: .continuous).strokeBorder(AppDesign.outline, lineWidth: 0.5))
  }
}

/// Feedback for occasional actions, not for individual keyboard keystrokes.
struct PressFeedbackStyle: ButtonStyle {
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  func makeBody(configuration: Configuration) -> some View {
    configuration.label
      .scaleEffect(configuration.isPressed && !reduceMotion ? 0.98 : 1)
      .opacity(configuration.isPressed ? 0.8 : 1)
      .animation(.easeOut(duration: 0.14), value: configuration.isPressed)
  }
}

struct OpenLessMark: View {
  var size: CGFloat = 40
  var body: some View {
    Image("OpenLessBrand").renderingMode(.original).resizable().scaledToFit()
      .frame(width: size, height: size)
      .clipShape(RoundedRectangle(cornerRadius: size * 0.22))
      .accessibilityHidden(true)
  }
}

struct ProductRow: View {
  let title: String
  let subtitle: String
  let symbol: String
  @Environment(\.dynamicTypeSize) private var typeSize
  private var icon: some View {
    Image(systemName: symbol).font(.system(size: 20, weight: .medium))
      .foregroundStyle(AppDesign.accent).frame(width: 44, height: 44)
      .background(AppDesign.accent.opacity(0.07), in: RoundedRectangle(cornerRadius: 14))
  }
  private var chevron: some View {
    Image(systemName: "chevron.right").font(.caption.weight(.semibold))
      .foregroundStyle(.tertiary).accessibilityHidden(true)
  }
  private var details: some View {
    VStack(alignment: .leading, spacing: 5) {
      Text(LocalizedStringKey(title)).font(.body.weight(.semibold)).foregroundStyle(.primary)
        .fixedSize(horizontal: false, vertical: true)
      Text(LocalizedStringKey(subtitle)).font(.footnote).foregroundStyle(.secondary)
        .fixedSize(horizontal: false, vertical: true)
    }
  }
  var body: some View {
    Group {
      if typeSize.isAccessibilitySize {
        VStack(alignment: .leading, spacing: 14) {
          HStack { icon; Spacer(); chevron }
          details
        }
      } else {
        HStack(spacing: 14) { icon; details; Spacer(minLength: 4); chevron }
      }
    }.frame(maxWidth: .infinity, alignment: .leading).contentShape(Rectangle())
  }
}

/// Labels and menu values use separate lines at accessibility sizes instead
/// of squeezing both into narrow, vertically broken columns.
struct AdaptiveRow<Content: View>: View {
  let title: String
  var icon: String? = nil
  var font: Font = .body
  @ViewBuilder var content: Content
  @Environment(\.dynamicTypeSize) private var typeSize
  @ViewBuilder private var titleView: some View {
    if let icon { SettingsLabel(title, symbol: icon).font(font) }
    else { Text(LocalizedStringKey(title)).font(font) }
  }
  var body: some View {
    if typeSize.isAccessibilitySize {
      VStack(alignment: .leading, spacing: 10) {
        titleView
        content.frame(maxWidth: .infinity, alignment: .leading)
      }
    } else {
      HStack {
        titleView
        Spacer()
        content
      }
    }
  }
}

struct SettingsCard<Content: View>: View {
  let title: String
  @ViewBuilder var content: Content
  var body: some View {
    VStack(alignment: .leading, spacing: 12) {
      Text(LocalizedStringKey(title)).font(.footnote.weight(.medium)).foregroundStyle(.secondary)
        .padding(.leading, 6).accessibilityAddTraits(.isHeader)
      VStack(alignment: .leading, spacing: 16) { content }.productCard()
    }
  }
}
