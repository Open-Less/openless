import SwiftUI

/// Ordered targets are persisted and validated by Core, never in UI preferences.
struct StyleTranslationSettingsView: View {
  @ObservedObject var library: StyleLibrary
  var onConfigureModels: (() -> Void)? = nil
  var body: some View {
    List {
      StyleOperationSection(library: library)
      if let configuration = library.translation {
        Section {
          if configuration.orderedTargets.isEmpty {
            Text("还没有翻译目标，点击右上角添加。").foregroundStyle(.secondary)
          }
          ForEach(configuration.orderedTargets, id: \.self) { target in
            // Targets are only a list; there is no global default to pick.
            Text(verbatim: target).foregroundStyle(.primary)
              .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
              .accessibilityIdentifier("translationTarget-\(target)")
          }
          .onDelete { indices in
            guard !library.blocked, let index = indices.first, configuration.orderedTargets.indices.contains(index) else { return }
            let target = configuration.orderedTargets[index]
            Task { await library.changeTranslationTargets(action: "deleteTranslationTarget", target: target) }
          }
          .onMove { from, to in
            guard !library.blocked else { return }
            var ordered = configuration.orderedTargets
            ordered.move(fromOffsets: from, toOffset: to)
            Task { await library.changeTranslationTargets(action: "reorderTranslationTargets", targets: ordered) }
          }
          .deleteDisabled(library.blocked).moveDisabled(library.blocked)
        } header: { Text("目标语言") } footer: {
          Text("这里只管理已添加的目标语言；编辑可排序或删除。")
        }
        Section("翻译模型") {
          Label(library.capabilities?.llm.canAttemptTranslation == true ? "已设置" : "尚未设置",
            systemImage: library.capabilities?.llm.canAttemptTranslation == true ? "checkmark.circle" : "slider.horizontal.3")
            .foregroundStyle(.secondary).font(.subheadline)
            .accessibilityIdentifier("translationAvailability")
          if let onConfigureModels {
            Button("配置翻译模型", action: onConfigureModels)
              .disabled(library.blocked).accessibilityIdentifier("translationModelSettings")
          }
          Text("翻译只作用于你在键盘上选了语言的那一次录音。")
            .font(.footnote).foregroundStyle(.secondary)
        }
        Section {
          Label("在键盘上长按“点击说话”（或翻译按钮）并向上滑动选择语言，松手后只翻译这一次录音；点按或不滑动的长按都是普通听写。", systemImage: "character.bubble")
            .font(.subheadline).foregroundStyle(.secondary)
        } header: { Text("在键盘中翻译") }
      } else {
        Text("正在载入…").foregroundStyle(.secondary)
        Button("重新载入") { Task { await library.load() } }.disabled(library.blocked)
      }
    }.scrollContentBackground(.hidden).background(AppDesign.canvas)
  }
}

/// Uses the shell's search binding and dismisses only after Core confirms addition.
struct TranslationTargetPickerView: View {
  @ObservedObject var library: StyleLibrary
  @Environment(\.dismiss) private var dismiss
  @Binding var search: String
  private var availableLanguages: [StyleTranslationConfiguration.Language] {
    guard let configuration = library.translation else { return [] }
    return configuration.catalog.filter { language in
      !configuration.orderedTargets.contains(language.nativeName)
        && (search.isEmpty || language.nativeName.localizedCaseInsensitiveContains(search)
          || language.code.localizedCaseInsensitiveContains(search))
    }
  }
  var body: some View {
    NavigationStack {
      List {
        StyleOperationSection(library: library)
        if library.translation != nil {
          if availableLanguages.isEmpty { Text("没有匹配的语言").foregroundStyle(.secondary) }
          ForEach(availableLanguages) { language in
            Button {
              Task {
                await library.changeTranslationTargets(action: "addTranslationTarget", target: language.nativeName)
                if library.translation?.orderedTargets.contains(language.nativeName) == true { dismiss() }
              }
            } label: { Text(verbatim: language.nativeName) }.disabled(library.blocked)
          }
        }
      }.searchable(text: $search, prompt: "搜索语言").navigationTitle("添加目标语言")
        .toolbar { ToolbarItem(placement: .cancellationAction) { Button("取消") { dismiss() } } }
    }
  }
}
