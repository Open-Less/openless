import SwiftUI

struct DictationView: View {
    @EnvironmentObject private var model: AppModel
    @State private var showingOriginal = false
    @State private var showingKeyboardGuide = false
    @State private var confirmClear = false
    @FocusState private var editorFocused: Bool

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 22) {
                introduction
                stylePicker
                if model.pendingAudioFile != nil { pendingRecording }
                transcript
                if let notice = model.draftNotice {
                    Label(notice, systemImage: "info.circle")
                        .font(.footnote).foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .padding(20)
            .frame(maxWidth: 760)
            .frame(maxWidth: .infinity)
        }
        .background(OpenLessTheme.canvas)
        .navigationTitle("OpenLess")
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarLeading) {
                Image(systemName: "waveform.circle.fill").foregroundStyle(OpenLessTheme.accent)
            }
            ToolbarItemGroup(placement: .topBarTrailing) {
                Button { showingKeyboardGuide = true } label: { Image(systemName: "keyboard") }
                    .accessibilityLabel("在其他应用中输入")
                Menu {
                    Button("清空当前草稿", systemImage: "trash", role: .destructive) { confirmClear = true }
                        .disabled(!model.hasDraft || model.isBusy)
                } label: { Image(systemName: "ellipsis.circle") }
                    .accessibilityLabel("草稿操作")
            }
            ToolbarItemGroup(placement: .keyboard) {
                Spacer()
                Button("完成") { editorFocused = false }
            }
        }
        .safeAreaInset(edge: .bottom, spacing: 0) { recordingControls }
        .sheet(isPresented: $showingKeyboardGuide) { NavigationStack { KeyboardGuideView() } }
        .confirmationDialog("清空当前草稿？", isPresented: $confirmClear, titleVisibility: .visible) {
            Button("清空草稿和待转写录音", role: .destructive) { model.clearDraft() }
            Button("取消", role: .cancel) {}
        } message: { Text("已保存的历史记录会继续保留。") }
        .onChange(of: model.rawText) { _, _ in if !model.isBusy { model.queueDraftSave() } }
        .onChange(of: model.outputText) { _, _ in if !model.isBusy { model.queueDraftSave() } }
    }

    private var introduction: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("开口，成文。")
                .font(.system(size: 34, weight: .semibold, design: .rounded))
                .accessibilityAddTraits(.isHeader)
            Text("把脑海里的话，变成可以直接使用的文字。")
                .font(.subheadline).foregroundStyle(.secondary)
            HStack(spacing: 18) {
                Label("今天 \(model.todayHistory.count) 次", systemImage: "sun.max")
                Text("\(model.todayHistory.reduce(0) { $0 + $1.outputText.count }) 字")
            }
            .font(.caption.weight(.medium)).foregroundStyle(.secondary)
            .padding(.top, 4)
        }
    }

    private var stylePicker: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("表达方式").font(.subheadline.weight(.semibold))
                Spacer()
                Button { model.toggleTranslation() } label: {
                    Label(model.settings.translationEnabled ? "译为\(model.settings.translationLanguage)" : "翻译", systemImage: "character.bubble")
                        .font(.caption.weight(.medium))
                        .foregroundStyle(model.settings.translationEnabled ? OpenLessTheme.accent : Color.secondary)
                }
                .disabled(model.isBusy)
            }
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 8) {
                    ForEach(model.styles) { style in
                        let selected = model.settings.selectedStyleID == style.id
                        Button { model.selectStyle(style) } label: {
                            Label(style.name, systemImage: style.symbol)
                                .font(.subheadline.weight(.medium))
                                .padding(.horizontal, 15).padding(.vertical, 12)
                                .background(selected ? OpenLessTheme.accent : OpenLessTheme.surface, in: Capsule())
                                .foregroundStyle(selected ? Color.white : Color.primary)
                        }
                        .buttonStyle(.plain).disabled(model.isBusy)
                        .accessibilityAddTraits(selected ? [.isSelected] : [])
                    }
                }
            }
            Text(model.selectedStyle.summary).font(.caption).foregroundStyle(.secondary)
        }
    }

    private var transcript: some View {
        Surface {
            VStack(alignment: .leading, spacing: 16) {
                HStack {
                    Text(model.isBusy ? model.phase.title : "这次的文字").font(.headline)
                    Spacer()
                    if model.elapsed > 0 {
                        Text(durationLabel(model.elapsed)).font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                    }
                }
                if model.phase == .recording || model.phase == .authorizing {
                    WaveformView(level: model.audioLevel, active: model.phase == .recording)
                    Text(model.rawText.isEmpty ? "从一个想法开始说起……" : model.rawText)
                        .font(.body).foregroundStyle(model.rawText.isEmpty ? Color.secondary : Color.primary)
                        .frame(maxWidth: .infinity, minHeight: 150, alignment: .topLeading)
                        .textSelection(.enabled)
                } else {
                    Picker("显示内容", selection: $showingOriginal) {
                        Text("整理结果").tag(false)
                        Text("原始转写").tag(true)
                    }
                    .pickerStyle(.segmented)
                    ZStack(alignment: .topLeading) {
                        TextEditor(text: showingOriginal ? $model.rawText : $model.outputText)
                            .scrollContentBackground(.hidden)
                            .frame(minHeight: 200)
                            .focused($editorFocused)
                            .disabled(model.isBusy || model.pendingAudioFile != nil)
                            .accessibilityLabel(showingOriginal ? "原始转写，可编辑" : "整理结果，可编辑")
                        if (showingOriginal ? model.rawText : model.outputText).isEmpty {
                            Text(showingOriginal ? "也可以在这里输入或粘贴文字，再点“整理原文”。" : "点击下方开始听写。\n完成后，文字会出现在这里。")
                                .font(.body).foregroundStyle(.tertiary)
                                .padding(.top, 8).padding(.leading, 5)
                                .allowsHitTesting(false)
                        }
                    }
                    if model.hasText {
                        Button {
                            editorFocused = false
                            showingOriginal = false
                            model.polishDraft()
                        } label: {
                            Label(model.settings.translationEnabled ? "整理并翻译原文" : "整理原文", systemImage: "wand.and.stars")
                                .font(.subheadline.weight(.medium))
                        }
                        .disabled(model.isBusy || model.pendingAudioFile != nil)
                    }
                    if !visibleText.isEmpty {
                        Divider()
                        ViewThatFits(in: .horizontal) {
                            outputActions(labelled: true)
                            outputActions(labelled: false)
                        }
                    }
                }
            }
        }
    }

    private var visibleText: String { showingOriginal ? model.rawText : model.outputText }

    private func outputActions(labelled: Bool) -> some View {
        HStack(spacing: 18) {
            Button { model.copy(visibleText) } label: {
                actionLabel("复制", symbol: "doc.on.doc", labelled: labelled)
            }
            Button { model.publishToKeyboard(text: visibleText) } label: {
                actionLabel("发送到键盘", symbol: "keyboard", labelled: labelled)
            }
            Spacer(minLength: 0)
            ShareLink(item: visibleText) { Image(systemName: "square.and.arrow.up") }
                .accessibilityLabel("分享文字")
        }
        .font(.subheadline.weight(.medium)).frame(minHeight: 36).disabled(model.isBusy)
    }

    @ViewBuilder
    private func actionLabel(_ text: String, symbol: String, labelled: Bool) -> some View {
        if labelled { Label(text, systemImage: symbol).fixedSize() }
        else { Image(systemName: symbol).accessibilityLabel(text) }
    }

    private var pendingRecording: some View {
        Surface {
            VStack(alignment: .leading, spacing: 12) {
                Label("有一段录音等待转写", systemImage: "waveform.badge.exclamationmark").font(.headline)
                Text("录音保留在此设备。检查转写服务配置后可以继续。")
                    .font(.subheadline).foregroundStyle(.secondary)
                Button("重试转写") { model.retryTranscription() }
                    .buttonStyle(.borderedProminent).disabled(model.isBusy)
            }
        }
    }

    private var recordingControls: some View {
        VStack(spacing: 10) {
            if model.isBusy && model.phase != .recording {
                HStack(spacing: 10) {
                    ProgressView()
                    Text(model.phase.title).font(.subheadline)
                    Spacer()
                    Button("取消") { model.cancelWork() }
                }
                .padding(.vertical, 13)
            } else {
                Button {
                    editorFocused = false
                    showingOriginal = false
                    if model.phase == .recording { model.stopRecording() }
                    else { model.startRecording() }
                } label: {
                    HStack(spacing: 12) {
                        Image(systemName: model.phase == .recording ? "stop.fill" : "mic.fill")
                        Text(model.phase == .recording ? "结束听写" : "开始听写")
                        if model.phase == .recording { Text(durationLabel(model.elapsed)).monospacedDigit() }
                    }
                    .font(.headline).frame(maxWidth: .infinity).frame(minHeight: 54)
                    .background(model.phase == .recording ? Color.red : OpenLessTheme.accent,
                                in: RoundedRectangle(cornerRadius: 18))
                    .foregroundStyle(.white)
                }
                .buttonStyle(.plain)
                .disabled(model.pendingAudioFile != nil)
                .opacity(model.pendingAudioFile != nil ? 0.5 : 1)
            }
            Text("\(model.settings.recognitionProvider.title) · 最长 \(model.settings.effectiveRecordingLimit) 秒")
                .font(.caption2).foregroundStyle(.secondary)
        }
        .padding(.horizontal, 20).padding(.top, 14).padding(.bottom, 10)
        .frame(maxWidth: 760).frame(maxWidth: .infinity)
        .background(.regularMaterial)
    }
}
