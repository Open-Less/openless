import Combine
import SwiftUI
import UIKit

/// The parent owns Core/reloads and supplies all speech/control busy state.
/// This consumer never opens Core, performs OAuth HTTP, or configures sync.
struct AccountSettingsView: View {
  let core: CoreBridge?
  let isBusy: Bool
  @Environment(\.scenePhase) private var scenePhase
  @Environment(\.openURL) private var openURL
  @StateObject private var account: AccountSettings
  @State private var visible = false
  @State private var confirmingSignOut = false
  private let timer = Timer.publish(every: 1, on: .main, in: .common).autoconnect()

  init(core: CoreBridge?, isBusy: Bool) {
    self.core = core; self.isBusy = isBusy
    _account = StateObject(wrappedValue: AccountSettings(core: core))
  }
  var body: some View {
    List {
      Section {
        AccountProfileHeader(status: account.status)
      }.listRowInsets(EdgeInsets()).listRowBackground(Color.clear)
      if !account.ready {
        Section { Text("账户暂时无法载入。")
          .accessibilityIdentifier("accountUnavailable") }
      }
      Section("GitHub") {
        authentication
        if let code = account.userCode {
          HStack {
            Text(verbatim: code).font(.title2.monospaced().weight(.semibold))
              .fixedSize(horizontal: false, vertical: true)
              .textSelection(.enabled)
              .accessibilityLabel("GitHub 登录代码：" + code)
              .accessibilityIdentifier("accountUserCode")
            Spacer()
            Button { UIPasteboard.general.string = code } label: {
              Image(systemName: "doc.on.doc")
            }.accessibilityLabel("复制登录代码").accessibilityIdentifier("accountCopyCode")
          }
          Text("请在 GitHub 输入此代码；\(account.secondsRemaining) 秒后过期。")
            .font(.subheadline).foregroundStyle(.secondary)
          Button("打开 GitHub 授权网页") {
            // Never navigate to a server-supplied URL, even if it is HTTPS.
            if let url = URL(string: AccountAuthorization.trustedVerificationURL) { openURL(url) }
          }.accessibilityIdentifier("accountOpenGitHub")
        }
        if account.hasFlow {
          if account.pollingPaused {
            Text("登录检查已暂停。")
              .font(.subheadline).foregroundStyle(.secondary).accessibilityIdentifier("accountPollingPaused")
          }
          if account.needsPollRetry {
            Button("重试登录检查") { account.retryPolling() }
              .disabled(account.blocked).accessibilityIdentifier("accountRetryPoll")
          }
          Button("取消登录", role: .cancel) { Task { await account.cancelSignIn() } }
            .disabled(!account.canCancel).accessibilityIdentifier("accountCancelSignIn")
        } else if account.status?.profileLogin == nil {
          Button("登录 GitHub") { Task { await account.beginSignIn() } }
            .disabled(account.blocked).accessibilityIdentifier("accountBeginSignIn")
        }
      }.buttonStyle(.borderless)
      Section("同步") {
        Label("同步暂不可用", systemImage: "icloud")
          .foregroundStyle(.secondary)
          .accessibilityIdentifier("accountSyncUnavailable")
      }
      if account.operation != nil || account.cancelRequested || account.signOutRequested || isBusy || account.error != nil || account.notice != nil {
        Section {
          if let operation = account.operation {
            ProgressView(LocalizedStringKey(operationTitle(operation))).accessibilityIdentifier("accountWorking")
          }
          if isBusy { Text("当前任务完成后即可继续。").font(.subheadline).foregroundStyle(.secondary) }
          if account.cancelRequested || account.signOutRequested {
            Text("正在等待当前操作结束…").font(.subheadline).foregroundStyle(.secondary)
          }
          if account.error != nil {
            Text("暂时未能完成，请稍后重试。")
              .font(.subheadline).foregroundStyle(.secondary).accessibilityIdentifier("accountError")
          }
          if let notice = account.notice {
            Text(LocalizedStringKey(notice)).font(.subheadline).foregroundStyle(.secondary)
              .accessibilityIdentifier("accountNotice")
          }
        }
      }
      if account.hasFlow || account.status?.authentication == .unverified || account.status?.profileLogin != nil {
        Section {
          Button("退出账户…", role: .destructive) { confirmingSignOut = true }
            .disabled(!account.ready || account.activityBusy || account.signOutRequested)
            .accessibilityIdentifier("accountSignOut")
        } footer: {
          Text("本地词典、历史和设置会保留。")
        }
      }
    }
    .listStyle(.insetGrouped).navigationTitle("我的主页").navigationBarTitleDisplayMode(.inline)
    .toolbar {
      ToolbarItem(placement: .navigationBarTrailing) {
        Button { Task { await account.load() } } label: { Image(systemName: "arrow.clockwise") }
          .disabled(account.blocked).accessibilityLabel("刷新账户状态").accessibilityIdentifier("accountRefresh")
      }
    }
    .task(id: core.map(ObjectIdentifier.init)) {
      account.connect(core); account.setActivityBusy(isBusy)
      account.setLifecycle(visible: visible, foreground: scenePhase == .active)
      await account.load() // Local status only: never automatic token verification.
    }
    .onAppear { visible = true; account.setLifecycle(visible: true, foreground: scenePhase == .active) }
    .onDisappear { visible = false; account.setLifecycle(visible: false, foreground: false) }
    .onChange(of: scenePhase) { account.setLifecycle(visible: visible, foreground: $0 == .active) }
    .onChange(of: isBusy) { account.setActivityBusy($0) }
    .onReceive(timer) { _ in
      guard visible, scenePhase == .active else { return }
      // Independent operation task: cancelling a view task cannot imply that a
      // dispatched Core HTTP/persistence operation was cancelled.
      Task { await account.tick() }
    }
    .confirmationDialog("退出 GitHub 账户？", isPresented: $confirmingSignOut, titleVisibility: .visible) {
      Button("退出账户，保留本地数据", role: .destructive) { Task { await account.confirmSignOut() } }
      Button("取消", role: .cancel) {}
    } message: { Text("本地词典、历史和设置会保留。") }
  }
  @ViewBuilder private var authentication: some View {
    if let status = account.status {
      switch status.authentication {
      case .signedOut: Label("未登录", systemImage: "person.crop.circle").accessibilityIdentifier("accountSignedOut")
      case .unverified:
        Label("点击登录以继续", systemImage: "person.crop.circle.badge.questionmark")
          .accessibilityIdentifier("accountUnverified")
      case .verifiedLogin(let login):
        Label { Text("已登录 · \(login)") } icon: { Image(systemName: "person.crop.circle.badge.checkmark") }
          .accessibilityIdentifier("accountVerifiedLogin")
      case .unsupported:
        Label("账户暂时不可用", systemImage: "questionmark.circle").accessibilityIdentifier("accountUnsupported")
      }
    } else { Text("正在载入…").accessibilityIdentifier("accountNotLoaded") }
  }
  private func operationTitle(_ operation: AccountSettings.Operation) -> String {
    switch operation {
    case .loading: "正在读取账户状态…"
    case .starting: "正在准备 GitHub 登录…"
    case .polling: "正在检查 GitHub 授权…"
    case .cancelling: "正在取消登录…"
    case .signingOut: "正在退出账户…"
    }
  }
}
