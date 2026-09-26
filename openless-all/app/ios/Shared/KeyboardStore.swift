import Foundation

/// Only explicitly published text enters this container. Credentials stay in the app keychain.
enum KeyboardStore {
    private static func fileURL() throws -> URL {
        guard let identifier = Bundle.main.object(forInfoDictionaryKey: "OpenLessAppGroup") as? String,
              !identifier.isEmpty,
              let root = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: identifier) else {
            throw OpenLessError.message("键盘共享空间不可用，请确认主应用和键盘使用相同的 App Group 签名配置。")
        }
        return root.appendingPathComponent("keyboard-clips.json")
    }

    static func read() throws -> [KeyboardClip] {
        let url = try fileURL()
        guard FileManager.default.fileExists(atPath: url.path) else { return [] }
        return Array(try JSONDecoder().decode([KeyboardClip].self, from: Data(contentsOf: url)).prefix(10))
    }

    static func publish(_ clip: KeyboardClip) throws {
        var clips = try read().filter { $0.id != clip.id && $0.text != clip.text }
        clips.insert(clip, at: 0)
        try write(Array(clips.prefix(10)))
    }

    static func remove(ids: Set<UUID>) throws {
        try write(read().filter { !ids.contains($0.id) })
    }

    static func clear() throws { try write([]) }

    private static func write(_ clips: [KeyboardClip]) throws {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try encoder.encode(clips).write(to: fileURL(), options: [.atomic, .completeFileProtection])
    }
}
