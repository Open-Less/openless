import OpenLessShared
import SwiftUI
import UIKit

struct SettingsView: View {
  @ObservedObject var controller: AppController
  var onOpenStyles: (() -> Void)? = nil
  @State private var query = ""
  @State private var showDeveloperUnlock = false
  @StateObject private var profile: AccountSettings
  init(controller: AppController, onOpenStyles: (() -> Void)? = nil) {
    self.controller = controller
    self.onOpenStyles = onOpenStyles
    _profile = StateObject(wrappedValue: AccountSettings(core: controller.styleLibraryCore))
  }
  @EnvironmentObject private var interfacePreferences: InterfacePreferences
  var body: some View {
    List {
      if matches("键盘 高度 大小 添加 默认面板 默认打字布局 中英混合 自动识别 免跳转 后台保活 长按空格 轻触反馈 按键声音 单一 体验 输出语言 翻译 keyboard experience translation height size key sound keyboard typing layout mixed language automatic recognition no jump background keep alive haptics") {
      Section("键盘") {
        NavigationLink {
          KeyboardInstallationView(controller: controller)
        } label: {
          SettingsLabel("添加 OpenLess 键盘", symbol: "keyboard.badge.ellipsis")
        }
        Picker(selection: $controller.preferences.experience) {
          ForEach(KeyboardExperience.allCases, id: \.self) {
            Text(interfacePreferences.localizer.text($0.title)).tag($0)
          }
        } label: { SettingsLabel("键盘体验", symbol: "keyboard") }
          .pickerStyle(.menu).accessibilityIdentifier("keyboardExperiencePicker")
          .disabled(controller.styleLibraryActivityBusy)
        if controller.preferences.experience == .voiceFocused {
          Picker(selection: $controller.preferences.appearance) {
            ForEach(KeyboardAppearance.allCases, id: \.self) { Text($0.title).tag($0) }
          } label: { SettingsLabel("默认面板", symbol: "rectangle.grid.1x2") }.pickerStyle(.menu).accessibilityIdentifier("appearancePicker")
        } else {
          Text("中英打字共用一个键盘。按住空格说话，上滑选择撤回、翻译或发送。")
            .font(.caption).foregroundStyle(.secondary)
          Picker(selection: $controller.preferences.translationOutputLanguage) {
            Text("选择目标语言").tag(String?.none)
            if let selected = controller.preferences.translationOutputLanguage,
              !translationTargets.contains(where: { $0.id == selected }) {
              Text("目标语言已不可用").tag(Optional(selected))
            }
            ForEach(translationTargets, id: \.id) { Text(verbatim: $0.title).tag(Optional($0.id)) }
          } label: { SettingsLabel("翻译输出语言", symbol: "character.bubble") }
            .pickerStyle(.menu).accessibilityIdentifier("keyboardOutputLanguagePicker")
            .disabled(controller.styleLibraryActivityBusy)
          Text("一次只输出一种语言。录音时滑到中间翻译区，松手后直接翻译为所选语言。")
            .font(.caption).foregroundStyle(.secondary)
          if controller.snapshot.configuredVoiceCapabilities?.canTranslate != true || translationTargets.isEmpty {
            Text("请先在风格与翻译中添加目标语言并配置翻译模型。")
              .font(.caption).foregroundStyle(.secondary)
            if let onOpenStyles { Button("配置翻译", action: onOpenStyles) }
          }
        }
        Picker(selection: $controller.preferences.language) {
          ForEach(KeyboardLanguage.allCases, id: \.self) { Text($0.title).tag($0) }
        } label: { SettingsLabel("默认打字布局", symbol: "globe") }.pickerStyle(.menu).disabled(controller.snapshot.phase.isBusy || controller.preparing)
        VStack(alignment: .leading, spacing: 8) {
          HStack {
            SettingsLabel("键盘高度", symbol: "arrow.up.and.down")
            Spacer()
            Text("\(Int((KeyboardSizing.validScale(controller.preferences.keyboardScale) * 100).rounded()))%")
              .monospacedDigit().foregroundStyle(.secondary)
          }
          Slider(value: $controller.preferences.keyboardScale, in: KeyboardSizing.scaleRange, step: 0.05)
            .accessibilityLabel("键盘高度").accessibilityIdentifier("keyboardHeightSlider")
          Button("恢复推荐大小") { controller.preferences.keyboardScale = 1 }
            .font(.footnote).accessibilityIdentifier("resetKeyboardHeight")
        }
        if controller.preferences.experience == .voiceFocused {
          Picker(selection: $controller.preferences.spaceHold) {
            ForEach(SpaceHoldAction.allCases, id: \.self) { Text($0.title).tag($0) }
          } label: { SettingsLabel("长按空格", symbol: "space") }.pickerStyle(.menu).accessibilityIdentifier("spaceHoldPicker")
        }
        Toggle(isOn: $controller.preferences.haptics) { SettingsLabel("轻触反馈", symbol: "hand.tap") }
        Toggle(isOn: $controller.preferences.keySound) { SettingsLabel("按键声音", symbol: "speaker.wave.2") }
        Toggle(isOn: $controller.preferences.backgroundKeepAlive) {
          SettingsLabel("免跳转后台保活", symbol: "pip")
        }.accessibilityIdentifier("backgroundKeepAliveToggle")
        Text(controller.keepAlive.explanation)
          .font(.caption).foregroundStyle(.secondary)
          .fixedSize(horizontal: false, vertical: true)
      }
      }
      if matches("风格 翻译 界面语言 外观 主题 深色 style translation appearance language") {
      Section {
        if let onOpenStyles {
          Button(action: onOpenStyles) { SettingsLabel("风格与翻译", symbol: "sparkles") }
            .foregroundStyle(.primary).accessibilityIdentifier("openStyleLibrary")
        }
        NavigationLink { InterfacePreferencesView(preferences: interfacePreferences) } label: {
          SettingsLabel("界面语言与外观", symbol: "paintbrush")
        }.accessibilityIdentifier("openInterfacePreferences")
      }
      }
      if matches("账户 账号 登录 GitHub 同步 account profile") {
      Section {
        NavigationLink { AccountSettingsDestination(controller: controller) } label: {
          AccountProfileHeader(status: profile.status, compact: true)
        }.accessibilityIdentifier("openAccountSettings")
      }
      }
      if matches("系统录音 其他 App 控制中心 recording") {
      Section("系统录音") {
        DisclosureGroup { SystemRecordingInstructions() } label: { SettingsLabel("在其他 App 中录音", symbol: "mic.badge.plus") }
      }
      }
      if matches("语音识别 准备 识别方式 speech recognition") {
      Section("语音识别") {
        AdaptiveRow(title: controller.speechPrepared ? "识别已就绪" : "准备语音识别", icon: "waveform") {
          Button(controller.preparing ? "准备中…" : "准备") {
            Task { await controller.prepareSpeech(allowDownload: false) }
          }
          .disabled(controller.preparing || controller.snapshot.phase.isBusy || !controller.coreReady)
        }
        DisclosureGroup {
          Text(interfacePreferences.localizer.asrDescription(controller.asrTitle, developerMode: controller.developerMode))
            .font(.subheadline).foregroundStyle(.secondary)
        } label: { SettingsLabel("识别方式", symbol: "cpu") }
      }
      }
      if matches("帮助 关于 版本说明 隐私 数据 开源许可 开发者模式 help privacy license developer") {
      Section("帮助与关于") {
        NavigationLink { HelpView() } label: {
          SettingsLabel("帮助中心", symbol: "questionmark.circle")
        }.accessibilityIdentifier("openHelp")
        NavigationLink { ReleaseNotesView() } label: {
          SettingsLabel("版本说明", symbol: "list.bullet.rectangle")
        }.accessibilityIdentifier("openReleaseNotes")
        NavigationLink { PrivacyView() } label: {
          SettingsLabel("隐私与数据", symbol: "lock.shield")
        }
        NavigationLink { LicenseView() } label: {
          SettingsLabel("开源许可", symbol: "doc.text")
        }
        if controller.developerMode {
          NavigationLink { DeveloperSettingsView(controller: controller) } label: {
            SettingsLabel("开发者模式", symbol: "curlybraces")
          }.accessibilityIdentifier("openDeveloperSettings")
        }
      }
      }
      if matches("版本 OpenLess version") {
      Section {
        Button {
          if controller.registerVersionTap() { showDeveloperUnlock = true }
        } label: {
          Text("OpenLess " + (Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? ""))
            .font(.footnote).foregroundStyle(.secondary)
            .frame(maxWidth: .infinity, minHeight: 44)
        }.buttonStyle(.plain).accessibilityIdentifier("appVersion")
      }
      }
    }
    .listStyle(.insetGrouped).scrollContentBackground(.hidden).background(AppDesign.canvas)
    .navigationTitle("设置").navigationBarTitleDisplayMode(.inline)
    .safeAreaInset(edge: .top, spacing: 0) {
      PinnedSearchBar(text: $query, placeholder: "搜索设置", identifier: "settingsSearch")
    }
    .task(id: controller.styleLibraryCore.map(ObjectIdentifier.init)) {
      profile.connect(controller.styleLibraryCore)
      await profile.load()
    }
    .alert(controller.developerMode ? "退出开发者模式？" : "开启开发者模式？", isPresented: $showDeveloperUnlock) {
      Button("取消", role: .cancel) {}
      Button(controller.developerMode ? "重载并关闭" : "重载并开启") {
        Task { await controller.reloadDeveloperMode(!controller.developerMode) }
      }
    } message: {
      Text("开发者模式可配置语音、文字模型与 API；切换时会重新加载应用。")
    }
  }
  private func matches(_ keywords: String) -> Bool {
    let term = query.trimmingCharacters(in: .whitespacesAndNewlines)
    return term.isEmpty || keywords.localizedStandardContains(term)
  }
  private var translationTargets: [KeyboardVoiceOption] {
    controller.snapshot.configuredVoiceCapabilities?.translationTargets ?? []
  }
}

private struct AccountSettingsDestination: View {
  @ObservedObject var controller: AppController
  @ObservedObject var productLibrary: ProductLibrary
  @ObservedObject var developer: DeveloperSettings
  init(controller: AppController) {
    self.controller = controller
    self.productLibrary = controller.library
    self.developer = controller.developer
  }
  var body: some View {
    AccountSettingsView(core: controller.styleLibraryCore,
      isBusy: controller.styleLibraryActivityBusy || productLibrary.busy || developer.busy)
  }
}

/// One destination is shared by the bottom tab and the Settings shortcut.
struct StyleLibrarySettingsDestination: View {
  @ObservedObject var controller: AppController
  @ObservedObject var productLibrary: ProductLibrary
  @ObservedObject var developer: DeveloperSettings
  let onOpenStyles: (() -> Void)?
  let section: Binding<StyleLibrarySection>?
  @State private var localSection = StyleLibrarySection.library
  @SceneStorage("openless.resumeStyleModelSetup") private var resumeModelSetup = false
  @State private var showModels = false
  @State private var showEnableModels = false
  @State private var loadingModels = false
  @State private var configurationRevision = 0
  init(controller: AppController, onOpenStyles: (() -> Void)? = nil, section: Binding<StyleLibrarySection>? = nil) {
    self.controller = controller
    self.onOpenStyles = onOpenStyles
    self.section = section
    self.productLibrary = controller.library
    self.developer = controller.developer
  }
  var body: some View {
    StyleLibraryView(core: controller.styleLibraryCore,
      isBusy: controller.styleLibraryActivityBusy || productLibrary.busy || developer.busy,
      section: section ?? $localSection, configurationRevision: configurationRevision,
      onConfigureModels: {
        if controller.developerMode { openModelsWithFreshSnapshot() }
        else { showEnableModels = true }
      })
      .sheet(isPresented: $showModels, onDismiss: { configurationRevision += 1 }) {
        NavigationStack {
          DeveloperSettingsView(controller: controller)
            .toolbar { ToolbarItem(placement: .cancellationAction) { Button("完成") { showModels = false } } }
        }
      }
      .alert("开启开发者模式？", isPresented: $showEnableModels) {
        Button("取消", role: .cancel) {}
        Button("开启并配置") {
          resumeModelSetup = true
          onOpenStyles?()
          Task {
            await controller.reloadDeveloperMode(true)
            if !controller.developerMode { resumeModelSetup = false }
          }
        }
      } message: {
        Text("添加文字模型后，即可使用翻译和 AI 风格。")
      }
      .onAppear { resumePendingModelSetup() }
      .onChange(of: controller.coreReady) { _ in resumePendingModelSetup() }
      .onChange(of: controller.restarting) { _ in resumePendingModelSetup() }
  }
  private func openModelsWithFreshSnapshot() {
    guard !loadingModels else { return }
    loadingModels = true
    let epoch = controller.runtimeID
    Task {
      await developer.refresh()
      loadingModels = false
      guard controller.runtimeID == epoch, controller.developerMode, !controller.restarting else { return }
      showModels = true
    }
  }
  private func resumePendingModelSetup() {
    guard resumeModelSetup, controller.developerMode, controller.coreReady, !controller.restarting else { return }
    resumeModelSetup = false
    openModelsWithFreshSnapshot()
  }
}

struct KeyboardInstallationView: View {
  @ObservedObject var controller: AppController
  var body: some View {
    List {
      Section("添加步骤") {
        Label("打开系统设置 → 通用 → 键盘", systemImage: "1.circle")
        Label("添加新键盘 → OpenLess", systemImage: "2.circle")
        Label("输入时长按地球键，选择 OpenLess", systemImage: "3.circle")
        Button {
          UIApplication.shared.open(URL(string: UIApplication.openSettingsURLString)!)
        } label: {
          Label("打开系统设置", systemImage: "arrow.up.right")
        }.accessibilityIdentifier("openSystemSettings")
      }
      Section("录音与插入") {
        SystemRecordingInstructions()
      }
      Section {
        Label("基础打字无需完全访问", systemImage: "keyboard")
        DisclosureGroup("完全访问的用途") {
          Text("语音控制与键盘偏好共享需要完全访问；中文和英文基础输入不需要。")
          Text("完全访问用于键盘与 App 共享录音状态和结果，不会自动开始录音。")
            .foregroundStyle(.secondary)
        }
        DisclosureGroup("连接状态") {
          Label(controller.groupAvailable ? "已连接" : "暂未连接",
            systemImage: controller.groupAvailable ? "checkmark.circle" : "exclamationmark.circle")
            .font(.subheadline).foregroundStyle(.secondary)
        }
      }
    }
    .listStyle(.insetGrouped).navigationTitle("添加键盘").navigationBarTitleDisplayMode(.inline)
    .toolbar(.visible, for: .navigationBar)
  }
}

struct SettingsLabel: View {
  let title: String
  let symbol: String
  init(_ title: String, symbol: String) { self.title = title; self.symbol = symbol }
  var body: some View {
    HStack(spacing: 12) {
      IconTile(symbol: symbol, size: 28)
      Text(LocalizedStringKey(title))
    }.frame(minHeight: 36)
  }
}

/// Instructions only: adding a system control remains an explicit system action.
struct SystemRecordingInstructions: View {
  var body: some View {
    if #available(iOS 18.0, *) {
      Text("控制中心 → 添加控制 → OpenLess 录音")
      Text("首次使用请在 App 中授权麦克风并准备语音。")
        .font(.subheadline).foregroundStyle(.secondary)
      Text("停止录音后，文字会自动输入到当前输入框。")
        .font(.subheadline).foregroundStyle(.secondary)
    } else {
      Text("在 App 中录音，完成后文字会自动输入到当前输入框。")
    }
    Text("首次启用或后台会话过期时，键盘会短暂打开 OpenLess 并自动返回；其余情况直接在键盘内录音。")
      .font(.footnote).foregroundStyle(.secondary)
  }
}

private struct PrivacyView: View {
  var body: some View {
    Form {
      Section("声音") {
        Label("只在你主动录音时采集", systemImage: "mic")
        Label("识别未完成时保留可恢复录音", systemImage: "waveform")
        Text("录音保存在本机。识别成功后清理；未完成的录音可播放、导出或随历史删除，不参与词典同步。")
          .font(.footnote).foregroundStyle(.secondary)
      }
      Section("输入与文字") {
        Label("不自动读写剪贴板", systemImage: "doc.on.clipboard")
        Label("键盘不联网，不上传输入内容", systemImage: "keyboard")
        Text("普通模式在本机识别。使用自配模型时，会向所选提供商发送你主动录制的声音或请求处理的文字。历史保存在本机；点击“复制”才写入剪贴板。")
          .font(.footnote).foregroundStyle(.secondary)
      }
      Section("系统限制") {
        Text("第三方键盘不能用于系统密码框；部分应用可能禁止第三方键盘。Apple 的语音能力与资源由系统管理。")
          .font(.subheadline)
      }
    }.navigationTitle("隐私与数据").navigationBarTitleDisplayMode(.inline)
  }
}

private struct LicenseView: View {
  let names = ["KeyboardKit", "sherpa-onnx", "onnxruntime", "librime", "rime-ice", "SymSpell", "prediction-data", "pinyin-simp", "boost", "marisa-trie", "yaml-cpp", "leveldb", "opencc", "rapidjson", "darts", "OpenLess"]
  var body: some View {
    List(names, id: \.self) { name in
      NavigationLink(name) {
        ScrollView {
          let url = Bundle.main.url(forResource: name + "-LICENSE", withExtension: "txt", subdirectory: "Licenses")
          Text(url.flatMap { try? String(contentsOf: $0, encoding: .utf8) } ?? "请参阅工程中的许可文件。")
            .font(.body).textSelection(.enabled).padding()
        }.navigationTitle(name)
      }
    }.navigationTitle("开源许可").navigationBarTitleDisplayMode(.inline)
  }
}

