import AVFoundation
import Accelerate
import Speech
import Foundation

struct CaptureResult {
    var text: String
    var audioURL: URL?
    var duration: TimeInterval
    var notice: String?
}

@MainActor
final class AudioCapture: NSObject, AVAudioRecorderDelegate {
    var onUpdate: ((String, TimeInterval, Double) -> Void)?
    var onStopRequested: (() -> Void)?
    var onRecordingReady: ((URL, TimeInterval) -> Void)?

    private var engine: AVAudioEngine?
    private var fileRecorder: AVAudioRecorder?
    private var recognizer: SFSpeechRecognizer?
    private var speechRequest: SFSpeechAudioBufferRecognitionRequest?
    private var recognitionTask: SFSpeechRecognitionTask?
    private var meterTask: Task<Void, Never>?
    private var finalTimeout: Task<Void, Never>?
    private var finishContinuation: CheckedContinuation<CaptureResult, Error>?
    private var observers: [NSObjectProtocol] = []
    private var sessionID = UUID()
    private var startedAt = Date()
    private var text = ""
    private var duration: TimeInterval = 0
    private var level: Double = 0
    private var fileURL: URL?
    private var notice: String?
    private var speechError: Error?
    private var hasFinalResult = false
    private var capturing = false
    private var tapInstalled = false

    override init() {
        super.init()
        let center = NotificationCenter.default
        observers.append(center.addObserver(forName: AVAudioSession.interruptionNotification,
                                             object: nil, queue: .main) { [weak self] notification in
            guard let raw = notification.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt,
                  AVAudioSession.InterruptionType(rawValue: raw) == .began else { return }
            Task { @MainActor [weak self] in self?.requestStop(notice: "录音被电话或其他音频打断，已保留当前内容。") }
        })
        observers.append(center.addObserver(forName: AVAudioSession.routeChangeNotification,
                                             object: nil, queue: .main) { [weak self] notification in
            guard let raw = notification.userInfo?[AVAudioSessionRouteChangeReasonKey] as? UInt,
                  AVAudioSession.RouteChangeReason(rawValue: raw) == .oldDeviceUnavailable else { return }
            Task { @MainActor [weak self] in self?.requestStop(notice: "录音设备已断开，已结束本次录音。") }
        })
    }

    deinit { observers.forEach { NotificationCenter.default.removeObserver($0) } }

    func start(settings: AppSettings, vocabulary: [VocabularyEntry], outputURL: URL) async throws {
        cancel()
        let id = sessionID
        guard await AVAudioApplication.requestRecordPermission() else {
            throw OpenLessError.message("需要麦克风权限才能听写。请在系统设置中允许 OpenLess 使用麦克风。")
        }
        try ensureActive(id)
        if settings.recognitionProvider == .apple {
            let authorization = await withCheckedContinuation { continuation in
                SFSpeechRecognizer.requestAuthorization { continuation.resume(returning: $0) }
            }
            try ensureActive(id)
            guard authorization == .authorized else {
                throw OpenLessError.message("需要语音识别权限。请在系统设置中允许 OpenLess 使用语音识别。")
            }
        }
        do {
            let audioSession = AVAudioSession.sharedInstance()
            try audioSession.setCategory(.record, mode: .measurement, options: [.allowBluetooth])
            try audioSession.setActive(true)
            if settings.recognitionProvider == .apple {
                try startApple(settings: settings, vocabulary: vocabulary, id: id)
            } else {
                let recorder = try AVAudioRecorder(url: outputURL, settings: [
                    AVFormatIDKey: kAudioFormatMPEG4AAC,
                    AVSampleRateKey: 44_100,
                    AVNumberOfChannelsKey: 1,
                    AVEncoderAudioQualityKey: AVAudioQuality.high.rawValue
                ])
                fileURL = outputURL
                fileRecorder = recorder
                recorder.delegate = self
                recorder.isMeteringEnabled = true
                guard recorder.prepareToRecord(), recorder.record() else {
                    throw OpenLessError.message("无法开始录音，请确认麦克风未被占用。")
                }
                try FileManager.default.setAttributes([.protectionKey: FileProtectionType.complete],
                                                      ofItemAtPath: outputURL.path)
            }
            capturing = true
            startedAt = Date()
            meterTask = Task { @MainActor [weak self] in
                while !Task.isCancelled {
                    do { try await Task.sleep(for: .milliseconds(100)) } catch { return }
                    guard let self, self.sessionID == id, self.capturing else { return }
                    self.duration = Date().timeIntervalSince(self.startedAt)
                    if let recorder = self.fileRecorder {
                        recorder.updateMeters()
                        self.level = Self.normalizedLevel(Double(recorder.averagePower(forChannel: 0)))
                    }
                    self.onUpdate?(self.text, self.duration, self.level)
                    if self.duration >= Double(settings.effectiveRecordingLimit) {
                        self.requestStop(notice: "已达到本次录音时长上限。")
                        return
                    }
                }
            }
        } catch {
            cancel()
            throw error
        }
    }

    private func startApple(settings: AppSettings, vocabulary: [VocabularyEntry], id: UUID) throws {
        guard let recognizer = SFSpeechRecognizer(locale: Locale(identifier: settings.speechLocale)),
              recognizer.isAvailable else {
            throw OpenLessError.message("所选语言的 Apple 语音识别暂不可用，请更换语言或使用兼容转写服务。")
        }
        if settings.onDeviceOnly && !recognizer.supportsOnDeviceRecognition {
            throw OpenLessError.message("当前设备或语言不支持离线识别。可在设置中更换语言，或关闭“仅在设备上识别”。")
        }
        self.recognizer = recognizer
        let request = SFSpeechAudioBufferRecognitionRequest()
        request.shouldReportPartialResults = true
        request.requiresOnDeviceRecognition = settings.onDeviceOnly
        request.addsPunctuation = true
        request.taskHint = .dictation
        request.contextualStrings = Array(vocabulary.prefix(100).map(\.term))
        speechRequest = request
        recognitionTask = recognizer.recognitionTask(with: request) { [weak self] result, error in
            Task { @MainActor [weak self] in
                guard let self, self.sessionID == id else { return }
                if let result {
                    self.text = result.bestTranscription.formattedString
                    self.hasFinalResult = result.isFinal
                    self.onUpdate?(self.text, self.duration, self.level)
                }
                if let error, !self.hasFinalResult { self.speechError = error }
                if self.hasFinalResult || self.speechError != nil {
                    if self.finishContinuation != nil {
                        self.finishApple()
                    } else if self.capturing {
                        self.onStopRequested?()
                    }
                }
            }
        }
        let engine = AVAudioEngine()
        self.engine = engine
        let input = engine.inputNode
        let format = input.outputFormat(forBus: 0)
        guard format.sampleRate > 0, format.channelCount > 0 else {
            throw OpenLessError.message("没有可用的麦克风输入。")
        }
        input.installTap(onBus: 0, bufferSize: 1_024, format: format) { [weak self] buffer, _ in
            request.append(buffer)
            var rms: Float = 0
            if let samples = buffer.floatChannelData?[0], buffer.frameLength > 0 {
                vDSP_rmsqv(samples, 1, &rms, vDSP_Length(buffer.frameLength))
            }
            let decibels = 20 * log10(Double(max(rms, 0.000_001)))
            Task { @MainActor [weak self] in
                guard let self, self.sessionID == id else { return }
                self.level = Self.normalizedLevel(decibels)
            }
        }
        tapInstalled = true
        engine.prepare()
        try engine.start()
    }

    func stop() async throws -> CaptureResult {
        guard capturing else { throw OpenLessError.message("当前没有正在进行的录音。") }
        duration = Date().timeIntervalSince(startedAt)
        stopHardware()
        if let fileURL {
            self.fileURL = nil // Ownership passes to the draft; failures can retry this recording.
            onRecordingReady?(fileURL, duration)
            return CaptureResult(text: "", audioURL: fileURL, duration: duration, notice: notice)
        }
        speechRequest?.endAudio()
        let id = sessionID
        return try await withTaskCancellationHandler {
            try Task.checkCancellation()
            return try await withCheckedThrowingContinuation { continuation in
                finishContinuation = continuation
                if hasFinalResult || speechError != nil {
                    finishApple()
                } else {
                    finalTimeout = Task { @MainActor [weak self] in
                        do { try await Task.sleep(for: .seconds(4)) } catch { return }
                        guard let self, self.sessionID == id else { return }
                        self.notice = self.notice ?? "最终识别等待超时，已保留当前转写。"
                        self.finishApple()
                    }
                }
            }
        } onCancel: {
            Task { @MainActor [weak self] in
                guard let self, self.sessionID == id else { return }
                self.cancel()
            }
        }
    }

    private func finishApple() {
        guard let continuation = finishContinuation else { return }
        finishContinuation = nil
        finalTimeout?.cancel()
        finalTimeout = nil
        let result: Result<CaptureResult, Error>
        if text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            result = .failure(speechError ?? OpenLessError.message("没有识别到语音，请靠近麦克风后重试。"))
        } else {
            let warning = speechError.map { "语音识别提前结束，已保留当前文字。\($0.localizedDescription)" }
            result = .success(CaptureResult(text: text, audioURL: nil, duration: duration, notice: warning ?? notice))
        }
        sessionID = UUID()
        recognitionTask?.cancel()
        recognitionTask = nil
        speechRequest = nil
        recognizer = nil
        continuation.resume(with: result)
    }

    func cancel() {
        sessionID = UUID()
        stopHardware()
        finalTimeout?.cancel()
        finalTimeout = nil
        recognitionTask?.cancel()
        recognitionTask = nil
        speechRequest = nil
        recognizer = nil
        finishContinuation?.resume(throwing: CancellationError())
        finishContinuation = nil
        if let fileURL { try? FileManager.default.removeItem(at: fileURL) }
        fileURL = nil
        text = ""
        duration = 0
        level = 0
        hasFinalResult = false
        speechError = nil
        notice = nil
    }

    private func stopHardware() {
        capturing = false
        meterTask?.cancel()
        meterTask = nil
        if tapInstalled { engine?.inputNode.removeTap(onBus: 0); tapInstalled = false }
        engine?.stop()
        engine = nil
        fileRecorder?.stop()
        fileRecorder = nil
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }

    private func requestStop(notice: String) {
        guard capturing else { return }
        self.notice = notice
        onStopRequested?()
    }

    private func ensureActive(_ id: UUID) throws {
        try Task.checkCancellation()
        guard id == sessionID else { throw CancellationError() }
    }

    private static func normalizedLevel(_ decibels: Double) -> Double {
        min(1, max(0, (decibels + 55) / 55))
    }

    nonisolated func audioRecorderEncodeErrorDidOccur(_ recorder: AVAudioRecorder, error: Error?) {
        Task { @MainActor [weak self] in
            guard let self, self.fileRecorder === recorder else { return }
            self.requestStop(notice: "录音写入被打断，请重试转写或重新录音。")
        }
    }

    nonisolated func audioRecorderDidFinishRecording(_ recorder: AVAudioRecorder, successfully flag: Bool) {
        Task { @MainActor [weak self] in
            guard let self, self.fileRecorder === recorder, self.capturing else { return }
            self.requestStop(notice: flag ? "录音已结束。" : "录音意外结束，请确认转写内容。")
        }
    }
}
