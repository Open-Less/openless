import OpenLessShared
import SwiftUI
import UIKit

struct VocabularyView: View {
  @ObservedObject var library: ProductLibrary
  @ObservedObject var controller: AppController
  @State private var query = ""
  @State private var source: VocabularySource = .all
  @State private var showSearch = false
  @State private var searchFocused = false
  @State private var editor: EditorSelection?
  @State private var details: CoreVocabularyEntry?
  @State private var deleting: CoreVocabularyEntry?
  @State private var actionError: String?
  @Environment(\.locale) private var locale
  @Environment(\.accessibilityReduceMotion) private var reduceMotion
  private var locked: Bool { library.busy || controller.snapshot.phase.isBusy || !controller.coreReady }
  private var currentPage: Bool { library.vocabularyMatches(query: query, source: source) }
  private var selection: VocabularySelection { VocabularySelection(query: query, source: source) }

  var body: some View {
    page
      .disabled(editor != nil)
      .accessibilityHidden(editor != nil)
      .background {
        VocabularyModalPresenter(content: editorContent).frame(width: 0, height: 0)
      }
      .background(AppDesign.canvas)
      .navigationTitle("")
      .toolbar(.hidden, for: .navigationBar)
      .sheet(item: $details) { entry in VocabularyDetails(entry: entry) }
      .alert("删除词条？", isPresented: Binding(get: { deleting != nil }, set: { if !$0 { deleting = nil } })) {
        Button("取消", role: .cancel) { deleting = nil }
        Button("删除", role: .destructive) {
          if let entry = deleting { perform(["action": "removeVocabulary", "id": entry.id]) }
          deleting = nil
        }
      } message: { Text("只删除所选词条，此操作不能撤销。") }
      .task(id: selection) {
        do { try await Task.sleep(for: .milliseconds(250)); try Task.checkCancellation() }
        catch { return }
        guard controller.coreReady else { return }
        await reload()
      }
      .onChange(of: controller.coreReady) { ready in
        if ready { Task { await reload() } }
      }
  }

  private var editorContent: VocabularyEditor? {
    guard let editor else { return nil }
    return VocabularyEditor(library: library, entry: editor.entry, locked: locked, locale: locale,
      cancel: { self.editor = nil }, saved: {
        self.editor = nil
        Task { await reload() }
      })
  }

  private var page: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: 24) {
        Text("词典").font(.largeTitle.bold()).accessibilityAddTraits(.isHeader)
        sourceFilters
        if actionError != nil || library.vocabularyError != nil {
          VStack(alignment: .leading, spacing: 8) {
            Label("词典暂时无法更新，请重试。", systemImage: "exclamationmark.circle")
            Button("重试读取") { actionError = nil; Task { await reload() } }
          }.font(.subheadline).accessibilityIdentifier("vocabularyError")
        }
        if !controller.coreReady || !currentPage || (library.vocabularyLoading && !library.vocabularyLoaded) {
          ProgressView("正在读取词典…").frame(maxWidth: .infinity).padding(.vertical, 32)
        } else if library.vocabulary.isEmpty {
          EmptyState(symbol: "text.book.closed",
            title: query.isEmpty && source == .all ? "还没有词条" : "没有匹配的词条",
            message: query.isEmpty && source == .all ? "点击右下角加号，添加常用人名或术语。" : "试试其他来源或搜索词。")
            .accessibilityIdentifier("vocabularyEmpty")
        } else {
          vocabularyList
          if library.vocabulary.count < library.vocabularyTotal {
            Button { Task { await library.loadVocabulary(query: query, source: source, more: true) } } label: {
              if library.vocabularyLoading { ProgressView("正在读取…") } else { Text("加载更多") }
            }.disabled(library.vocabularyLoading).frame(maxWidth: .infinity)
          } else if library.vocabularyLoading {
            ProgressView("正在刷新…").frame(maxWidth: .infinity)
          }
          Text("点按词条可编辑、停用、删除或查看详情。")
            .font(.footnote).foregroundStyle(.secondary)
        }
      }.padding(.horizontal, AppDesign.Space.page).padding(.top, 20).padding(.bottom, 24)
    }
    .scrollDismissesKeyboard(.interactively)
    .refreshable { await reload() }
    .safeAreaInset(edge: .bottom, spacing: 0) { dictionaryActions }
  }

  private var dictionaryActions: some View {
    GeometryReader { geometry in
      HStack(spacing: 12) {
        Group {
          if showSearch {
            NativeSearchField(text: $query, placeholder: "搜索词条", identifier: "vocabularySearch",
              focused: searchFocused, onIconTap: toggleSearch)
          } else {
            Button(action: toggleSearch) {
              Image(systemName: "magnifyingglass").font(.title3)
                .frame(width: 52, height: 52)
            }.buttonStyle(.plain).modifier(SearchGlass())
              .accessibilityLabel("搜索词条").accessibilityIdentifier("searchVocabulary")
          }
        }.frame(width: showSearch ? max(52, geometry.size.width - 64) : 52, height: 52)
        if !showSearch { Spacer(minLength: 0) }
        Button {
          searchFocused = false
          editor = EditorSelection(entry: nil)
        } label: {
          Image(systemName: "plus").font(.title3.weight(.medium)).frame(width: 52, height: 52)
        }.buttonStyle(.plain).modifier(SearchGlass()).disabled(locked)
          .accessibilityLabel("添加词条").accessibilityIdentifier("addVocabulary")
      }.tint(.primary)
    }.frame(height: 52).padding(.horizontal, 24).padding(.vertical, 12)
  }

  private func toggleSearch() {
    InterfaceFeedback.expansionChanged(enabled: controller.preferences.haptics)
    withAnimation(ProductMotion.resolve(.spring(response: 0.4, dampingFraction: 0.82), reduce: reduceMotion)) {
      showSearch.toggle()
      searchFocused = showSearch
      if !showSearch { query = "" }
    }
  }

  private var sourceFilters: some View {
    ScrollView(.horizontal, showsIndicators: false) {
      HStack(spacing: 4) {
        ForEach(VocabularySource.filters) { filter in
          Button {
            guard source != filter else { return }
            InterfaceFeedback.selectionChanged(enabled: controller.preferences.haptics)
            source = filter
          } label: {
            HStack(spacing: 6) {
              if filter != .all {
                Image(systemName: filter == .learned ? "sparkle" : "leaf.fill")
                  .foregroundStyle(filter == .learned ? Color.teal : Color.secondary).accessibilityHidden(true)
              }
              Text(LocalizedStringKey(filter.title)).fontWeight(source == filter ? .semibold : .medium)
            }.font(.body).fixedSize().padding(.horizontal, 16).padding(.vertical, 12)
              .foregroundStyle(Color.primary)
              .background {
                if source == filter { Capsule().fill(Color(uiColor: .systemBackground)) }
              }
          }.buttonStyle(.plain).accessibilityAddTraits(source == filter ? .isSelected : [])
            .accessibilityIdentifier("vocabularySource-\(filter.rawValue)")
        }
      }.padding(5).background(Color(uiColor: .secondarySystemBackground), in: Capsule())
    }
  }

  private var vocabularyList: some View {
    LazyVStack(spacing: 0) {
      ForEach(library.vocabulary) { entry in
        if entry.id != library.vocabulary.first?.id { Divider().padding(.leading, 56) }
        Menu {
          Button { details = entry } label: { Label("查看详情", systemImage: "info.circle") }
          Button {
            Task {
              if !(await controller.confirmVocabularyEntry(id: entry.id, phrase: entry.phrase)) {
                actionError = controller.errorMessage ?? "无法确认词条，请重试。"
              } else { actionError = nil }
            }
          } label: { Label("确认用于键盘词典", systemImage: "checkmark.circle") }
            .disabled(locked || !entry.enabled)
          Button { editor = EditorSelection(entry: entry) } label: {
            Label("编辑词条", systemImage: "pencil")
          }.disabled(locked)
          Button { perform(["action": "setVocabularyEnabled", "id": entry.id, "enabled": !entry.enabled]) } label: {
            Label(entry.enabled ? "停用词条" : "启用词条", systemImage: entry.enabled ? "pause.circle" : "play.circle")
          }.disabled(locked)
          Button(role: .destructive) { deleting = entry } label: {
            Label("删除词条", systemImage: "trash")
          }.disabled(locked)
        } label: {
          HStack(alignment: .firstTextBaseline, spacing: 14) {
            Image(systemName: entry.classifiedSource == .manual ? "leaf.fill" : entry.classifiedSource == .learned ? "sparkle" : entry.classifiedSource.symbol)
              .font(.body).foregroundStyle(entry.classifiedSource == .learned ? Color.teal : Color.secondary)
              .frame(width: 24).accessibilityHidden(true)
            Text(entry.phrase).font(.title2).foregroundStyle(.primary)
              .multilineTextAlignment(.leading).frame(maxWidth: .infinity, alignment: .leading)
            if !entry.enabled { Text("已停用").font(.caption).foregroundStyle(.secondary) }
          }.padding(.horizontal, 18).padding(.vertical, 24).contentShape(Rectangle())
        }.buttonStyle(.plain)
          .accessibilityLabel("\(entry.phrase)，\(entry.classifiedSource.title)\(entry.enabled ? "" : "，已停用")")
          .accessibilityHint("打开词条管理菜单")
          .accessibilityIdentifier("vocabularyRow-\(entry.id)")
      }
    }.background(AppDesign.canvas)
      .clipShape(RoundedRectangle(cornerRadius: 22))
      .overlay(RoundedRectangle(cornerRadius: 22).strokeBorder(Color(uiColor: .separator), lineWidth: 0.5))
  }

  private func reload() async { await library.loadVocabulary(query: query, source: source) }
  private func perform(_ action: [String: Any]) {
    guard !locked else { return }
    Task {
      if await library.mutate(action) { actionError = nil; await reload() }
      else { actionError = library.error ?? "操作未完成，请重试。" }
    }
  }
  private struct EditorSelection: Identifiable {
    let id = UUID()
    let entry: CoreVocabularyEntry?
  }
}

private struct VocabularyEditor: View {
  @ObservedObject var library: ProductLibrary
  let entry: CoreVocabularyEntry?
  let locked: Bool
  let locale: Locale
  let cancel: () -> Void
  let saved: () -> Void
  @State private var phrase = ""
  @State private var saving = false
  @State private var saveError: String?
  @FocusState private var focused: Bool
  private var validation: String? { VocabularyInput.validationMessage(phrase) }

  var body: some View {
    GeometryReader { _ in
      ZStack {
        Color.black.opacity(0.28).ignoresSafeArea()
        ViewThatFits(in: .vertical) {
          editorContents
          ScrollView { editorContents }
        }
          .frame(maxWidth: 440)
          .background(Color(uiColor: .systemBackground), in: RoundedRectangle(cornerRadius: 16))
          .padding(24)
          .frame(maxWidth: .infinity, maxHeight: .infinity)
      }
    }
    .environment(\.locale, locale)
    .accessibilityAction(.escape) { if !saving { cancel() } }
    .onAppear { phrase = entry?.phrase ?? ""; focused = true }
  }

  private var editorContents: some View {
    VStack(alignment: .leading, spacing: 20) {
      Text(LocalizedStringKey(entry == nil ? "添加到词典" : "编辑词条")).font(.title2.bold())
        .accessibilityAddTraits(.isHeader)
      TextField("添加新单词", text: $phrase)
        .font(.title3).textInputAutocapitalization(.never).autocorrectionDisabled()
        .focused($focused).padding(14)
        .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Color(uiColor: .separator)))
        .accessibilityIdentifier("vocabularyPhrase")
        .onChange(of: phrase) { _ in saveError = nil }
      if saveError != nil {
        Label("保存未完成，请重试。", systemImage: "exclamationmark.circle").font(.footnote).foregroundStyle(.red)
          .accessibilityIdentifier("vocabularySaveError")
      } else if !phrase.isEmpty, let validation {
        Text(validation).font(.footnote).foregroundStyle(.secondary)
      }
      Text("相同词条不会重复添加。").font(.footnote).foregroundStyle(.secondary)
      ViewThatFits(in: .horizontal) {
        HStack(spacing: 12) { Spacer(minLength: 0); cancelButton; saveButton }
        VStack(alignment: .trailing, spacing: 12) { cancelButton; saveButton }
          .frame(maxWidth: .infinity, alignment: .trailing)
      }
    }.padding(24)
  }

  private var cancelButton: some View {
    Button("取消") { cancel() }.buttonStyle(.plain).padding(.horizontal, 18).padding(.vertical, 12)
      .overlay(RoundedRectangle(cornerRadius: 8).strokeBorder(Color(uiColor: .separator)))
      .disabled(saving).accessibilityIdentifier("cancelVocabulary")
  }
  private var saveButton: some View {
    Button {
      guard !saving, !locked, validation == nil else { return }
      saving = true
      Task {
        if await library.saveVocabulary(phrase: phrase, entryID: entry?.id) { saved() }
        else { saveError = library.error ?? "保存失败，请重试。" }
        saving = false
      }
    } label: {
      HStack(spacing: 8) {
        if saving { ProgressView().tint(.white) }
        Text(LocalizedStringKey(entry == nil ? "添加单词" : "保存词条"))
      }.padding(.horizontal, 18).padding(.vertical, 12)
        .foregroundStyle(Color.white)
        .background(validation != nil || locked || saving ? Color(uiColor: .systemGray3) : Color.accentColor,
          in: RoundedRectangle(cornerRadius: 8))
    }.buttonStyle(.plain).disabled(validation != nil || locked || saving)
      .accessibilityIdentifier("saveVocabulary")
  }
}

private struct VocabularyDetails: View {
  let entry: CoreVocabularyEntry
  @Environment(\.dismiss) private var dismiss
  var body: some View {
    NavigationStack {
      List {
        Section("词条") { Text(entry.phrase).font(.title2).textSelection(.enabled) }
        Section("详细信息") {
          LabeledContent("添加方式", value: entry.classifiedSource.title)
          LabeledContent("状态", value: entry.enabled ? "已启用" : "已停用")
          LabeledContent("使用次数", value: entry.hits.formatted())
          if let date = HistoryFormatting.date(entry.createdAt) { LabeledContent("添加时间", value: date.formatted()) }
          if let note = entry.note, !note.isEmpty { Text(note).textSelection(.enabled) }
        }
        Section {
          Text("确认后的中文词条会用于键盘候选。")
            .font(.footnote).foregroundStyle(.secondary)
        }
      }.navigationTitle("词条详情").navigationBarTitleDisplayMode(.inline)
        .toolbar { ToolbarItem(placement: .confirmationAction) { Button("完成") { dismiss() } } }
    }
  }
}

/// A real modal blocks the native tab bar and isolates VoiceOver; the card itself
/// stays centered and uses the keyboard-safe area, including on iOS 16.
private struct VocabularyModalPresenter: UIViewControllerRepresentable {
  let content: VocabularyEditor?
  func makeUIViewController(context: Context) -> VocabularyPresentationAnchor { VocabularyPresentationAnchor() }
  func updateUIViewController(_ controller: VocabularyPresentationAnchor, context: Context) {
    controller.content = content
    controller.updatePresentation()
  }
  static func dismantleUIViewController(_ controller: VocabularyPresentationAnchor, coordinator: ()) {
    controller.content = nil
    controller.updatePresentation()
  }
}

private final class VocabularyPresentationAnchor: UIViewController {
  var content: VocabularyEditor?
  private var modal: UIHostingController<VocabularyEditor>?

  override func viewDidAppear(_ animated: Bool) {
    super.viewDidAppear(animated)
    updatePresentation()
  }
  func updatePresentation() {
    guard let content else {
      if let modal {
        self.modal = nil
        modal.dismiss(animated: false)
      }
      return
    }
    if let modal {
      modal.rootView = content
    } else if viewIfLoaded?.window != nil, presentedViewController == nil {
      let modal = UIHostingController(rootView: content)
      modal.modalPresentationStyle = .overFullScreen
      modal.isModalInPresentation = true
      modal.view.backgroundColor = .clear
      modal.view.accessibilityViewIsModal = true
      self.modal = modal
      present(modal, animated: false)
    }
  }
}
