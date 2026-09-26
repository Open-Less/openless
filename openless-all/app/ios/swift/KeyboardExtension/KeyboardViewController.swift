// OpenLess 键盘扩展主控制器（v1）。
//
// 流程：按住麦克风 → AVAudioRecorder 录 16k/mono/16-bit WAV → 松开 → 读
// App Group 的 kb-config.json（由主 App 键盘设置页写入）→ URLSession 直连
// OpenAI 兼容 /audio/transcriptions → textDocumentProxy.insertText。
//
// v1 不接入 Rust 润色管线：键盘扩展内存预算 ~60MB，完整 openless-core
// staticlib + tokio 风险大，且需要主 App 数据先迁移进 App Group。润色与
// 纠错词典复用走 src-tauri 的 C ABI 桥（M3+，见 ios/README.md）。
//
// 前提：系统设置 → 键盘 → OpenLess → 「允许完全访问」（网络与麦克风都需要）。

import AVFoundation
import UIKit

private struct TranscribeError: Error, CustomStringConvertible {
    let message: String
    var description: String { message }
}

private struct KeyboardConfig: Decodable {
    var endpoint: String
    var apiKey: String
    var model: String
    var prompt: String?

    static func load() -> KeyboardConfig? {
        guard
            let url = FileManager.default.containerURL(
                forSecurityApplicationGroupIdentifier: "group.com.openless.app"
            )?.appendingPathComponent("kb-config.json"),
            let data = try? Data(contentsOf: url),
            let config = try? JSONDecoder().decode(KeyboardConfig.self, from: data),
            !config.endpoint.isEmpty, !config.apiKey.isEmpty
        else {
            return nil
        }
        return config
    }
}

final class KeyboardViewController: UIInputViewController {
    private enum State {
        case idle
        case recording
        case transcribing
    }

    private var state: State = .idle {
        didSet { refreshAppearance() }
    }

    private let micButton = UIButton(type: .system)
    private let doneButton = UIButton(type: .system)
    private let statusLabel = UILabel()
    private var recorder: AVAudioRecorder?

    // MARK: - View lifecycle

    override func viewDidLoad() {
        super.viewDidLoad()
        buildInterface()
        refreshAppearance()
    }

    private func buildInterface() {
        view.backgroundColor = UIColor(
            red: 0.95, green: 0.95, blue: 0.96, alpha: 1.0
        )

        let row = UIStackView(arrangedSubviews: [doneButton, micButton])
        row.axis = .horizontal
        row.alignment = .center
        row.distribution = .equalSpacing
        row.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(row)

        statusLabel.textAlignment = .center
        statusLabel.font = .systemFont(ofSize: 11, weight: .regular)
        statusLabel.textColor = .secondaryLabel
        statusLabel.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(statusLabel)

        doneButton.setTitle("完成", for: .normal)
        doneButton.setTitleColor(.systemBlue, for: .normal)
        doneButton.titleLabel?.font = .systemFont(ofSize: 15, weight: .medium)
        doneButton.addTarget(self, action: #selector(dismissTapped), for: .touchUpInside)

        micButton.setTitle("🎤 按住说话", for: .normal)
        micButton.setTitleColor(.white, for: .normal)
        micButton.titleLabel?.font = .systemFont(ofSize: 16, weight: .semibold)
        micButton.backgroundColor = .systemBlue
        micButton.layer.cornerRadius = 8
        micButton.contentEdgeInsets = UIEdgeInsets(top: 10, left: 24, bottom: 10, right: 24)
        // 按住说话：长按手势驱动录音起止，与 Android IME 的交互对齐。
        let press = UILongPressGestureRecognizer(
            target: self, action: #selector(handleMicPress(_:))
        )
        press.minimumPressDuration = 0.05
        micButton.addGestureRecognizer(press)

        NSLayoutConstraint.activate([
            row.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 16),
            row.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -16),
            row.centerYAnchor.constraint(equalTo: view.centerYAnchor, constant: -8),
            statusLabel.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 16),
            statusLabel.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -16),
            statusLabel.bottomAnchor.constraint(equalTo: view.bottomAnchor, constant: -4),
        ])
    }

    private func refreshAppearance() {
        switch state {
        case .idle:
            statusLabel.text = configHint()
            micButton.backgroundColor = .systemBlue
        case .recording:
            statusLabel.text = "正在录音…松开结束"
            micButton.backgroundColor = .systemRed
        case .transcribing:
            statusLabel.text = "正在转写…"
            micButton.backgroundColor = .systemGray
        }
    }

    private func configHint() -> String {
        if KeyboardConfig.load() == nil {
            return "请在 OpenLess 主应用 → 设置 → 键盘 完成转写配置"
        }
        if !hasFullAccess {
            return "需在系统设置中开启「允许完全访问」（网络/麦克风）"
        }
        return "按住麦克风说话，松开自动转写"
    }

    @objc private func dismissTapped() {
        dismissKeyboard()
    }

    // MARK: - Recording

    @objc private func handleMicPress(_ gesture: UILongPressGestureRecognizer) {
        switch gesture.state {
        case .began:
            startRecording()
        case .ended, .cancelled:
            stopRecordingAndTranscribe()
        default:
            break
        }
    }

    private func startRecording() {
        guard state == .idle else { return }
        guard hasFullAccess else {
            statusLabel.text = "需「允许完全访问」才能使用麦克风"
            return
        }
        guard KeyboardConfig.load() != nil else {
            statusLabel.text = "请先在主应用完成键盘转写配置"
            return
        }

        let session = AVAudioSession.sharedInstance()
        session.requestRecordPermission { [weak self] granted in
            DispatchQueue.main.async {
                self?.beginRecordingAfterPermission(granted)
            }
        }
    }

    private func beginRecordingAfterPermission(_ granted: Bool) {
        guard granted else {
            statusLabel.text = "麦克风权限被拒绝，请到系统设置开启"
            return
        }
        do {
            let session = AVAudioSession.sharedInstance()
            try session.setCategory(.playAndRecord, mode: .measurement, options: [.duckOthers])
            try session.setActive(true)

            let settings: [String: Any] = [
                AVFormatIDKey: kAudioFormatLinearPCM,
                AVSampleRateKey: 16_000.0,
                AVNumberOfChannelsKey: 1,
                AVLinearPCMBitDepthKey: 16,
                AVLinearPCMIsFloatKey: false,
                AVLinearPCMIsBigEndianKey: false,
            ]
            let url = Self.recordingURL()
            let recorder = try AVAudioRecorder(url: url, settings: settings)
            recorder.record()
            self.recorder = recorder
            state = .recording
        } catch {
            statusLabel.text = "录音启动失败：\(error.localizedDescription)"
        }
    }

    private func stopRecordingAndTranscribe() {
        guard state == .recording, let recorder = recorder else { return }
        recorder.stop()
        self.recorder = nil
        try? AVAudioSession.sharedInstance().setActive(
            false, options: .notifyOthersOnDeactivation
        )
        state = .transcribing

        let wavURL = Self.recordingURL()
        let config = KeyboardConfig.load()
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let self, let config else {
                DispatchQueue.main.async { self?.finish(error: "配置缺失") }
                return
            }
            let result = Self.transcribe(wavURL: wavURL, config: config)
            DispatchQueue.main.async {
                switch result {
                case .success(let text):
                    self.finish(inserting: text)
                case .failure(let error):
                    self.finish(error: error.description)
                }
            }
        }
    }

    private func finish(inserting text: String) {
        state = .idle
        if !text.isEmpty {
            textDocumentProxy.insertText(text)
        }
    }

    private func finish(error message: String) {
        state = .idle
        statusLabel.text = message
    }

    // MARK: - Transcription

    private static func recordingURL() -> URL {
        let base = FileManager.default.containerURL(
            forSecurityApplicationGroupIdentifier: "group.com.openless.app"
        ) ?? FileManager.default.temporaryDirectory
        return base.appendingPathComponent("kb-dictation.wav")
    }

    private static func transcribe(wavURL: URL, config: KeyboardConfig) -> Result<String, TranscribeError> {
        guard
            let endpoint = URL(string: config.endpoint.hasSuffix("/")
                ? config.endpoint + "audio/transcriptions"
                : config.endpoint + "/audio/transcriptions")
        else {
            return .failure(TranscribeError(message: "转写端点格式错误"))
        }

        var request = URLRequest(url: endpoint)
        request.httpMethod = "POST"
        request.timeoutInterval = 60
        request.setValue("Bearer \(config.apiKey)", forHTTPHeaderField: "Authorization")

        let boundary = "OpenLessKeyboard-\(UUID().uuidString)"
        request.setValue(
            "multipart/form-data; boundary=\(boundary)", forHTTPHeaderField: "Content-Type"
        )

        guard let audio = try? Data(contentsOf: wavURL) else {
            return .failure(TranscribeError(message: "录音文件读取失败"))
        }

        var body = Data()
        func appendField(_ name: String, _ value: String) {
            body.append("--\(boundary)\r\n".data(using: .utf8)!)
            body.append(
                "Content-Disposition: form-data; name=\"\(name)\"\r\n\r\n".data(using: .utf8)!
            )
            body.append(value.data(using: .utf8)!)
            body.append("\r\n".data(using: .utf8)!)
        }
        appendField("model", config.model)
        if let prompt = config.prompt, !prompt.isEmpty {
            appendField("prompt", prompt)
        }
        body.append("--\(boundary)\r\n".data(using: .utf8)!)
        body.append(
            "Content-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\n"
                .data(using: .utf8)!
        )
        body.append("Content-Type: audio/wav\r\n\r\n".data(using: .utf8)!)
        body.append(audio)
        body.append("\r\n--\(boundary)--\r\n".data(using: .utf8)!)
        request.httpBody = body

        // 键盘扩展进程没有 RunLoop 常驻信号量问题：用信号量同步等待。
        let semaphore = DispatchSemaphore(value: 0)
        var payload: (Data?, URLResponse?, Error?)
        URLSession.shared.dataTask(with: request) { data, response, error in
            payload = (data, response, error)
            semaphore.signal()
        }.resume()
        _ = semaphore.wait(timeout: .now() + 60)

        if let error = payload.2 {
            return .failure(TranscribeError(message: "网络错误：\(error.localizedDescription)"))
        }
        guard let http = payload.1 as? HTTPURLResponse, let data = payload.0 else {
            return .failure(TranscribeError(message: "转写服务无响应"))
        }
        guard (200 ..< 300).contains(http.statusCode) else {
            let message = String(data: data, encoding: .utf8) ?? ""
            return .failure(TranscribeError(message: "转写失败（\(http.statusCode)）：\(message.prefix(200))"))
        }
        guard
            let text = (try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["text"]
                as? String
        else {
            return .failure(TranscribeError(message: "转写响应格式异常"))
        }
        try? FileManager.default.removeItem(at: wavURL)
        return .success(text)
    }
}
