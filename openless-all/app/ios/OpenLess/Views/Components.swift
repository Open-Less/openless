import SwiftUI

enum OpenLessTheme {
    static let accent = Color(red: 37 / 255, green: 99 / 255, blue: 235 / 255)
    static let canvas = Color(uiColor: .systemGroupedBackground)
    static let surface = Color(uiColor: .secondarySystemGroupedBackground)
}

struct Surface<Content: View>: View {
    @ViewBuilder var content: Content

    var body: some View {
        content
            .padding(20)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(OpenLessTheme.surface, in: RoundedRectangle(cornerRadius: 22))
    }
}

struct WaveformView: View {
    var level: Double
    var active: Bool
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        HStack(alignment: .center, spacing: 4) {
            ForEach(0..<31, id: \.self) { index in
                let envelope = 0.25 + 0.75 * pow(sin(Double(index + 1) / 32 * .pi), 2)
                Capsule()
                    .fill(active ? OpenLessTheme.accent : Color.secondary.opacity(0.22))
                    .frame(maxWidth: .infinity)
                    .frame(height: active ? 5 + level * 53 * envelope : 5)
            }
        }
        .frame(height: 64)
        .animation(reduceMotion ? nil : .linear(duration: 0.1), value: level)
        .accessibilityHidden(true)
    }
}

struct EmptyMessage: View {
    var symbol: String
    var title: String
    var message: String

    var body: some View {
        VStack(spacing: 14) {
            Image(systemName: symbol).font(.system(size: 34, weight: .light)).foregroundStyle(OpenLessTheme.accent)
                .frame(width: 72, height: 72).background(OpenLessTheme.accent.opacity(0.08), in: RoundedRectangle(cornerRadius: 22))
            Text(title).font(.headline)
            Text(message).font(.subheadline).foregroundStyle(.secondary).multilineTextAlignment(.center)
        }
        .frame(maxWidth: .infinity).padding(.vertical, 28).padding(.horizontal, 12)
    }
}

struct ServiceTextField: View {
    var title: String
    @Binding var text: String
    var isURL = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title).font(.caption).foregroundStyle(.secondary)
            TextField(title, text: $text)
                .font(.subheadline.monospaced())
                .keyboardType(isURL ? .URL : .asciiCapable)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
        }
        .padding(.vertical, 4)
    }
}

func durationLabel(_ seconds: TimeInterval) -> String {
    let value = max(0, Int(seconds))
    return String(format: "%02d:%02d", value / 60, value % 60)
}

extension View {
    func editorError(_ message: Binding<String?>) -> some View {
        alert("无法保存", isPresented: Binding(get: { message.wrappedValue != nil }, set: {
            if !$0 { message.wrappedValue = nil }
        })) {
            Button("知道了", role: .cancel) { message.wrappedValue = nil }
        } message: { Text(message.wrappedValue ?? "") }
    }
}
