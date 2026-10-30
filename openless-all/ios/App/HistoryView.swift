import OpenLessShared
import SwiftUI
import UIKit

struct HistoryView: View {
  @ObservedObject var controller: AppController
  @ObservedObject var library: ProductLibrary
  @State private var query = ""
  @State private var dateFilter = false
  @State private var startDate = Calendar.current.startOfDay(for: Date())
  @State private var endDate = Date()
  @State private var confirmClear = false
  @State private var selected: CoreHistoryEntry?
  private var locked: Bool { library.busy || controller.snapshot.phase.isBusy }

  private var entries: [CoreHistoryEntry] {
    let values = query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? library.history : (library.historyQuery == query ? library.historyResults : [])
    guard dateFilter else { return values }
    let start = Calendar.current.startOfDay(for: min(startDate, endDate))
    let end = Calendar.current.date(byAdding: .day, value: 1, to: Calendar.current.startOfDay(for: max(startDate, endDate))) ?? endDate
    return values.filter { entry in entry.date.map { $0 >= start && $0 < end } ?? false }
  }
  var body: some View {
    ScrollView {
      LazyVStack(alignment: .leading, spacing: 12) {
        dateFilterRow
        pendingItems
        if library.error != nil {
          Label("历史暂时无法载入。", systemImage: "exclamationmark.circle").font(.footnote).foregroundStyle(.secondary)
          Button("重新加载") { Task { await library.refresh() } }
        }
        if library.loading && !library.hasLoaded {
          ProgressView("正在读取历史…").frame(maxWidth: .infinity).padding(.vertical, 60)
        } else if library.history.isEmpty && library.hasLoaded {
          EmptyState(symbol: "clock.arrow.circlepath", title: "还没有识别记录",
            message: "主动录音后，文字会保存在这里。\n历史保留在本机，不随词典同步上传。")
        }
        if library.historySearchError != nil && !query.isEmpty {
          Text("搜索暂时无法完成，请重试。").foregroundStyle(.secondary)
          Button("重试") { Task { await library.searchHistory(query) } }
        }
        if library.historySearching { ProgressView() }
        if entries.isEmpty && (!query.isEmpty || dateFilter) && !library.historySearching {
          Text("没有匹配的记录").foregroundStyle(.secondary).frame(maxWidth: .infinity).padding(.vertical, 32)
        }
        ForEach(entries) { item in
          Button { selected = item } label: {
            VStack(alignment: .leading, spacing: 10) {
              HStack(alignment: .firstTextBaseline) {
                Text(item.date?.formatted(date: .abbreviated, time: .shortened) ?? "时间未知")
                  .font(.caption).foregroundStyle(.secondary)
                Spacer(minLength: 8)
                if let duration = item.durationMs {
                  Text(HistoryFormatting.duration(duration)).font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                }
              }
              Text(item.displayText.isEmpty ? "未生成文字，点按查看详情" : item.displayText)
                .font(.body).lineSpacing(4).lineLimit(3).foregroundStyle(.primary)
                .frame(maxWidth: .infinity, alignment: .leading)
              HStack(spacing: 6) {
                if item.errorCode != nil { Image(systemName: "exclamationmark.circle") }
                Text(LocalizedStringKey(item.statusTitle))
                Spacer()
              }.font(.caption).foregroundStyle(item.errorCode != nil ? AppDesign.danger : .secondary)
            }.productCard()
          }.buttonStyle(.plain).accessibilityIdentifier("historyEntry-\(item.id)")
        }
      }.padding(20)
    }.background(AppDesign.canvas).navigationTitle("历史")
      .searchable(text: $query, placement: .navigationBarDrawer(displayMode: .always), prompt: "搜索历史")
      .navigationBarTitleDisplayMode(.inline) // fixed title: a large title + search drawer + refresh fought over the top inset
      .task(id: query) {
        do { try await Task.sleep(for: .milliseconds(250)); try Task.checkCancellation() }
        catch { return }
        if !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { await library.searchHistory(query) }
      }
      .onChange(of: library.history.map(\.id)) { _ in
        if !query.isEmpty { Task { await library.searchHistory(query) } }
      }
      .toolbar {
        Menu {
          Button { Task { await library.refresh() } } label: { Label("刷新", systemImage: "arrow.clockwise") }
            .disabled(library.loading)
          Button(role: .destructive) { confirmClear = true } label: { Label("清空历史", systemImage: "trash") }
            .disabled(library.history.isEmpty || locked)
        } label: { Image(systemName: "ellipsis.circle").frame(width: 44, height: 44) }
          .accessibilityLabel("历史管理").accessibilityIdentifier("historyActions")
      }
      .alert("清空本机识别历史及其录音？", isPresented: $confirmClear) {
        Button("清空历史", role: .destructive) { Task { await library.mutate(["action": "clearLibraryHistory"]) } }
        Button("取消", role: .cancel) {}
      } message: { Text("词典和独立活动统计不会被清空。此操作不可撤销。") }
      .sheet(item: $selected) { item in
        NavigationStack { HistoryDetailView(summary: item, library: library, controller: controller) }
      }
      .onAppear { controller.refreshHistory() }
      .refreshable { await library.refresh() }
  }

  /// Scrolls with the list, so the native large title, search drawer and pull-to-refresh own the top edge.
  private var dateFilterRow: some View {
    VStack(spacing: 8) {
      HStack {
        Toggle("按日期筛选", isOn: $dateFilter).toggleStyle(.button)
          .accessibilityIdentifier("historyDateFilter")
        Spacer()
        if dateFilter { Button("重置") { dateFilter = false } }
      }.font(.subheadline)
      if dateFilter {
        VStack(spacing: 8) {
          DatePicker("开始", selection: $startDate, displayedComponents: .date)
          DatePicker("结束", selection: $endDate, displayedComponents: .date)
        }.font(.subheadline)
      }
    }
  }

  @ViewBuilder private var pendingItems: some View {
    if !controller.pending.isEmpty {
      Text("待确认文字").font(.headline).padding(.top, 12)
      Text("不会自动写入其他输入框。").font(.footnote).foregroundStyle(.secondary)
      ForEach(controller.pending, id: \.id) { item in
        VStack(alignment: .leading, spacing: 14) {
          Text(item.text).font(.body).lineSpacing(5).textSelection(.enabled)
          Text(item.state == .uncertain ? "写入结果不确定，请检查原输入框。" : "原输入目标已改变。")
            .font(.caption).foregroundStyle(.secondary)
          HStack {
            Button("复制") { UIPasteboard.general.string = item.text }
            Spacer()
            Button("移除", role: .destructive) { controller.discardPending(item.id) }
          }.font(.footnote)
        }.productCard()
      }
    }
  }
}

private struct HistoryDetailView: View {
  let summary: CoreHistoryEntry
  @ObservedObject var library: ProductLibrary
  @ObservedObject var controller: AppController
  @StateObject private var player = HistoryAudioPlayer()
  @Environment(\.dismiss) private var dismiss
  @State private var entry: CoreHistoryEntry?
  @State private var audioURL: URL?
  @State private var failure: String?
  @State private var repolished: String?
  @State private var chosenStyle = ""
  @State private var confirmDelete = false
  @State private var rewriteTask: Task<Void, Never>?
  private var locked: Bool { library.busy || controller.snapshot.phase.isBusy }

  var body: some View {
    ScrollView {
      VStack(alignment: .leading, spacing: 20) {
        if let entry {
          VStack(alignment: .leading, spacing: 8) {
            Text(LocalizedStringKey(entry.statusTitle)).font(.subheadline.weight(.semibold))
            Text(entry.date?.formatted(date: .abbreviated, time: .shortened) ?? "时间未知")
              .font(.caption).foregroundStyle(.secondary)
            if let duration = entry.durationMs { Text(HistoryFormatting.duration(duration)).font(.caption).foregroundStyle(.secondary) }
          }
          if !entry.finalText.isEmpty { textCard("处理结果", text: entry.finalText, id: "historyFinalText") }
          if !entry.originalText.isEmpty { textCard("识别原文", text: entry.originalText, id: "historyOriginalText") }
          if entry.finalText.isEmpty && entry.originalText.isEmpty {
            Text("尚未生成文字。有实际保存的录音时，可在下方播放或导出。")
              .font(.subheadline).foregroundStyle(.secondary)
          }
          if let audioURL {
            VStack(alignment: .leading, spacing: 14) {
              Text("录音").font(.subheadline.weight(.semibold))
              Button { player.toggle(audioURL) } label: {
                Label(player.playing ? "停止播放" : "播放录音", systemImage: player.playing ? "stop.fill" : "play.fill")
              }.disabled(locked).accessibilityIdentifier("historyPlayAudio")
              if entry.canExportAudio {
                ShareLink(item: audioURL) { Label("导出录音", systemImage: "square.and.arrow.up") }
                  .disabled(locked).accessibilityIdentifier("historyExportAudio")
              }
              if player.error != nil { Text("录音暂时无法播放。").font(.footnote).foregroundStyle(.secondary) }
            }.frame(maxWidth: .infinity, alignment: .leading).productCard()
          }
          if !entry.rawTranscript.isEmpty {
            VStack(alignment: .leading, spacing: 12) {
              Menu {
                ForEach(library.styles) { style in
                  Button(style.name) { repolish(entry, style: style) }
                }
              } label: { Label(library.busy ? "正在重新润色…" : "重新润色", systemImage: "wand.and.stars") }
                .disabled(!library.canPolish || locked).accessibilityIdentifier("historyRepolish")
              if library.busy { ProgressView() }
              if !library.canPolish {
                Text("添加文字模型后可重新润色。")
                  .font(.footnote).foregroundStyle(.secondary)
              }
            }.frame(maxWidth: .infinity, alignment: .leading).productCard()
          }
          if let repolished { textCard("重新润色 · \(chosenStyle)", text: repolished, id: "historyRepolishedText") }
          Button("删除记录", role: .destructive) { confirmDelete = true }
            .frame(minHeight: 44).disabled(locked).accessibilityIdentifier("historyDelete")
        } else if failure == nil { ProgressView("正在读取原文…") }
        if failure != nil {
          Label("操作未完成，请稍后重试。", systemImage: "exclamationmark.circle").font(.footnote).foregroundStyle(.secondary)
        }
      }.padding(20)
    }.background(AppDesign.canvas).navigationTitle("记录详情").navigationBarTitleDisplayMode(.inline)
      .toolbar { Button("完成") { dismiss() } }
      .alert("删除这条记录及其录音？", isPresented: $confirmDelete) {
        Button("删除", role: .destructive) {
          Task { player.stop(); if await library.mutate(["action": "deleteHistory", "id": summary.id]) { dismiss() } else { failure = library.error } }
        }
        Button("取消", role: .cancel) {}
      }
      .task {
        do {
          entry = try await library.detail(summary.id)
          if entry?.hasAudioRecording == true {
            do { audioURL = try await library.audioURL(summary.id) }
            catch { failure = "录音文件不存在或不可读取，文字操作仍然可用。" }
          }
        } catch is CancellationError {} catch { failure = error.localizedDescription }
      }
      .onChange(of: controller.snapshot.phase) { phase in if phase.isBusy { player.stop() } }
      .onDisappear { player.stop(); rewriteTask?.cancel() }
  }
  private func textCard(_ title: String, text: String, id: String) -> some View {
    VStack(alignment: .leading, spacing: 14) {
      HStack {
        Text(LocalizedStringKey(title)).font(.subheadline.weight(.semibold))
        Spacer()
        Button { UIPasteboard.general.string = text } label: { Label("复制", systemImage: "doc.on.doc") }
          .font(.footnote).accessibilityLabel("复制\(title)").accessibilityIdentifier(id + "Copy")
      }
      Text(text).font(.body).lineSpacing(6).textSelection(.enabled).accessibilityIdentifier(id)
        .frame(maxWidth: .infinity, alignment: .leading)
    }.productCard()
  }
  private func repolish(_ entry: CoreHistoryEntry, style: LibraryStyle) {
    failure = nil; repolished = nil; chosenStyle = style.name
    rewriteTask = Task {
      do { repolished = try await library.repolish(entry.id, style: style.id) }
      catch is CancellationError {} catch { failure = error.localizedDescription }
    }
  }
}
