import SwiftUI
import UIKit

enum DictationPhase: Equatable {
    case ready, authorizing, recording, transcribing, polishing
    var title: String {
        switch self {
        case .ready: return "准备好，随时开口"
        case .authorizing: return "正在准备麦克风"
        case .recording: return "正在聆听"
        case .transcribing: return "正在转写"
        case .polishing: return "正在整理文字"
        }
    }
}

@MainActor
final class AppModel: ObservableObject {
    @Published private(set) var document = AppDocument()
    @Published private(set) var phase: DictationPhase = .ready
    @Published var rawText = ""
    @Published var outputText = ""
    @Published private(set) var elapsed: TimeInterval = 0
    @Published private(set) var audioLevel: Double = 0
    @Published private(set) var pendingAudioFile: String?
    @Published private(set) var draftNotice: String?
    @Published var alert: UserNotice?
    @Published var feedback: String?
    @Published var selectedTab = 0

    private var storage: LocalStore?
    private let capture = AudioCapture()
    private let cloud = CloudClient()
    private var work: Task<Void, Never>?
    private var autosave: Task<Void, Never>?
    private var feedbackTask: Task<Void, Never>?
    private var operationID = UUID()
    private var historyID: UUID?
    private var context: WorkContext?

    private struct WorkContext {
        var settings: AppSettings
        var style: WritingStyle
        var vocabulary: [VocabularyEntry]
    }

    init() {
        do {
            let storage = try LocalStore()
            document = try storage.load()
            self.storage = storage
            rawText = document.draft.rawText
            outputText = document.draft.outputText
            pendingAudioFile = document.draft.audioFileName
            elapsed = document.draft.duration
            historyID = document.draft.historyID
            draftNotice = document.draft.notice
        } catch {
            alert = UserNotice(title: "无法读取本地数据", message: "\(error.localizedDescription)\n为保留原文件，本次不会覆盖本地数据。")
        }
        capture.onUpdate = { [weak self] text, duration, level in
            guard let self else { return }
            if self.rawText != text {
                self.rawText = text
                self.queueDraftSave()
            }
            self.elapsed = duration
            self.audioLevel = level
        }
        capture.onRecordingReady = { [weak self] url, duration in
            guard let self else { return }
            self.pendingAudioFile = url.lastPathComponent
            self.elapsed = duration
            self.persistDraft()
        }
        capture.onStopRequested = { [weak self] in self?.stopRecording() }
    }

    var settings: AppSettings { document.settings }
    var styles: [WritingStyle] { WritingStyle.builtIns + document.customStyles }
    var selectedStyle: WritingStyle {
        styles.first { $0.id == settings.selectedStyleID } ?? WritingStyle.builtIns[0]
    }
    var isBusy: Bool { phase != .ready }
    var hasText: Bool { !rawText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }
    var hasDraft: Bool { hasText || !outputText.isEmpty || pendingAudioFile != nil }
    var colorScheme: ColorScheme? {
        switch settings.appearance { case .system: return nil; case .light: return .light; case .dark: return .dark }
    }
    var todayHistory: [HistoryEntry] {
        document.history.filter { Calendar.current.isDateInToday($0.createdAt) }
    }

    @discardableResult
    private func commit(_ mutation: (inout AppDocument) -> Void) -> Bool {
        guard let storage else {
            showError("本地存储不可用，请重新打开应用。原有文件不会被覆盖。")
            return false
        }
        var next = document
        mutation(&next)
        do {
            try storage.save(next)
            document = next
            return true
        } catch {
            showError("保存失败：\(error.localizedDescription) 当前编辑仍留在屏幕上，请先复制。")
            return false
        }
    }

    @discardableResult
    func updateSettings(_ settings: AppSettings) -> Bool {
        guard !isBusy else { showError("请等待当前听写结束后再保存设置。"); return false }
        return commit { $0.settings = settings }
    }

    func selectStyle(_ style: WritingStyle) {
        guard !isBusy else { return }
        commit { $0.settings.selectedStyleID = style.id }
    }

    func toggleTranslation() {
        guard !isBusy else { return }
        commit { $0.settings.translationEnabled.toggle() }
    }

    func startRecording() {
        guard !isBusy else { return }
        guard pendingAudioFile == nil else {
            showError("还有一段待转写录音，请先重试转写，或清空草稿后再开始。")
            return
        }
        guard let storage else { showError("本地存储不可用，无法开始录音。"); return }
        let current = makeContext()
        do {
            if current.settings.recognitionProvider == .compatible {
                _ = try CloudClient.endpoint(base: settings.asrBaseURL, path: "audio/transcriptions")
                guard !settings.asrModel.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
                      !(try KeychainStore.read(.transcription)).isEmpty else {
                    throw OpenLessError.message("请先在设置中填写转写模型并保存转写 API Key。")
                }
            }
        } catch { showError(error.localizedDescription); return }
        if hasDraft && !saveHistory(using: context ?? current) { return }
        guard commit({ $0.draft = DictationDraft() }) else { return }
        resetDraftInMemory()
        context = current
        phase = .authorizing
        let id = UUID()
        operationID = id
        work = Task { [weak self] in
            guard let self else { return }
            do {
                try await self.capture.start(settings: current.settings, vocabulary: current.vocabulary,
                                             outputURL: storage.newAudioURL())
                guard self.operationID == id else { return }
                self.phase = .recording
            } catch {
                guard self.operationID == id else { return }
                self.fail(error, context: current)
            }
        }
    }

    func stopRecording() {
        guard phase == .recording, let current = context else { return }
        phase = .transcribing
        let id = operationID
        work = Task { [weak self] in
            guard let self else { return }
            do {
                let result = try await self.capture.stop()
                try Task.checkCancellation()
                guard self.operationID == id else { return }
                self.elapsed = result.duration
                self.draftNotice = result.notice
                self.audioLevel = 0
                if let url = result.audioURL {
                    self.pendingAudioFile = url.lastPathComponent
                    guard self.persistDraft() else {
                        throw OpenLessError.message("录音已保留，但草稿保存失败。请先重试保存或转写。")
                    }
                    try await self.transcribeAudio(using: current, id: id)
                } else {
                    self.rawText = result.text
                    self.outputText = result.text
                    self.persistDraft()
                }
                try await self.finishText(using: current, id: id)
            } catch {
                guard self.operationID == id else { return }
                self.fail(error, context: current)
            }
        }
    }

    func retryTranscription() {
        guard !isBusy, pendingAudioFile != nil else { return }
        var current = makeContext()
        current.settings.recognitionProvider = .compatible
        context = current
        phase = .transcribing
        let id = UUID()
        operationID = id
        work = Task { [weak self] in
            guard let self else { return }
            do {
                try await self.transcribeAudio(using: current, id: id)
                try await self.finishText(using: current, id: id)
            } catch {
                guard self.operationID == id else { return }
                self.fail(error, context: current)
            }
        }
    }

    func polishDraft() {
        guard !isBusy, hasText, pendingAudioFile == nil else { return }
        let current = makeContext()
        context = current
        draftNotice = nil
        phase = .polishing
        let id = UUID()
        operationID = id
        work = Task { [weak self] in
            guard let self else { return }
            do { try await self.finishText(using: current, id: id) }
            catch {
                guard self.operationID == id else { return }
                self.fail(error, context: current)
            }
        }
    }

    private func transcribeAudio(using current: WorkContext, id: UUID) async throws {
        guard let name = pendingAudioFile, let storage else { throw OpenLessError.message("待转写录音不存在。") }
        let key = try KeychainStore.read(.transcription)
        let text = try await cloud.transcribe(file: storage.audioURL(fileName: name), settings: current.settings,
                                              key: key, vocabulary: current.vocabulary)
        try Task.checkCancellation()
        guard operationID == id else { throw CancellationError() }
        rawText = text
        outputText = text
        draftNotice = nil
        pendingAudioFile = nil
        // Delete audio only after the transcript has reached durable storage.
        guard persistDraft() else { pendingAudioFile = name; throw OpenLessError.message("转写已完成，但未能保存。录音仍保留。") }
        do { try storage.removeAudio(fileName: name) }
        catch { draftNotice = "文字已保存，但录音清理失败：\(error.localizedDescription)" }
    }

    private func finishText(using current: WorkContext, id: UUID) async throws {
        if !current.style.isVerbatim || current.settings.translationEnabled {
            phase = .polishing
            let key = try KeychainStore.read(.polishing)
            let result = try await cloud.polish(text: rawText, style: current.style, settings: current.settings,
                                               key: key, vocabulary: current.vocabulary)
            try Task.checkCancellation()
            guard operationID == id else { throw CancellationError() }
            outputText = result
        } else {
            outputText = rawText
        }
        phase = .ready
        work = nil
        saveHistory(using: current, updateStyle: true)
        persistDraft()
    }

    func cancelWork() {
        operationID = UUID()
        work?.cancel()
        work = nil
        capture.cancel()
        phase = .ready
        audioLevel = 0
        if outputText.isEmpty { outputText = rawText }
        draftNotice = "已取消处理，当前文字和待转写录音已保留。"
        if hasText { saveHistory(using: context ?? makeContext()) }
        persistDraft()
    }

    private func fail(_ error: Error, context: WorkContext) {
        capture.cancel()
        phase = .ready
        audioLevel = 0
        work = nil
        if outputText.isEmpty { outputText = rawText }
        draftNotice = error.localizedDescription
        if hasText { saveHistory(using: context) }
        persistDraft()
        if !(error is CancellationError) { showError(error.localizedDescription) }
    }

    func queueDraftSave() {
        // Throttle writes instead of postponing indefinitely during continuous speech.
        guard autosave == nil else { return }
        autosave = Task { @MainActor [weak self] in
            do { try await Task.sleep(for: .milliseconds(650)) } catch { return }
            self?.persistDraft()
        }
    }

    @discardableResult
    func persistDraft() -> Bool {
        autosave?.cancel()
        autosave = nil
        let draft = DictationDraft(rawText: rawText, outputText: outputText, audioFileName: pendingAudioFile,
                                   duration: elapsed, historyID: historyID, notice: draftNotice)
        return commit { $0.draft = draft }
    }

    @discardableResult
    private func saveHistory(using current: WorkContext, updateStyle: Bool = false) -> Bool {
        guard hasText || !outputText.isEmpty else { return true }
        let id = historyID ?? UUID()
        let prior = document.history.first { $0.id == id }
        let selectedName = current.style.name + (current.settings.translationEnabled ? " · \(current.settings.translationLanguage)" : "")
        let styleName = updateStyle ? selectedName : (prior?.styleName ?? selectedName)
        let entry = HistoryEntry(id: id, createdAt: prior?.createdAt ?? Date(), rawText: rawText,
                                 outputText: outputText.isEmpty ? rawText : outputText, styleName: styleName,
                                 providerName: prior?.providerName ?? current.settings.recognitionProvider.title, duration: elapsed,
                                 notice: draftNotice)
        let saved = commit {
            $0.history.removeAll { $0.id == id }
            $0.history.insert(entry, at: 0)
            $0.history.sort { $0.createdAt > $1.createdAt }
        }
        if saved { historyID = id }
        return saved
    }

    func openHistory(_ entry: HistoryEntry) {
        guard !isBusy, pendingAudioFile == nil else { showError("请先结束当前任务并处理待转写录音。"); return }
        if hasDraft && !saveHistory(using: context ?? makeContext()) { return }
        let source = document.history.first { $0.id == entry.id } ?? entry
        rawText = source.rawText
        outputText = source.outputText
        elapsed = source.duration
        historyID = source.id
        draftNotice = source.notice
        context = nil
        selectedTab = 0
        persistDraft()
    }

    func deleteHistory(ids: Set<UUID>) {
        guard !isBusy else { return }
        if commit({ $0.history.removeAll { ids.contains($0.id) } }) {
            if let historyID, ids.contains(historyID) {
                self.historyID = nil
                persistDraft()
            }
            do { try KeyboardStore.remove(ids: ids) }
            catch { showError("历史已删除，但键盘暂存未能同步清理：\(error.localizedDescription)") }
        }
    }

    func clearDraft() {
        guard !isBusy else { return }
        let audio = pendingAudioFile
        guard commit({ $0.draft = DictationDraft() }) else { return }
        autosave?.cancel()
        resetDraftInMemory()
        if let audio {
            do { try storage?.removeAudio(fileName: audio) }
            catch { showError("草稿已清空，但录音文件删除失败：\(error.localizedDescription)") }
        }
    }

    func saveVocabulary(_ entry: VocabularyEntry) -> Bool {
        guard !document.vocabulary.contains(where: { $0.id != entry.id && $0.term.caseInsensitiveCompare(entry.term) == .orderedSame }) else {
            showError("词典里已经有这个词了。"); return false
        }
        return commit {
            $0.vocabulary.removeAll { $0.id == entry.id }
            $0.vocabulary.append(entry)
        }
    }

    func deleteVocabulary(ids: Set<UUID>) { commit { $0.vocabulary.removeAll { ids.contains($0.id) } } }

    func saveStyle(_ style: WritingStyle) -> Bool {
        guard !style.isBuiltIn else { return false }
        return commit {
            $0.customStyles.removeAll { $0.id == style.id }
            $0.customStyles.append(style)
        }
    }

    func deleteStyle(_ style: WritingStyle) {
        guard !style.isBuiltIn, !isBusy else { return }
        commit {
            $0.customStyles.removeAll { $0.id == style.id }
            if $0.settings.selectedStyleID == style.id { $0.settings.selectedStyleID = "raw" }
        }
    }

    func copy(_ text: String) {
        guard !text.isEmpty else { return }
        UIPasteboard.general.setItems([["public.utf8-plain-text": text]], options: [.localOnly: true])
        announce("已复制")
    }

    func publishToKeyboard(text: String, id: UUID? = nil, styleName: String? = nil) {
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        guard text.count <= 16_000 else { showError("键盘暂存单条最多 16,000 个字符，请分段发送。"); return }
        do {
            try KeyboardStore.publish(.init(id: id ?? historyID ?? UUID(), text: text,
                                            styleName: styleName ?? selectedStyle.name, createdAt: Date()))
            announce("已发送到键盘，切回目标应用即可插入")
        } catch { showError(error.localizedDescription) }
    }

    func clearKeyboard() {
        do { try KeyboardStore.clear(); announce("键盘暂存已清空") }
        catch { showError(error.localizedDescription) }
    }

    func sceneDidEnterBackground() {
        if phase == .recording { stopRecording() }
        else if phase == .authorizing { cancelWork() }
        persistDraft()
    }

    func showError(_ message: String) { alert = UserNotice(title: "OpenLess", message: message) }

    func announce(_ text: String) {
        feedbackTask?.cancel()
        feedback = text
        feedbackTask = Task { @MainActor [weak self] in
            do { try await Task.sleep(for: .seconds(3)) } catch { return }
            self?.feedback = nil
        }
    }

    private func makeContext() -> WorkContext {
        .init(settings: settings, style: selectedStyle, vocabulary: document.vocabulary)
    }

    private func resetDraftInMemory() {
        rawText = ""
        outputText = ""
        pendingAudioFile = nil
        elapsed = 0
        historyID = nil
        draftNotice = nil
        context = nil
    }
}
