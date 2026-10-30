import OpenLessShared
import SwiftUI
import UIKit

struct HomeView: View {
  @ObservedObject var controller: AppController
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  @State private var copied = false

  private var phase: VoicePhase { controller.snapshot.phase }
  private var stateTitle: String {
    if controller.preparing { return "正在准备语音" }
    return switch phase {
    case .starting: "正在准备麦克风"
    case .recording: "正在聆听"
    case .processing: "thinking"
    case .awaitingInsertion: "正在输入"
    case .done: "转写完成"
    default: controller.speechPrepared ? "可以开始了" : "首次使用，先启用语音"
    }
  }
  private var hint: String {
    switch phase {
    case .starting: "还没开始录音，现在仍可取消。"
    case .recording: "轻点结束；按住说话时，松手结束、移出取消。"
    case .processing, .awaitingInsertion: "麦克风已释放，请稍候。"
    default: controller.speechPrepared ? "轻点开始，再点结束。也可以按住说话。" : "授权并准备识别资源，不会开始录音。"
    }
  }
  private var output: String {
    controller.resultText.isEmpty ? controller.snapshot.preview : controller.resultText
  }

  private var recording: Bool { phase == .recording }
  private var levelScale: CGFloat { reduceMotion || !recording ? 1 : 1 + 0.12 * CGFloat(min(1, max(0, controller.snapshot.level))) }
  private var statusColor: Color { recording ? AppDesign.danger : controller.speechPrepared ? AppDesign.accent : Color.secondary.opacity(0.45) }

  /// Primary action first: state, microphone control, hint and language live in one card.
  private var voiceCard: some View {
    VStack(spacing: 16) {
      HStack(spacing: 8) {
        Circle().fill(statusColor).frame(width: 7, height: 7).accessibilityHidden(true)
        Text(stateTitle).font(.subheadline.weight(.medium)).foregroundStyle(.primary)
          .contentTransition(.opacity)
        Spacer(minLength: 8)
        if phase.isBusy {
          Text(Duration.seconds(Double(controller.snapshot.elapsedMilliseconds) / 1000)
            .formatted(.time(pattern: .minuteSecond))).monospacedDigit()
            .font(.subheadline).foregroundStyle(.secondary)
        } else {
          Label("中英混合自动识别", systemImage: "globe")
            .font(.subheadline).foregroundStyle(.secondary)
        }
      }.frame(minHeight: 44)
      ZStack {
        if controller.speechPrepared {
          NativeSpeakControl(
            phase: phase, level: controller.snapshot.level, enabled: controller.coreReady,
            onAction: { action in
              switch action {
              case .start: controller.start()
              case .stop: controller.stop()
              case .cancel: controller.cancel()
              }
            }
          ).frame(width: 230, height: 132)
        } else {
          Button {
            Task { await controller.prepareSpeech(allowDownload: false) }
          } label: {
            HStack(spacing: 9) {
              if controller.preparing { ProgressView().tint(Color(uiColor: .systemBackground)) }
              else { Image(systemName: "mic.fill").font(.system(size: 21)) }
              Text(controller.preparing ? "准备中…" : "启用语音").font(.subheadline.weight(.semibold))
            }.padding(.horizontal, 22).padding(.vertical, 17)
              .frame(minWidth: 157, minHeight: 60).fixedSize()
              .foregroundStyle(Color(uiColor: .systemBackground))
              .background(.primary, in: Capsule())
          }.buttonStyle(PressFeedbackStyle())
            .disabled(controller.preparing || !controller.coreReady)
            .accessibilityIdentifier("prepareSpeech")
        }
      }.frame(maxWidth: .infinity, minHeight: controller.speechPrepared ? 132 : 76)
        .background {
          Circle().fill(AppDesign.accentSoft).frame(width: 164, height: 164)
            .scaleEffect(levelScale).opacity(recording ? 1 : 0)
            .animation(.easeOut(duration: 0.1), value: levelScale)
            .accessibilityHidden(true)
        }
      Text(hint).font(.footnote).foregroundStyle(.secondary)
        .multilineTextAlignment(.center).fixedSize(horizontal: false, vertical: true)
        .contentTransition(.opacity)
      if controller.speechPrepared && !phase.isBusy {
        Button {
          Task { await controller.toggleKeyboardVoice() }
        } label: {
          Label(controller.keyboardVoiceEnabled ? "结束键盘语音会话" : "启用键盘语音",
            systemImage: controller.keyboardVoiceEnabled ? "keyboard.badge.checkmark" : "keyboard")
            .font(.footnote.weight(.medium)).frame(minHeight: 36)
        }.buttonStyle(.bordered).buttonBorderShape(.capsule)
          .disabled(controller.preparing || controller.restarting)
          .accessibilityIdentifier("toggleKeyboardVoice")
        if controller.keyboardVoiceReady {
          Text("可直接在其他应用录音，停止后释放麦克风。")
            .font(.caption).foregroundStyle(.secondary).multilineTextAlignment(.center)
        }
      }
      if phase.isBusy {
        Button("取消", role: .destructive) { controller.cancel() }
          .font(.footnote).frame(minHeight: 44).transition(.opacity)
      }
    }.frame(maxWidth: .infinity).productCard(padding: 16)
      .animation(ProductMotion.resolve(ProductMotion.quick, reduce: reduceMotion), value: phase)
  }

  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: AppDesign.Space.section) {
        HStack(spacing: 10) {
          OpenLessMark(size: 34)
          Text("OpenLess").font(.system(size: 22, weight: .semibold)).tracking(-0.6)
          Spacer()
          NavigationLink(value: HomeRoute.installation) {
            Image(systemName: "keyboard.badge.ellipsis")
              .font(.system(size: 19, weight: .regular))
              .foregroundStyle(AppDesign.accent).frame(width: 44, height: 44)
              .background(AppDesign.accentSoft, in: Circle())
          }.accessibilityLabel("添加与切换键盘").accessibilityIdentifier("keyboardSetup")
        }

        voiceCard

        if controller.errorMessage != nil {
          VStack(alignment: .leading, spacing: 12) {
            Label(controller.speechAuthorizationDenied ? "请允许麦克风访问" : "语音暂时不可用，请重试。", systemImage: "exclamationmark.circle")
              .font(.footnote).fixedSize(horizontal: false, vertical: true)
              .accessibilityIdentifier("speechPreparationError")
            if controller.speechAuthorizationDenied {
              Button {
                UIApplication.shared.open(URL(string: UIApplication.openSettingsURLString)!)
              } label: { Label("前往系统设置", systemImage: "arrow.up.right") }.font(.footnote.weight(.semibold)).accessibilityIdentifier("openSpeechSettings")
            }
          }.frame(maxWidth: .infinity, alignment: .leading).productCard()
            .transition(reduceMotion ? .opacity : .opacity.combined(with: .move(edge: .top)))
        }
        if !output.isEmpty {
          VStack(alignment: .leading, spacing: 16) {
            HStack {
              Text(phase.isBusy ? "实时转写" : "识别结果").font(.subheadline.weight(.semibold))
              Spacer()
              Button {
                UIPasteboard.general.string = output
                copied = true
              } label: { Label(copied ? "已复制" : "复制", systemImage: copied ? "checkmark" : "doc.on.doc") }.font(.footnote.weight(.medium)).contentTransition(.opacity)
            }
            Text(output).font(.body).lineSpacing(5).textSelection(.enabled)
              .frame(maxWidth: .infinity, alignment: .leading)
          }.productCard().transition(reduceMotion ? .opacity : .opacity.combined(with: .move(edge: .top)))
        }

        OverviewMetricsView(library: controller.library)

        NavigationLink(value: HomeRoute.keyboard) {
          ProductRow(title: "试用键盘", subtitle: "中文全拼与英文，即刻输入", symbol: "keyboard")
            .productCard()
        }.buttonStyle(PressFeedbackStyle()).accessibilityIdentifier("openKeyboardTrial")
        HStack(alignment: .top, spacing: 9) {
          Image(systemName: "info.circle").padding(.top, 1)
          Text("启用键盘语音后，可在其他应用中录音")
            .lineLimit(1).minimumScaleFactor(0.8)
        }.font(.footnote).foregroundStyle(.secondary).lineSpacing(3)
          .accessibilityElement(children: .combine).accessibilityIdentifier("keyboardVoiceBoundary")
      }.padding(.horizontal, AppDesign.Space.page).padding(.top, 12).padding(.bottom, 28)
    }.background(AppDesign.canvas).toolbar(.hidden, for: .navigationBar)
      .animation(ProductMotion.resolve(ProductMotion.standard, reduce: reduceMotion), value: controller.errorMessage != nil)
      .animation(ProductMotion.resolve(ProductMotion.standard, reduce: reduceMotion), value: output.isEmpty)
      .task(id: copied) {
        guard copied else { return }
        do { try await Task.sleep(for: .seconds(2)) } catch { return }
        copied = false
      }
  }
}
