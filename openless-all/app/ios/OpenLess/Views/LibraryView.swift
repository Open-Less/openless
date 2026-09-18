import SwiftUI

struct LibraryView: View {
    @EnvironmentObject private var model: AppModel
    @State private var section = 0
    @State private var query = ""
    @State private var editingWord: VocabularyEntry?
    @State private var editingStyle: WritingStyle?

    private var words: [VocabularyEntry] {
        model.document.vocabulary.filter {
            query.isEmpty || $0.term.localizedCaseInsensitiveContains(query) || $0.note.localizedCaseInsensitiveContains(query)
        }.sorted { $0.term.localizedStandardCompare($1.term) == .orderedAscending }
    }

    var body: some View {
        List {
            Picker("资料类型", selection: $section) {
                Text("个人词典").tag(0)
                Text("写作风格").tag(1)
            }
            .pickerStyle(.segmented).listRowInsets(EdgeInsets())
            .listRowBackground(Color.clear).listRowSeparator(.hidden)
            if section == 0 {
                Section {
                    if words.isEmpty {
                        EmptyMessage(symbol: "character.book.closed", title: query.isEmpty ? "让它记住你的词" : "没有找到这个词",
                                     message: "添加人名、产品名和专业术语，让转写与润色更贴近你的表达。")
                    }
                    ForEach(words) { word in
                        Button { editingWord = word } label: {
                            VStack(alignment: .leading, spacing: 6) {
                                Text(word.term).font(.body.weight(.medium)).foregroundStyle(.primary)
                                if !word.note.isEmpty { Text(word.note).font(.caption).foregroundStyle(.secondary).lineLimit(2) }
                            }.padding(.vertical, 5)
                        }
                        .swipeActions {
                            Button("删除", role: .destructive) { model.deleteVocabulary(ids: [word.id]) }
                        }
                    }
                } footer: {
                    Text("每次识别和润色使用最多 100 个词条作为提示。使用云端服务时，这些词条会随该次请求发送。")
                }
            } else {
                Section("内置风格") {
                    ForEach(WritingStyle.builtIns) { style in styleRow(style) }
                }
                Section {
                    ForEach(model.document.customStyles) { style in
                        styleRow(style)
                            .swipeActions {
                                Button("删除", role: .destructive) { model.deleteStyle(style) }.disabled(model.isBusy)
                            }
                    }
                    Button("新建写作风格", systemImage: "plus") { newStyle() }
                } header: { Text("我的风格") } footer: {
                    Text("风格只负责整理你说的话。原文以外的风格和翻译需要在设置中配置润色服务。")
                }
            }
        }
        .navigationTitle("词典与风格")
        .searchable(text: $query, prompt: "搜索词典")
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    if section == 0 { editingWord = VocabularyEntry(term: "", note: "") }
                    else { newStyle() }
                } label: { Image(systemName: "plus") }
                .accessibilityLabel(section == 0 ? "添加词条" : "新建风格")
            }
        }
        .sheet(item: $editingWord) { word in NavigationStack { VocabularyEditor(entry: word) } }
        .sheet(item: $editingStyle) { style in NavigationStack { StyleEditor(style: style) } }
    }

    private func styleRow(_ style: WritingStyle) -> some View {
        Button { editingStyle = style } label: {
            HStack(spacing: 14) {
                Image(systemName: style.symbol).font(.title3).foregroundStyle(OpenLessTheme.accent)
                    .frame(width: 40, height: 40).background(OpenLessTheme.accent.opacity(0.08), in: RoundedRectangle(cornerRadius: 12))
                VStack(alignment: .leading, spacing: 5) {
                    Text(style.name).font(.body.weight(.medium)).foregroundStyle(.primary)
                    Text(style.summary).font(.caption).foregroundStyle(.secondary).lineLimit(2)
                }
                Spacer()
                if model.settings.selectedStyleID == style.id {
                    Image(systemName: "checkmark.circle.fill").foregroundStyle(OpenLessTheme.accent)
                        .accessibilityLabel("当前风格")
                }
            }.padding(.vertical, 5)
        }
    }

    private func newStyle() {
        editingStyle = WritingStyle(id: UUID().uuidString, name: "", summary: "", symbol: "sparkles", instruction: "", isBuiltIn: false)
    }
}

private struct VocabularyEditor: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.dismiss) private var dismiss
    @State private var entry: VocabularyEntry
    @State private var error: String?

    init(entry: VocabularyEntry) { _entry = State(initialValue: entry) }

    var body: some View {
        Form {
            Section("标准写法") {
                TextField("例如 OpenLess、张晓明", text: $entry.term).autocorrectionDisabled()
            }
            Section {
                TextField("例如：语音输入工具，避免写成 Open Less", text: $entry.note, axis: .vertical)
                    .lineLimit(3...6)
            } header: { Text("说明（可选）") } footer: {
                Text("说明用于帮助润色模型理解词义和常见误写。不会机械替换原文中的相似文字。")
            }
        }
        .navigationTitle("词条").navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .cancellationAction) { Button("取消") { dismiss() } }
            ToolbarItem(placement: .confirmationAction) {
                Button("保存") {
                    entry.term = entry.term.trimmingCharacters(in: .whitespacesAndNewlines)
                    entry.note = entry.note.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !entry.term.isEmpty, entry.term.count <= 100, entry.note.count <= 300 else {
                        error = "词条须为 1–100 个字符，说明不能超过 300 个字符。"; return
                    }
                    if model.saveVocabulary(entry) { dismiss() }
                    else { error = model.alert?.message; model.alert = nil }
                }
            }
        }
        .editorError($error)
    }
}

private struct StyleEditor: View {
    @EnvironmentObject private var model: AppModel
    @Environment(\.dismiss) private var dismiss
    @State private var style: WritingStyle
    @State private var error: String?

    init(style: WritingStyle) { _style = State(initialValue: style) }

    var body: some View {
        Form {
            Section("名称与介绍") {
                TextField("风格名称", text: $style.name).disabled(style.isBuiltIn)
                TextField("一句话说明用途", text: $style.summary, axis: .vertical).disabled(style.isBuiltIn)
            }
            if !style.isVerbatim {
                Section {
                    TextEditor(text: $style.instruction).frame(minHeight: 240).disabled(style.isBuiltIn)
                } header: { Text("整理要求") } footer: {
                    Text("描述语气、结构和写作习惯。例如：整理为简洁的工作消息，保留具体时间和下一步行动。")
                }
            }
            if style.isBuiltIn {
                Section {
                    Button("使用这个风格") {
                        model.selectStyle(style)
                        dismiss()
                    }.disabled(model.isBusy)
                    if !style.isVerbatim {
                        Button("复制为自定义风格") {
                            style.id = UUID().uuidString
                            style.name += " · 副本"
                            style.isBuiltIn = false
                        }
                    }
                }
            }
        }
        .navigationTitle(style.isBuiltIn ? style.name : "编辑风格").navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .cancellationAction) { Button(style.isBuiltIn ? "完成" : "取消") { dismiss() } }
            if !style.isBuiltIn {
                ToolbarItem(placement: .confirmationAction) {
                    Button("保存") {
                        style.name = style.name.trimmingCharacters(in: .whitespacesAndNewlines)
                        style.instruction = style.instruction.trimmingCharacters(in: .whitespacesAndNewlines)
                        guard !style.name.isEmpty, style.name.count <= 40,
                              !style.instruction.isEmpty, style.instruction.count <= 4_000,
                              style.summary.count <= 140 else {
                            error = "名称须为 1–40 字，整理要求须为 1–4,000 字，介绍最多 140 字。"; return
                        }
                        if model.saveStyle(style) { dismiss() }
                        else { error = model.alert?.message; model.alert = nil }
                    }
                }
            }
        }
        .editorError($error)
    }
}
