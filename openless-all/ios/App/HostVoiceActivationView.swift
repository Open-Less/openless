import OpenLessShared
import SwiftUI
import UIKit

struct HostVoiceActivationView: View {
  @ObservedObject var controller: AppController
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  var body: some View {
    NavigationStack {
      ScrollView {
        VStack(spacing: 16) {
          if let request = controller.hostActivation {
            Label(request.intent.kind == .translation ? "翻译录音" : request.intent.kind == .helpWrite ? "描述要写的内容" : "录音",
              systemImage: "mic.fill").font(.headline)
            if let target = request.intent.targetLanguage { Text(verbatim: target).foregroundStyle(.secondary) }
            if request.phase == .ready, let text = request.text {
              HostActivationText(text: text).frame(height: 180)
              Text("返回原应用后，结果会自动输入当前输入框。")
                .font(.subheadline).foregroundStyle(.secondary)
              Button("复制结果") { UIPasteboard.general.string = text }.buttonStyle(.bordered)
            } else if request.phase == .starting || request.phase == .recording || request.phase == .processing {
              NativeSpeakControl(phase: controller.snapshot.phase, level: controller.snapshot.level,
                enabled: controller.snapshot.phase == .recording,
                onAction: { action in if action == .cancel { controller.cancel() } else { controller.stop() } })
                .frame(width: 176, height: 120)
              Text(controller.snapshot.phase == .recording ? LocalizedStringKey("再次点击以完成")
                : request.phase == .processing ? LocalizedStringKey("thinking") : LocalizedStringKey("正在准备…"))
                .font(.subheadline).foregroundStyle(.secondary)
              Button("取消", role: .cancel) { controller.cancel() }
            } else if request.phase == .requested, request.startOnOpen == true {
              ProgressView("正在准备…")
            } else {
              Text("录音准备完成后将返回原应用。")
                .font(.subheadline).foregroundStyle(.secondary)
              Button(controller.preparing ? "准备中…" : "开始录音") {
                Task { await controller.startHostActivation() }
              }.buttonStyle(.borderedProminent)
                .disabled(controller.preparing || !controller.coreReady || controller.styleLibraryActivityBusy)
                .accessibilityIdentifier("hostActivationRecord")
            }
            if request.phase == .failed {
              Label("本次录音未完成，请重试。", systemImage: "exclamationmark.circle").foregroundStyle(.secondary)
            }
            if controller.errorMessage != nil { Text("语音暂时不可用，请重试。").font(.footnote).foregroundStyle(.secondary) }
          }
        }.frame(maxWidth: .infinity).padding(AppDesign.Space.page)
          .animation(ProductMotion.resolve(ProductMotion.standard, reduce: reduceMotion), value: controller.hostActivation?.phase)
      }.navigationTitle("键盘语音").navigationBarTitleDisplayMode(.inline)
        .toolbar { ToolbarItem(placement: .cancellationAction) {
          Button("关闭") { controller.closeHostActivation() }
        } }
    }.interactiveDismissDisabled(controller.snapshot.phase.isBusy)
      .presentationDetents([.medium, .large])
      .presentationDragIndicator(.visible)
  }
}

private struct HostActivationText: UIViewRepresentable {
  let text: String
  func makeUIView(context: Context) -> UITextView {
    let view = UITextView()
    view.isEditable = false; view.isSelectable = true
    view.font = .preferredFont(forTextStyle: .body)
    view.adjustsFontForContentSizeCategory = true; view.backgroundColor = .clear
    return view
  }
  func updateUIView(_ view: UITextView, context: Context) { if view.text != text { view.text = text } }
}
