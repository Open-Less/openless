import Foundation

struct LocalStore {
    let root: URL

    init() throws {
        root = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask,
                                           appropriateFor: nil, create: true)
            .appendingPathComponent("OpenLess", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: recordingsDirectory, withIntermediateDirectories: true)
    }

    var recordingsDirectory: URL { root.appendingPathComponent("Recordings", isDirectory: true) }
    private var documentURL: URL { root.appendingPathComponent("openless.json") }

    func load() throws -> AppDocument {
        guard FileManager.default.fileExists(atPath: documentURL.path) else { return AppDocument() }
        let document = try JSONDecoder().decode(AppDocument.self, from: Data(contentsOf: documentURL))
        guard document.schemaVersion == 1 else {
            throw OpenLessError.message("本地数据来自其他版本，请使用兼容版本打开。原文件已保留。")
        }
        return document
    }

    func save(_ document: AppDocument) throws {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try encoder.encode(document).write(to: documentURL, options: [.atomic, .completeFileProtection])
    }

    func audioURL(fileName: String) throws -> URL {
        guard fileName == (fileName as NSString).lastPathComponent,
              fileName.hasSuffix(".m4a"), UUID(uuidString: String(fileName.dropLast(4))) != nil else {
            throw OpenLessError.message("录音文件名无效。")
        }
        return recordingsDirectory.appendingPathComponent(fileName)
    }

    func newAudioURL() -> URL {
        recordingsDirectory.appendingPathComponent("\(UUID().uuidString).m4a")
    }

    func removeAudio(fileName: String) throws {
        let url = try audioURL(fileName: fileName)
        if FileManager.default.fileExists(atPath: url.path) {
            try FileManager.default.removeItem(at: url)
        }
    }
}
