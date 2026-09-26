import SwiftUI
import UIKit

struct SettingsView: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.openURL) private var openURL
    @State private var draft: AppSettings
    @State private var credential: CredentialKind?
    @State private var configured: [CredentialKind: Bool] = [:]
    @State private var error: String?
    @State private var confirmClearKeyboard = false

    init(settings: AppSettings) { _draft = State(initialValue: settings) }

    var body: some View {
        Form {
            Section {
                HStack(spacing: 15) {
                    Image(systemName: "waveform.circle.fill").font(.system(size: 42)).foregroundStyle(OpenLessTheme.accent)
                    VStack(alignment: .leading, spacing: 5) {
                        Text("OpenLess for iOS").font(.headline)
                        Text("你的声音，你的表达方式。")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                }.padding(.vertical, 6)
            }
            Section {
                Picker("识别服务", selection: $draft.recognitionProvider) {
                    ForEach(RecognitionProvider.allCases) { Text($0.title).tag($0) }
                }
                Picker("识别语言", selection: $draft.speechLocale) {
                    Text("简体中文").tag("zh-CN")
                    Text("繁體中文").tag("zh-TW")
                    Text("English").tag("en-US")
                    Text("日本語").tag("ja-JP")
                    Text("한국어").tag("ko-KR")
                    Text("Français").tag("fr-FR")
                    Text("Deutsch").tag("de-DE")
                    Text("Español").tag("es-ES")
                }
                if draft.recognitionProvider == .apple {
                    Toggle("仅在设备上识别", isOn: $draft.onDeviceOnly)
                } else {
                    ServiceTextField(title: "转写服务地址", text: $draft.asrBaseURL, isURL: true)
                    ServiceTextField(title: "转写模型", text: $draft.asrModel)
                    credentialButton(.transcription)
                }
                Picker("单次录音上限", selection: $draft.recordingLimit) {
                    Text("30 秒").tag(30)
                    Text("55 秒").tag(55)
                    Text("2 分钟").tag(120)
                    Text("3 分钟").tag(180)
                }
            } header: { Text("语音转写") } footer: {
                Text(draft.recognitionProvider == .apple
                     ? "Apple 识别单次最多 55 秒。仅在设备上识别需要设备和语言支持；关闭后，Apple 可能通过网络处理音频。"
                     : "录音会发送到你配置的转写服务。使用支持 audio/transcriptions 的 HTTPS 接口，可填写基础地址或完整接口地址。")
            }
            Section {
                ServiceTextField(title: "润色服务地址", text: $draft.polishBaseURL, isURL: true)
                ServiceTextField(title: "润色模型", text: $draft.polishModel)
                credentialButton(.polishing)
                TextField("翻译目标语言", text: $draft.translationLanguage)
            } header: { Text("文字润色与翻译") } footer: {
                Text("支持 OpenAI 兼容的 chat/completions 接口。选择润色风格或翻译时，原文和词典会发送至该服务；“原文”且未开启翻译时不调用润色服务。API Key 仅存于系统钥匙串。")
            }
            Section("使用习惯") {
                Picker("外观", selection: $draft.appearance) {
                    ForEach(AppAppearance.allCases) { Text($0.title).tag($0) }
                }
                NavigationLink { KeyboardGuideView(showDoneButton: false) } label: {
                    Label("在其他应用中输入", systemImage: "keyboard")
                }
                Button {
                    if let url = URL(string: UIApplication.openSettingsURLString) { openURL(url) }
                } label: { Label("系统权限设置", systemImage: "hand.raised") }
            }
            Section {
                LabeledContent("历史记录", value: "\(model.document.history.count) 条")
                LabeledContent("个人词典", value: "\(model.document.vocabulary.count) 个词条")
                Button("清空键盘暂存", role: .destructive) { confirmClearKeyboard = true }
            } header: { Text("本机数据") } footer: {
                Text("历史、草稿、词典和风格保存在本机。只有主动点“发送到键盘”的文字会出现在键盘中，最多保留 10 条。暂存录音在成功转写并保存文字后删除。")
            }
            Section {
                LabeledContent("iOS 版本", value: "0.1.0")
                Link(destination: URL(string: "https://github.com/Open-Less/openless")!) {
                    Label("开源项目", systemImage: "arrow.up.right.square")
                }
            }
        }
        .navigationTitle("设置")
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button("保存") { save() }.fontWeight(.semibold)
                    .disabled(model.isBusy || draft == model.settings)
            }
        }
        .onAppear { refreshCredentials() }
        .onChange(of: model.settings) { previous, updated in
            if draft == previous { draft = updated }
            else {
                draft.selectedStyleID = updated.selectedStyleID
                draft.translationEnabled = updated.translationEnabled
            }
        }
        .sheet(item: $credential, onDismiss: { refreshCredentials() }) { kind in
            NavigationStack { CredentialEditor(kind: kind) }
        }
        .confirmationDialog("清空所有键盘暂存文字？", isPresented: $confirmClearKeyboard, titleVisibility: .visible) {
            Button("清空暂存", role: .destructive) { model.clearKeyboard() }
        } message: { Text("主应用的历史和草稿会继续保留。") }
        .editorError($error)
    }

    private func credentialButton(_ kind: CredentialKind) -> some View {
        Button { credential = kind } label: {
            HStack {
                Label(kind.title, systemImage: "key")
                Spacer()
                Text(configured[kind].map { $0 ? "已保存" : "未设置" } ?? "不可读取")
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
    }

    private func refreshCredentials() {
        for kind in CredentialKind.allCases {
            do { configured[kind] = !(try KeychainStore.read(kind)).isEmpty }
            catch { configured.removeValue(forKey: kind); self.error = error.localizedDescription }
        }
    }

    private func save() {
        do {
            draft.asrModel = draft.asrModel.trimmingCharacters(in: .whitespacesAndNewlines)
            draft.polishModel = draft.polishModel.trimmingCharacters(in: .whitespacesAndNewlines)
            draft.translationLanguage = draft.translationLanguage.trimmingCharacters(in: .whitespacesAndNewlines)
            _ = try CloudClient.endpoint(base: draft.polishBaseURL, path: "chat/completions")
            if draft.recognitionProvider == .compatible {
                _ = try CloudClient.endpoint(base: draft.asrBaseURL, path: "audio/transcriptions")
                guard !draft.asrModel.isEmpty else { throw OpenLessError.message("请填写转写模型。") }
            }
            guard !draft.polishModel.isEmpty, !draft.translationLanguage.isEmpty, draft.translationLanguage.count <= 40 else {
                throw OpenLessError.message("请填写润色模型，以及不超过 40 个字符的翻译语言。")
            }
            if model.updateSettings(draft) { model.announce("设置已保存") }
            else { error = model.alert?.message; model.alert = nil }
        } catch { self.error = error.localizedDescription }
    }
}

private struct CredentialEditor: View {
    var kind: CredentialKind
    @Environment(\.dismiss) private var dismiss
    @State private var key = ""
    @State private var error: String?
    @State private var canSave = false
    @State private var confirmDelete = false

    var body: some View {
        Form {
            Section {
                SecureField("API Key", text: $key)
                    .textInputAutocapitalization(.never).autocorrectionDisabled().privacySensitive()
            } header: { Text(kind.title) } footer: {
                Text("保存在本设备的系统钥匙串，不写入配置文件，不共享给键盘，也不通过 iCloud 钥匙串同步。")
            }
            Section { Button("删除此密钥", role: .destructive) { confirmDelete = true }.disabled(!canSave) }
        }
        .navigationTitle("服务密钥").navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .cancellationAction) { Button("取消") { dismiss() } }
            ToolbarItem(placement: .confirmationAction) {
                Button("保存") {
                    do { try KeychainStore.save(key, for: kind); dismiss() }
                    catch { self.error = error.localizedDescription }
                }.disabled(!canSave || key.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
        }
        .onAppear {
            do { key = try KeychainStore.read(kind); canSave = true }
            catch { self.error = error.localizedDescription }
        }
        .confirmationDialog("删除这个服务的 API Key？", isPresented: $confirmDelete, titleVisibility: .visible) {
            Button("删除密钥", role: .destructive) {
                do { try KeychainStore.delete(kind); dismiss() }
                catch { self.error = error.localizedDescription }
            }
        }
        .editorError($error)
    }
}
