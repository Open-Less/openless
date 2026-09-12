import SwiftUI

struct HistoryView: View {
    @EnvironmentObject private var model: AppModel
    @State private var search = ""
    @State private var selection: HistoryEntry?
    @State private var confirmClear = false

    private var filtered: [HistoryEntry] {
        let query = search.trimmingCharacters(in: .whitespacesAndNewlines)
        return model.document.history.filter {
            query.isEmpty || $0.outputText.localizedCaseInsensitiveContains(query)
                || $0.rawText.localizedCaseInsensitiveContains(query) || $0.styleName.localizedCaseInsensitiveContains(query)
        }
    }

    private var days: [Date] {
        Array(Set(filtered.map { Calendar.current.startOfDay(for: $0.createdAt) })).sorted(by: >)
    }

    var body: some View {
        List {
            if filtered.isEmpty {
                EmptyMessage(symbol: "clock.arrow.circlepath", title: search.isEmpty ? "好想法，都会留下来" : "没有找到相关记录",
                             message: search.isEmpty ? "完成听写后，原始转写和整理结果会保存在这里。" : "试试其他关键词，也可以搜索原始转写。")
                    .listRowBackground(Color.clear).listRowSeparator(.hidden)
            }
            ForEach(days, id: \.self) { day in
                Section(day.formatted(date: .abbreviated, time: .omitted)) {
                    ForEach(filtered.filter { Calendar.current.isDate($0.createdAt, inSameDayAs: day) }) { entry in
                        Button { selection = entry } label: {
                            VStack(alignment: .leading, spacing: 12) {
                                HStack {
                                    Text(entry.styleName).font(.caption.weight(.medium)).foregroundStyle(OpenLessTheme.accent)
                                    Spacer()
                                    Text(entry.createdAt, style: .time).font(.caption).foregroundStyle(.secondary)
                                }
                                Text(entry.outputText).font(.body).foregroundStyle(.primary).lineLimit(3)
                                HStack(spacing: 12) {
                                    Text("\(entry.outputText.count) 字")
                                    if entry.duration > 0 { Text(durationLabel(entry.duration)) }
                                    if entry.notice != nil { Image(systemName: "info.circle") }
                                }
                                .font(.caption).foregroundStyle(.secondary)
                            }
                            .padding(.vertical, 8)
                        }
                        .swipeActions {
                            Button("删除", role: .destructive) { model.deleteHistory(ids: [entry.id]) }
                                .disabled(model.isBusy)
                        }
                        .contextMenu {
                            Button("复制", systemImage: "doc.on.doc") { model.copy(entry.outputText) }
                            Button("发送到键盘", systemImage: "keyboard") {
                                model.publishToKeyboard(text: entry.outputText, id: entry.id, styleName: entry.styleName)
                            }
                        }
                    }
                }
            }
        }
        .listStyle(.insetGrouped)
        .navigationTitle("历史")
        .searchable(text: $search, prompt: "搜索文字或风格")
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button { confirmClear = true } label: { Image(systemName: "trash") }
                    .accessibilityLabel("清空历史").disabled(model.document.history.isEmpty || model.isBusy)
            }
        }
        .confirmationDialog("删除全部历史记录？", isPresented: $confirmClear, titleVisibility: .visible) {
            Button("删除全部历史", role: .destructive) {
                model.deleteHistory(ids: Set(model.document.history.map(\.id)))
            }
        } message: { Text("这个操作无法撤销。当前草稿会继续保留。") }
        .sheet(item: $selection) { entry in NavigationStack { HistoryDetailView(entry: entry) } }
    }
}

private struct HistoryDetailView: View {
    var entry: HistoryEntry
    @EnvironmentObject private var model: AppModel
    @Environment(\.dismiss) private var dismiss
    @State private var showingOriginal = false

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 20) {
                Text(entry.createdAt.formatted(date: .complete, time: .shortened))
                    .font(.subheadline).foregroundStyle(.secondary)
                Picker("显示内容", selection: $showingOriginal) {
                    Text("整理结果").tag(false)
                    Text("原始转写").tag(true)
                }.pickerStyle(.segmented)
                Text(showingOriginal ? entry.rawText : entry.outputText)
                    .font(.body).lineSpacing(7).textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                if let notice = entry.notice {
                    Label(notice, systemImage: "info.circle").font(.footnote).foregroundStyle(.secondary)
                }
                Divider()
                Label(entry.providerName, systemImage: "waveform").font(.caption).foregroundStyle(.secondary)
                HStack(spacing: 24) {
                    Button("复制", systemImage: "doc.on.doc") { model.copy(showingOriginal ? entry.rawText : entry.outputText) }
                    ShareLink(item: showingOriginal ? entry.rawText : entry.outputText) { Label("分享", systemImage: "square.and.arrow.up") }
                }
                Button("继续整理", systemImage: "wand.and.stars") {
                    model.openHistory(entry)
                    dismiss()
                }
                .buttonStyle(.borderedProminent).disabled(model.isBusy || model.pendingAudioFile != nil)
            }.padding(24)
        }
        .navigationTitle(entry.styleName).navigationBarTitleDisplayMode(.inline)
        .toolbar { ToolbarItem(placement: .confirmationAction) { Button("完成") { dismiss() } } }
    }
}
