import SwiftUI
import UniformTypeIdentifiers

enum StyleLibrarySection: String, CaseIterable, Identifiable {
  case library, market, translation
  var id: String { rawValue }
  var title: String {
    switch self {
    case .library: "我的风格"
    case .market: "风格市场"
    case .translation: "翻译目标"
    }
  }
  var symbol: String {
    switch self { case .library: "sparkles"; case .market: "square.grid.2x2"; case .translation: "character.bubble" }
  }
  var navigationTitle: String {
    switch self {
    case .library: "我的风格"
    case .market: "风格市场"
    case .translation: "翻译目标"
    }
  }
}

/// The tab and Settings shortcut share the same native surface and Core consumer.
/// Changing sections never creates another provider, request pipeline or credential store.
struct StyleLibraryView: View {
  let core: CoreBridge?
  let isBusy: Bool
  let configurationRevision: Int
  let onConfigureModels: (() -> Void)?
  @StateObject private var library: StyleLibrary
  @Binding var section: StyleLibrarySection
  @State private var importing = false
  @State private var showAddTranslation = false
  @State private var translationSearch = ""
  @State private var translationEditMode = EditMode.inactive
  @State private var marketQuery = ""
  @State private var installedQuery = ""
  @State private var removal: InstalledStyle?
  @State private var connectedCoreID: ObjectIdentifier?
  @State private var configurationRefreshPending = false

  init(core: CoreBridge?, isBusy: Bool, section: Binding<StyleLibrarySection>, configurationRevision: Int = 0, onConfigureModels: (() -> Void)? = nil) {
    self._section = section
    self.core = core; self.isBusy = isBusy; self.configurationRevision = configurationRevision
    self.onConfigureModels = onConfigureModels
    _library = StateObject(wrappedValue: StyleLibrary(core: core))
  }

  var body: some View {
    VStack(spacing: 0) {
      if section != .translation {
        PinnedSearchBar(text: section == .market ? $marketQuery : $installedQuery,
          placeholder: "搜索风格", identifier: section == .market ? "styleMarketQuery" : "installedStyleQuery")
      }
      Group {
        switch section {
        case .library: installedStyles
        case .market: marketplace
        case .translation: StyleTranslationSettingsView(library: library, onConfigureModels: onConfigureModels)
        }
      }
    }
    .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
    .background(AppDesign.canvas.ignoresSafeArea())
    .navigationTitle(LocalizedStringKey(section.navigationTitle))
    .navigationBarTitleDisplayMode(.inline)
    // Navigation chrome belongs to this stable shell, not the switched List child.
    .toolbar {
      if section == .library {
        ToolbarItem(placement: .navigationBarTrailing) {
          Button { importing = true } label: { Image(systemName: "plus") }
            .disabled(library.blocked || library.snapshot == nil)
            .accessibilityLabel("导入风格").accessibilityIdentifier("styleImportChoose")
        }
      }
      if section == .translation {
        ToolbarItem(id: "translationEdit", placement: .navigationBarTrailing) {
          EditButton().disabled(library.blocked)
        }
        ToolbarItem(id: "translationAdd", placement: .navigationBarTrailing) {
          Button { translationSearch = ""; showAddTranslation = true } label: { Image(systemName: "plus") }
            .disabled(library.blocked || library.translation == nil)
            .accessibilityLabel("添加翻译目标").accessibilityIdentifier("addTranslationTarget")
        }
      }
    }
    .sheet(isPresented: $showAddTranslation) {
      TranslationTargetPickerView(library: library, search: $translationSearch)
    }
    .environment(\.editMode, $translationEditMode)
    .onChange(of: section) { _ in
      translationEditMode = .inactive
      if section != .translation { showAddTranslation = false }
    }
    .onDisappear { translationEditMode = .inactive }
    .task(id: core.map(ObjectIdentifier.init)) {
      let nextCoreID = core.map(ObjectIdentifier.init)
      if connectedCoreID != nextCoreID { library.connect(core); connectedCoreID = nextCoreID }
      library.setActivityBusy(isBusy)
      // Silent: re-entering this tab must not show a progress row or dim the cards.
      await library.load(silent: true) // Market loads only when its visible section is entered.
    }
    .onChange(of: isBusy) {
      library.setActivityBusy($0)
      refreshConfigurationIfPossible()
    }
    .onChange(of: configurationRevision) { _ in
      configurationRefreshPending = true
      refreshConfigurationIfPossible()
    }
    .onChange(of: library.operation) { _ in refreshConfigurationIfPossible() }
    .fileImporter(isPresented: $importing, allowedContentTypes: [.zip], allowsMultipleSelection: false) { result in
      switch result {
      case .success(let urls): if let url = urls.first { library.pendingImport = url }
      case .failure(let error):
        if (error as NSError).code != NSUserCancelledError { library.error = error.localizedDescription }
      }
    }
    .confirmationDialog("移除此风格？", isPresented: Binding(get: { removal != nil }, set: { if !$0 { removal = nil } }), titleVisibility: .visible) {
      if let style = removal {
        Button("移除", role: .destructive) { Task { await library.remove(style) }; removal = nil }
          .disabled(library.blocked)
      }
      Button("取消", role: .cancel) { removal = nil }
    } message: { Text("移除后将自动调整当前风格选择。").fixedSize(horizontal: false, vertical: true) }
  }

  private func refreshConfigurationIfPossible() {
    guard configurationRefreshPending else { return }
    Task { @MainActor in
      guard configurationRefreshPending, !library.blocked else { return }
      configurationRefreshPending = false
      // Refresh the existing consumer; reconnecting would discard market/import state.
      await library.load(silent: true)
    }
  }

  private var installedStyles: some View {
    ScrollView {
      LazyVStack(alignment: .leading, spacing: 10) {
        StyleOperationSection(library: library)
        availability
        if let snapshot = library.snapshot {
          if snapshot.packs.isEmpty {
            Label("还没有添加风格", systemImage: "sparkles")
              .foregroundStyle(.secondary).accessibilityIdentifier("stylesEmpty")
              .frame(maxWidth: .infinity).padding(.vertical, 48)
          }
          if !snapshot.packs.isEmpty && filteredStyles.isEmpty {
            Text("没有匹配的风格，试试其他关键词。").foregroundStyle(.secondary).padding(.vertical, 32)
          }
          ForEach(filteredStyles) { style in
            StyleCard(library: library, style: style)
          }
        } else if library.silentReading {
          ProgressView().frame(maxWidth: .infinity).padding(.vertical, 48)
        } else if library.operation == nil {
          Button("重新载入") { Task { await library.load() } }
            .accessibilityIdentifier("stylesRefresh")
        }
        if let source = library.pendingImport {
          VStack(alignment: .leading, spacing: 12) {
            Text(verbatim: StyleTextBudget.summary(source.lastPathComponent, limit: 120)).lineLimit(2)
            HStack {
              Button("取消", role: .cancel) { library.pendingImport = nil }
                .accessibilityIdentifier("styleImportCancel")
              Spacer()
              Button("导入") { Task { await library.importPending() } }
                .accessibilityIdentifier("styleImportSave")
            }.disabled(library.blocked)
          }.productCard()
        }
        if let onConfigureModels, let capabilities = library.capabilities, !capabilities.llm.canAttemptTranslation {
          Button(action: onConfigureModels) {
            SettingsLabel("设置文字模型", symbol: "slider.horizontal.3")
              .font(.footnote).foregroundStyle(.secondary)
          }.disabled(library.blocked).accessibilityIdentifier("styleModelSettings")
            .padding(.horizontal, 6).padding(.vertical, 8)
        }
      }.padding(.horizontal, AppDesign.Space.page).padding(.vertical, 12)
    }.refreshable { await library.load(silent: true) }
  }

  private var marketplace: some View {
    List {
      StyleOperationSection(library: library)
      availability
      if library.marketLoaded && library.marketQuery == marketQuery && library.market.isEmpty {
        Section { Text("没有匹配的风格，试试其他关键词。").foregroundStyle(.secondary).accessibilityIdentifier("styleMarketEmpty") }
      }
      ForEach(library.marketQuery == marketQuery ? library.market : []) { item in
        Section { NavigationLink {
          MarketStyleDetailView(library: library, item: item)
        } label: {
          StyleSummary(name: item.name, summary: item.description,
            metadata: StyleTextBudget.summary(item.authorLogin, limit: 80))
            .padding(.vertical, 4)
        }.disabled(library.blocked).accessibilityIdentifier("styleMarketDetail-\(item.id)") }
      }
    }
    .listStyle(.insetGrouped).scrollContentBackground(.hidden).background(AppDesign.canvas)
    .task(id: marketQuery + String(describing: core.map(ObjectIdentifier.init))) {
      do {
        if !marketQuery.isEmpty { try await Task.sleep(for: .milliseconds(300)) }
        while library.blocked {
          try Task.checkCancellation()
          guard library.ready else { return }
          try await Task.sleep(for: .milliseconds(100))
        }
        try Task.checkCancellation()
        await library.searchMarket(query: marketQuery)
      } catch {}
    }
    .refreshable { await library.searchMarket(query: marketQuery) }
  }

  private var filteredStyles: [InstalledStyle] {
    let query = installedQuery.trimmingCharacters(in: .whitespacesAndNewlines)
    return (library.snapshot?.packs ?? []).filter {
      query.isEmpty || $0.name.localizedStandardContains(query) || $0.description.localizedStandardContains(query)
    }
  }

  @ViewBuilder private var availability: some View {
    if !library.ready {
      Text("风格库暂不可用，请稍后重试。").foregroundStyle(.secondary).accessibilityIdentifier("stylesUnavailable")
    }
    if let onConfigureModels, let capabilities = library.capabilities,
      !capabilities.developerMode || !capabilities.llm.canAttemptPolish {
      Button(action: onConfigureModels) {
        SettingsLabel("设置文字模型", symbol: "slider.horizontal.3")
      }.disabled(library.blocked || isBusy).accessibilityIdentifier("styleConfigureModels")
    }
    if isBusy {
      Text("当前任务完成后即可继续设置。").font(.footnote).foregroundStyle(.secondary)
    }
  }
}

/// One compact row per style: detail on the left, a single clear selection control on the right.
private struct StyleCard: View {
  @ObservedObject var library: StyleLibrary
  let style: InstalledStyle
  @Environment(\.dynamicTypeSize) private var typeSize
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  private var activating: Bool { library.activatingStyleID == style.id }
  private var control: some View {
    Button {
      Task { await library.activate(style) }
    } label: {
      HStack(spacing: 4) {
        if style.active { Image(systemName: "checkmark").font(.caption.weight(.bold)) }
        Text(style.active ? "使用中" : "使用").font(.footnote.weight(.semibold))
      }
      .opacity(activating ? 0 : 1) // keep the pill's size; the spinner replaces the label in place
      .overlay { if activating { ProgressView().controlSize(.small).tint(style.active ? .white : AppDesign.accent) } }
      .frame(minWidth: 56, minHeight: 32).padding(.horizontal, 10)
      .foregroundStyle(style.active ? Color.white : AppDesign.accent)
      .background(style.active ? AppDesign.accent : Color.clear, in: Capsule())
      .overlay(Capsule().strokeBorder(AppDesign.accent.opacity(style.active ? 0 : 0.45), lineWidth: 1))
      .frame(minHeight: 44).contentShape(Rectangle())
    }.buttonStyle(PressFeedbackStyle())
      .opacity(!style.active && (library.blocked || library.capabilities?.canActivate(style) != true) ? 0.45 : 1)
      .disabled(style.active || library.blocked || library.capabilities?.canActivate(style) != true)
      .accessibilityLabel(style.active ? "当前使用" : "使用此风格")
      .accessibilityIdentifier("styleUse-\(style.id)")
  }
  private var detailLink: some View {
    NavigationLink {
      InstalledStyleDetailView(library: library, styleID: style.id)
    } label: {
      HStack(alignment: .center, spacing: 12) {
        IconTile(symbol: "sparkles", size: 36)
        StyleSummary(name: style.name, summary: style.description, metadata: nil)
          .frame(maxWidth: .infinity, alignment: .leading)
      }.contentShape(Rectangle())
    }.buttonStyle(.plain).disabled(library.blocked)
      .accessibilityIdentifier("styleDetail-\(style.id)")
  }
  var body: some View {
    Group {
      if typeSize.isAccessibilitySize {
        VStack(alignment: .leading, spacing: 8) { detailLink; control }
      } else {
        HStack(spacing: 8) { detailLink; control }
      }
    }
    .productCard(padding: 14)
    .overlay(RoundedRectangle(cornerRadius: AppDesign.Radius.card, style: .continuous)
      .strokeBorder(AppDesign.accent.opacity(style.active ? 0.4 : 0), lineWidth: 1))
    .animation(ProductMotion.resolve(ProductMotion.quick, reduce: reduceMotion), value: style.active)
  }
}

struct StyleOperationSection: View {
  @ObservedObject var library: StyleLibrary
  var body: some View {
    if library.operation != nil || library.error != nil || library.notice != nil {
      Section {
        if let operation = library.operation {
          ProgressView(operation).accessibilityIdentifier("stylesLoading")
          if library.canCancelRead {
            Button("取消") { library.cancelRead() }.accessibilityIdentifier("stylesCancelRead")
          }
        }
        if library.error != nil {
          Text("暂时未能完成，请稍后重试。")
            .font(.subheadline).foregroundStyle(.secondary).lineLimit(2).accessibilityIdentifier("stylesError")
        }
        if let notice = library.notice {
          Text(LocalizedStringKey(notice))
            .font(.subheadline).foregroundStyle(.secondary).lineLimit(3)
            .accessibilityIdentifier("stylesNotice")
        }
      }
    }
  }
}

private struct InstalledStyleDetailView: View {
  @ObservedObject var library: StyleLibrary
  let styleID: String
  @State private var detail: StyleDetail?
  @State private var error: String?
  @State private var detailRevision = UUID()
  @State private var confirmRemoval = false
  private var installed: InstalledStyle? { library.snapshot?.packs.first { $0.id == styleID } }

  private func load() async {
    do { detail = try await library.detail(styleID); error = nil; detailRevision = UUID() }
    catch is CancellationError {} catch { self.error = error.localizedDescription }
  }
  var body: some View {
    List {
      StyleOperationSection(library: library)
      if error != nil {
        Section { Text("详情暂时无法载入，请重试。")
          .foregroundStyle(.red).accessibilityIdentifier("styleDetailError") }
      }
      if let detail {
        Group {
          Section {
            StyleSummary(name: detail.pack.name, summary: detail.pack.description, metadata: nil)
          }
          Section("内容") {
            styleField("完整说明", detail.pack.description)
            styleField("语音提示词", detail.pack.prompt)
            styleField("选区提示词", detail.pack.selectionPrompt)
            styleField("语音编辑提示词", detail.pack.voiceEditPrompt)
            StyleExamples(examples: detail.pack.examples)
          }
          Section("信息") {
            styleField("完整名称", detail.pack.name)
            styleField("版本", detail.pack.version)
            styleField("基础模式", detail.pack.baseMode)
            if let author = detail.pack.author { styleField("作者", author) }
            if let model = detail.pack.recommendedModel { styleField("推荐模型", model) }
            if let version = detail.pack.compatibleAppVersion { styleField("兼容版本", version) }
            StyleTags(tags: detail.pack.tags)
          }
        }.id(detailRevision)
      }
      if let installed {
        Section {
          Button(installed.enabled ? "停用此风格" : "启用此风格") {
            Task { await library.setEnabled(installed, enabled: !installed.enabled) }
          }.disabled(library.blocked || library.capabilities?.canChangeEnabled(installed) != true)
          if installed.canRemove {
            Button("移除此风格", role: .destructive) { confirmRemoval = true }
              .disabled(library.blocked)
          }
        }
      }
      Button("刷新详情") { Task { await load() } }
        .disabled(library.blocked).accessibilityIdentifier("styleDetailRefresh")
    }
    .navigationTitle("风格详情")
    .navigationBarTitleDisplayMode(.inline)
    .safeAreaInset(edge: .bottom) {
      if let installed {
        StylePrimaryAction(title: installed.active ? "当前使用" : "使用此风格", current: installed.active,
          disabled: library.blocked || installed.active || library.capabilities?.canActivate(installed) != true,
          identifier: "styleActivate-" + styleID) {
          Task { await library.activate(installed) }
        }
      }
    }
    .confirmationDialog("移除此风格？", isPresented: $confirmRemoval, titleVisibility: .visible) {
      if let installed { Button("移除", role: .destructive) { Task { await library.remove(installed) } } }
      Button("取消", role: .cancel) {}
    }
    .task { await load() }
  }
}

private struct MarketStyleDetailView: View {
  @ObservedObject var library: StyleLibrary
  let item: MarketStyle
  @State private var detail: MarketStyleDetail?
  @State private var error: String?
  @State private var confirmInstall = false
  @State private var detailRevision = UUID()
  private var summary: MarketStyle { detail?.summary ?? item }

  private func load() async {
    do { detail = try await library.marketDetail(item.id); error = nil; detailRevision = UUID() }
    catch is CancellationError {} catch { self.error = error.localizedDescription }
  }
  var body: some View {
    List {
      StyleOperationSection(library: library)
      Section {
        StyleSummary(name: summary.name, summary: summary.description,
          metadata: StyleTextBudget.summary(summary.authorLogin, limit: 80))
        Text("下载 \(summary.downloadCount) · 喜欢 \(summary.likeCount)")
          .font(.subheadline).foregroundStyle(.secondary)
      }
      if error != nil {
        Section { Text("详情暂时无法载入，请重试。")
          .foregroundStyle(.red).accessibilityIdentifier("styleMarketDetailError") }
      }
      Section("内容") {
        styleField("完整说明", summary.description)
        if let detail { styleField("提示词", detail.prompt) }
      }.id(detailRevision)
      Section("信息") {
        styleField("完整名称", summary.name)
        styleField("作者", summary.authorLogin)
        styleField("版本", summary.version)
        styleField("基础模式", summary.baseMode)
        if let detail { styleField("发布状态", detail.state) }
        Button("刷新详情") { Task { await load() } }
          .disabled(library.blocked).accessibilityIdentifier("styleMarketFetchDetail")
      }.id(detailRevision)
    }
    .navigationTitle("市场详情")
    .navigationBarTitleDisplayMode(.inline)
    .safeAreaInset(edge: .bottom) {
      StylePrimaryAction(title: "下载并安装", disabled: library.blocked,
        identifier: "styleMarketInstall") { confirmInstall = true }
    }
    .confirmationDialog("添加到风格库？", isPresented: $confirmInstall, titleVisibility: .visible) {
      Button("下载并安装") { Task { await library.install(item.id) } }.disabled(library.blocked)
      Button("取消", role: .cancel) {}
    } message: {
      Text("将安装独立副本；不会自动使用。")
    }
    // Opening a specific detail is the user's request. Tabs and list rows do not prefetch.
    .task(id: item.id) { await load() }
  }
}

/// System text styles, with bounded values *before* SwiftUI measures them.
private struct StyleSummary: View {
  let name: String
  let summary: String
  let metadata: String?
  var body: some View {
    VStack(alignment: .leading, spacing: 6) {
      Text(verbatim: StyleTextBudget.summary(name, limit: 100))
        .font(.headline).lineLimit(2)
      if !summary.isEmpty {
        Text(verbatim: StyleTextBudget.summary(summary))
          .font(.subheadline).foregroundStyle(.secondary).lineLimit(2)
      }
      if let metadata, !metadata.isEmpty {
        Text(verbatim: StyleTextBudget.summary(metadata, limit: 100))
          .font(.subheadline).foregroundStyle(.secondary).lineLimit(1)
      }
    }
    .padding(.vertical, 4)
  }
}

private struct StylePrimaryAction: View {
  let title: String
  var current = false
  let disabled: Bool
  let identifier: String
  let action: () -> Void
  var body: some View {
    Button(action: action) {
      Label { Text(LocalizedStringKey(title)) } icon: { if current { Image(systemName: "checkmark") } }
        .labelStyle(.titleAndIcon).font(.body.weight(.semibold))
        .frame(maxWidth: .infinity, minHeight: 44)
    }
    .buttonStyle(PressFeedbackStyle())
    .foregroundStyle(.white)
    .background(current ? AppDesign.success : AppDesign.accent, in: Capsule())
    .opacity(disabled && !current ? 0.45 : 1).disabled(disabled)
    .accessibilityIdentifier(identifier)
    .padding(.horizontal, AppDesign.Space.page).padding(.vertical, 10)
    .background(AppDesign.canvas.opacity(0.96))
  }
}

/// Each field is collapsed initially. Expanding lays out a single bounded page,
/// never the entire prompt (including maliciously long grapheme clusters).
private struct StylePagedText: View {
  let title: String
  // Retain the immutable string owning the cursor indices for this pager's lifetime.
  @State private var document: String
  @State private var expanded = false
  @State private var starts: [String.Index] = []
  init(title: String, value: String) {
    self.title = title
    _document = State(initialValue: value)
  }
  private var page: StyleTextBudget.Page {
    StyleTextBudget.page(document, from: starts.last ?? document.unicodeScalars.startIndex)
  }
  var body: some View {
    if !StyleTextBudget.page(document, from: document.unicodeScalars.startIndex, limit: 160, lines: 2).hasNext {
      VStack(alignment: .leading, spacing: 5) {
        Text(LocalizedStringKey(title)).font(.body)
        Text(verbatim: document.isEmpty ? "（空）" : StyleTextBudget.summary(document))
          .font(.subheadline).foregroundStyle(.secondary)
      }
    } else {
      DisclosureGroup(isExpanded: $expanded) {
        if expanded {
          let current = page
          Text(verbatim: current.text.isEmpty ? "（空）" : current.text)
            .font(.subheadline).textSelection(.enabled)
            .frame(maxWidth: .infinity, alignment: .leading)
          if !starts.isEmpty || current.hasNext {
            HStack {
              Button("上一页") { if !starts.isEmpty { starts.removeLast() } }
                .disabled(starts.isEmpty)
              Spacer()
              Text("第 \(starts.count + 1) 页").font(.subheadline).foregroundStyle(.secondary)
              Spacer()
              Button("下一页") { starts.append(current.end) }.disabled(!current.hasNext)
            }.buttonStyle(.borderless)
          }
        }
      } label: {
        VStack(alignment: .leading, spacing: 5) {
          Text(LocalizedStringKey(title)).font(.body)
          if !expanded {
            Text(verbatim: document.isEmpty ? "（空）" : StyleTextBudget.summary(document))
              .font(.subheadline).foregroundStyle(.secondary).lineLimit(2)
          }
        }
      }
    }
  }
}

private struct StyleExamples: View {
  let examples: [StyleDetail.Pack.Example]
  @State private var expanded = false
  @State private var index = 0
  var body: some View {
    DisclosureGroup("使用示例", isExpanded: $expanded) {
      if expanded, examples.indices.contains(index) {
        let example = examples[index]
        VStack(alignment: .leading, spacing: 12) {
          if let title = example.title {
            styleField("示例标题", title)
          }
          styleField("输入", example.input)
          styleField("输出", example.output)
          HStack {
            Button("上一个") { index -= 1 }.disabled(index == 0)
            Spacer()
            Text("\(index + 1) / \(examples.count)").font(.subheadline).foregroundStyle(.secondary)
            Spacer()
            Button("下一个") { index += 1 }.disabled(index + 1 >= examples.count)
          }.buttonStyle(.borderless)
        }.id(index)
      } else if expanded { Text("暂无示例").foregroundStyle(.secondary) }
    }
  }
}

/// Keep the original collection. Only the selected tag creates a bounded pager.
private struct StyleTags: View {
  let tags: [String]
  @State private var expanded = false
  @State private var index = 0
  var body: some View {
    DisclosureGroup("标签", isExpanded: $expanded) {
      if expanded, tags.indices.contains(index) {
        VStack(alignment: .leading, spacing: 12) {
          styleField("标签内容", tags[index])
          HStack {
            Button("上一项") { index -= 1 }.disabled(index == 0)
            Spacer()
            Text("第 \(index + 1) 项 / \(tags.count)")
              .font(.subheadline).foregroundStyle(.secondary)
            Spacer()
            Button("下一项") { index += 1 }.disabled(index + 1 >= tags.count)
          }.buttonStyle(.borderless)
        }.id(index)
      } else if expanded { Text("暂无标签").foregroundStyle(.secondary) }
    }
  }
}

// Kept Foundation-only so the pathological-text budget can be checked without UI tests.
private enum StyleTextBudget {
  struct Page {
    let text: String
    let end: String.Index
    let hasNext: Bool
  }
  static func summary(_ value: String, limit: Int = 160) -> String {
    let text = page(value, from: value.unicodeScalars.startIndex, limit: limit, lines: 2)
    return text.text + (text.hasNext ? "…" : "")
  }
  static func page(_ value: String, from start: String.Index,
    limit: Int = 1200, lines: Int = 24) -> Page {
    let scalars = value.unicodeScalars
    var end = start
    var units = 0
    var breaks = 0
    while end < scalars.endIndex {
      let scalar = scalars[end]
      let size = scalar.value > 0xffff ? 2 : 1
      guard units + size <= limit else { break }
      units += size
      scalars.formIndex(after: &end)
      if [0x0a, 0x0b, 0x0c, 0x0d, 0x85, 0x2028, 0x2029].contains(scalar.value) {
        breaks += 1
        if breaks >= lines { break }
      }
    }
    return Page(text: String(scalars[start..<end]), end: end, hasNext: end < scalars.endIndex)
  }
}

@MainActor
func styleField(_ title: String, _ value: String) -> some View {
  StylePagedText(title: title, value: value)
}
