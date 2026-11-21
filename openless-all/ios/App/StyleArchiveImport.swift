import Darwin
import Foundation

/// Only a bounded file copy lives in Swift. Core owns ZIP/content validation.
enum StyleArchiveImport {
  nonisolated static let maximumBytes = 512 * 1024
  enum Failure: LocalizedError {
    case unsupported, oversized, invalidStaging, unreadable, writeFailed, cleanupFailed
    var errorDescription: String? {
      switch self {
      case .unsupported: "请选择 ZIP 风格包。"
      case .oversized: "风格包不能超过 512 KiB（或 Core 返回的更小限制）。"
      case .invalidStaging: "Core 私有导入目录不可用，请刷新后重试。"
      case .unreadable: "无法读取此文件，请重新选择文件。"
      case .writeFailed: "无法保存临时风格包；尚未发送导入请求。"
      case .cleanupFailed: "尚未发送导入请求，但临时文件清理失败；请检查应用存储。"
      }
    }
  }

  struct Staged: Sendable {
    let url: URL
    var leaf: String { url.lastPathComponent }
    func remove() throws { try FileManager.default.removeItem(at: url) }
  }

  nonisolated static func stage(source: URL, policy: StyleArchivePolicy) throws -> Staged {
    guard source.isFileURL, source.pathExtension.lowercased() == "zip", policy.format == "zip" else {
      throw Failure.unsupported
    }
    guard policy.maxCompressedBytes > 0, policy.stagingDirectory.hasPrefix("/") else {
      throw Failure.invalidStaging
    }
    let limit = min(maximumBytes, policy.maxCompressedBytes)
    let scoped = source.startAccessingSecurityScopedResource()
    defer { if scoped { source.stopAccessingSecurityScopedResource() } }
    let input = open(source.path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC)
    guard input >= 0 else { throw Failure.unreadable }
    defer { close(input) }
    var metadata = stat()
    guard fstat(input, &metadata) == 0, metadata.st_mode & S_IFMT == S_IFREG else {
      throw Failure.unreadable
    }
    guard metadata.st_size <= limit else { throw Failure.oversized }
    let directory = open(policy.stagingDirectory, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
    guard directory >= 0 else { throw Failure.invalidStaging }
    defer { close(directory) }
    let leaf = UUID().uuidString + ".zip"
    let output = openat(directory, leaf, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0o600)
    guard output >= 0 else { throw Failure.writeFailed }
    defer { close(output) }
    do {
      var buffer = [UInt8](repeating: 0, count: 16 * 1024)
      var total = 0
      while true {
        try Task.checkCancellation()
        // Read at most limit + 1, even if the source grows after fstat.
        let count = buffer.withUnsafeMutableBytes { bytes in
          Darwin.read(input, bytes.baseAddress, min(bytes.count, limit - total + 1))
        }
        if count == 0 { break }
        if count < 0 { if errno == EINTR { continue }; throw Failure.unreadable }
        total += count
        guard total <= limit else { throw Failure.oversized }
        try buffer.withUnsafeBytes { bytes in
          var written = 0
          while written < count {
            let size = Darwin.write(output, bytes.baseAddress!.advanced(by: written), count - written)
            if size < 0 && errno == EINTR { continue }
            guard size > 0 else { throw Failure.writeFailed }
            written += size
          }
        }
      }
      guard fsync(output) == 0 else { throw Failure.writeFailed }
    } catch {
      guard unlinkat(directory, leaf, 0) == 0 || errno == ENOENT else { throw Failure.cleanupFailed }
      throw error
    }
    return Staged(url: URL(fileURLWithPath: policy.stagingDirectory, isDirectory: true).appendingPathComponent(leaf))
  }
}
