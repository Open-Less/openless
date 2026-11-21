import SwiftUI

struct DeveloperSettingsView: View {
  @ObservedObject var controller: AppController
  @State private var pipeline = "traditional"
  @State private var polish = false
  private var settings: DeveloperSettings { controller.developer }
  var body: some View {
    Form {
      if let configuration = settings.configuration {
        Section("处理方式") {
          Picker("模式", selection: $pipeline) {
            Text("语音转写").tag("traditional")
            Text("音频模型").tag("multimodal")
          }.pickerStyle(.segmented).accessibilityIdentifier("pipelinePicker")
          Text(pipeline == "traditional" ? "先转成文字，再按需要润色。" : "由音频模型直接识别和整理声音。")
            .font(.footnote).foregroundStyle(.secondary)
          if pipeline == "traditional" { Toggle("自动润色", isOn: $polish).accessibilityIdentifier("enablePolish") }
          Button("保存处理方式") {
            Task { _ = await settings.perform(["action": "updateSettings", "pipelineMode": pipeline, "polishEnabled": polish]) }
          }.accessibilityIdentifier("savePipeline")
        }
        if pipeline == "traditional" {
          channelSection("语音识别", kind: "asr", channels: configuration.channels.asr, active: configuration.active.asr)
          channelSection("文字处理", kind: "llm", channels: configuration.channels.llm, active: configuration.active.llm)
        } else {
          channelSection("音频模型", kind: "omni", channels: configuration.omniConfigurations, active: configuration.active.omni)
        }
        Section("数据与认证") {
          Text("密钥仅保存在此设备。")
            .font(.footnote).foregroundStyle(.secondary)
          Text("关闭自定义模型后，将使用本机语音识别。")
            .font(.footnote).foregroundStyle(.secondary)
        }
      } else {
        ProgressView("读取提供商配置…")
        Button("重试") { Task { await settings.refresh() } }
      }
      if settings.error != nil {
        Section { Text("暂时未能完成，请检查设置后重试。").foregroundStyle(.secondary).font(.footnote).accessibilityIdentifier("developerError") }
      }
      Section {
        Button("重载并退出开发者模式", role: .destructive) { Task { await controller.reloadDeveloperMode(false) } }
          .accessibilityIdentifier("exitDeveloperMode")
      }
    }
    .disabled(settings.busy || controller.restarting)
    .navigationTitle("开发者模式").navigationBarTitleDisplayMode(.inline)
    .onAppear { syncDraft() }
    .onChange(of: settings.configuration?.preferences.pipelineMode) { _ in syncDraft() }
    .onChange(of: settings.configuration?.polishEnabled) { _ in syncDraft() }
  }
  private func syncDraft() {
    guard let configuration = settings.configuration else { return }
    pipeline = configuration.preferences.pipelineMode
    polish = configuration.polishEnabled
  }
  @ViewBuilder private func channelSection(_ title: String, kind: String, channels: [DeveloperChannel], active: String) -> some View {
    Section(title) {
      ForEach(channels) { channel in
        HStack(spacing: 12) {
          NavigationLink {
            ProviderEditor(settings: settings, kind: kind, channel: channel)
          } label: {
            VStack(alignment: .leading, spacing: 5) {
              Text(channel.title).font(.body)
              Text(channel.model.isEmpty ? channel.providerType : channel.model).font(.caption).foregroundStyle(.secondary)
            }
          }
          Button(channel.id == active ? "已选用" : "使用") {
            Task { _ = await settings.perform(["action": "setActive", "kind": kind, "channelID": channel.id]) }
          }.font(.caption).buttonStyle(.bordered).disabled(channel.id == active || channel.supported == false)
        }
      }
      if kind != "omni" {
        NavigationLink("添加提供商") { ProviderEditor(settings: settings, kind: kind, channel: nil) }
          .accessibilityIdentifier("addProvider-" + kind)
      }
      if channels.isEmpty { Text("尚未添加").font(.footnote).foregroundStyle(.secondary) }
    }
  }
}

private struct ProviderEditor: View {
  @ObservedObject var settings: DeveloperSettings
  let kind: String
  let channel: DeveloperChannel?
  @Environment(\.dismiss) private var dismiss
  @State private var providerType = ""
  @State private var name = ""
  @State private var endpoint = ""
  @State private var model = ""
  @State private var apiKey = ""
  @State private var extra: [String: String] = [:]
  @State private var modelOptions: [String] = []
  @State private var connectionResult: String?
  private var providers: [DeveloperProvider] {
    settings.configuration?.providerDescriptors.filter { $0.kind == kind && $0.supported } ?? []
  }
  private var selected: DeveloperProvider? { providers.first { $0.providerType == providerType } }
  private var isNative: Bool { ["ios-sensevoice", "apple-speech"].contains(providerType) }
  var body: some View {
    Form {
      Section("提供商") {
        Picker("协议", selection: $providerType) {
          ForEach(providers) { Text($0.title).tag($0.providerType) }
        }.disabled(channel != nil).accessibilityIdentifier("providerType")
        TextField("名称", text: $name).accessibilityIdentifier("providerName")
      }
      if !isNative {
        Section("连接配置") {
          TextField("API 地址", text: $endpoint).keyboardType(.URL).textInputAutocapitalization(.never).autocorrectionDisabled()
            .accessibilityIdentifier("providerEndpoint")
          TextField("模型 ID", text: $model).textInputAutocapitalization(.never).autocorrectionDisabled()
            .accessibilityIdentifier("providerModel")
          SecureField(channel?.hasAPIKey == true ? "API Key（已保存，留空保留）" : "API Key", text: $apiKey)
            .textInputAutocapitalization(.never).autocorrectionDisabled().accessibilityIdentifier("providerAPIKey")
          if let channel {
            Button("获取模型列表") { Task { modelOptions = await settings.models(kind: kind, channelID: channel.id) } }
            if !modelOptions.isEmpty {
              Picker("可用模型", selection: $model) { ForEach(modelOptions, id: \.self) { Text($0).tag($0) } }
            }
          }
        }
        if !credentialFields.isEmpty {
          Section("额外认证字段") {
            ForEach(credentialFields, id: \.0) { account, title in
              SecureField(title + "（留空保留）", text: Binding(get: { extra[account] ?? "" }, set: { extra[account] = $0 }))
                .textInputAutocapitalization(.never).autocorrectionDisabled()
            }
          }
        }
        if let channel {
          Section {
            Button("测试连接") {
              Task {
                let ok = await settings.validate(kind: kind, channelID: channel.id)
                connectionResult = ok ? "连接成功" : nil
              }
            }
            Text("修改后请先保存，再测试连接。")
              .font(.footnote).foregroundStyle(.secondary)
            if let connectionResult { Text(connectionResult) }
          }
        }
      } else {
        Section { Text("在此设备上识别声音。")
          .font(.footnote).foregroundStyle(.secondary) }
      }
      if settings.error != nil { Section { Text("暂时未能完成，请检查设置后重试。").foregroundStyle(.secondary).font(.footnote) } }
      Section {
        Button("保存") { Task { await save() } }.disabled(selected == nil || name.trimmingCharacters(in: .whitespaces).isEmpty)
          .accessibilityIdentifier("saveProvider")
        if let channel, kind != "omni" {
          Button("删除提供商与密钥", role: .destructive) {
            Task {
              if await settings.perform(["action": "deleteChannel", "kind": kind, "channelID": channel.id]) { dismiss() }
            }
          }
        }
      }
    }.disabled(settings.busy)
      .navigationTitle(channel == nil ? "添加提供商" : "提供商配置").navigationBarTitleDisplayMode(.inline)
      .onAppear {
        providerType = channel?.providerType ?? providers.first?.providerType ?? ""
        name = channel?.title ?? selected?.title ?? ""
        endpoint = channel?.endpoint ?? selected?.defaultEndpoint ?? ""
        model = channel?.model ?? selected?.defaultModel ?? ""
        modelOptions = selected?.staticModels ?? []
      }
      .onChange(of: providerType) { _ in
        guard channel == nil else { return }
        name = selected?.title ?? providerType
        endpoint = selected?.defaultEndpoint ?? ""
        model = selected?.defaultModel ?? ""
        apiKey = ""; extra = [:]
      }
      .onDisappear { apiKey = ""; extra = [:] }
  }
  private func save() async {
    if !isNative {
      guard let url = URL(string: endpoint), ["https", "http"].contains(url.scheme), url.host != nil else {
        settings.error = "请输入有效的 HTTP 或 HTTPS API 地址。"; return
      }
    }
    var value: [String: Any] = ["action": "saveChannel", "kind": kind, "providerType": providerType,
      "name": name, "endpoint": endpoint, "model": model,
      "credentialFields": extra.filter { !$0.value.isEmpty }]
    if let channel { value["channelID"] = channel.id }
    if !apiKey.isEmpty { value["apiKey"] = apiKey }
    if await settings.perform(value) { apiKey = ""; extra = [:]; dismiss() }
  }
  private var credentialFields: [(String, String)] {
    switch providerType {
    case "volcengine": [("volcengine.app_key", "App Key"), ("volcengine.access_key", "Access Key"), ("volcengine.resource_id", "Resource ID"), ("volcengine.service", "Service"), ("volcengine.auth_mode", "Auth Mode"), ("volcengine.api_key", "API Key")]
    case "iflytek": [("xfyun.app_id", "App ID"), ("xfyun.api_key", "API Key")]
    case "tencent-cloud": [("tencent_cloud.app_id", "App ID"), ("tencent_cloud.secret_id", "Secret ID"), ("tencent_cloud.secret_key", "Secret Key")]
    default: []
    }
  }
}
