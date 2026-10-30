import Darwin
import Foundation
import Testing
@testable import OpenLess

@MainActor
struct StyleArchiveImportTests {
  private func directory() throws -> URL {
    let url = FileManager.default.temporaryDirectory.appendingPathComponent("style-copy-tests-" + UUID().uuidString)
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: false)
    return url
  }
  private func policy(_ path: URL, limit: Int = 524288) -> StyleArchivePolicy {
    StyleArchivePolicy(format: "zip", maxCompressedBytes: limit, stagingDirectory: path.path)
  }

  @Test func exact512KiBBoundaryCopiesPrivatelyWithGeneratedLeaf() throws {
    let root = try directory(); defer { try? FileManager.default.removeItem(at: root) }
    let source = root.appendingPathComponent("archive.zip")
    try Data(repeating: 65, count: StyleArchiveImport.maximumBytes).write(to: source)
    let result = try StyleArchiveImport.stage(source: source, policy: policy(root))
    defer { try? result.remove() }
    #expect(UUID(uuidString: String(result.leaf.dropLast(4))) != nil)
    let attributes = try FileManager.default.attributesOfItem(atPath: result.url.path)
    #expect(attributes[.size] as? Int == 524288)
    #expect(attributes[.posixPermissions] as? Int == 0o600)
    #expect(try Data(contentsOf: result.url) == Data(contentsOf: source))
  }

  @Test func oversizeAndSmallerHostLimitRejectWithoutPartialCopies() throws {
    let root = try directory(); defer { try? FileManager.default.removeItem(at: root) }
    let source = root.appendingPathComponent("archive.zip")
    try Data(repeating: 0, count: 524289).write(to: source)
    #expect(throws: StyleArchiveImport.Failure.self) { try StyleArchiveImport.stage(source: source, policy: policy(root)) }
    try Data(repeating: 0, count: 1025).write(to: source)
    #expect(throws: StyleArchiveImport.Failure.self) { try StyleArchiveImport.stage(source: source, policy: policy(root, limit: 1024)) }
    #expect(try FileManager.default.contentsOfDirectory(atPath: root.path) == ["archive.zip"])
  }

  @Test func unsupportedExtensionDirectoryAndSourceSymlinkAreRejected() throws {
    let root = try directory(); defer { try? FileManager.default.removeItem(at: root) }
    let text = root.appendingPathComponent("archive.txt")
    try Data().write(to: text)
    #expect(throws: StyleArchiveImport.Failure.self) { try StyleArchiveImport.stage(source: text, policy: policy(root)) }
    let link = root.appendingPathComponent("link.zip")
    try FileManager.default.createSymbolicLink(at: link, withDestinationURL: text)
    #expect(throws: StyleArchiveImport.Failure.self) { try StyleArchiveImport.stage(source: link, policy: policy(root)) }
    let folder = root.appendingPathComponent("folder.zip")
    try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: false)
    #expect(throws: StyleArchiveImport.Failure.self) { try StyleArchiveImport.stage(source: folder, policy: policy(root)) }
  }

  @Test func nonexistentOrSymlinkStagingRejectsWithoutTouchingSource() throws {
    let root = try directory(); defer { try? FileManager.default.removeItem(at: root) }
    let source = root.appendingPathComponent("source.zip")
    try Data("original".utf8).write(to: source)
    let missing = root.appendingPathComponent("missing")
    #expect(throws: StyleArchiveImport.Failure.self) { try StyleArchiveImport.stage(source: source, policy: policy(missing)) }
    let link = root.appendingPathComponent("linked")
    try FileManager.default.createSymbolicLink(at: link, withDestinationURL: root)
    #expect(throws: StyleArchiveImport.Failure.self) { try StyleArchiveImport.stage(source: source, policy: policy(link)) }
    #expect(try String(contentsOf: source, encoding: .utf8) == "original")
  }

  @Test func cancelledCopyLeavesNoPartialFile() async throws {
    let root = try directory(); defer { try? FileManager.default.removeItem(at: root) }
    let source = root.appendingPathComponent("source.zip")
    try Data(repeating: 0, count: 524288).write(to: source)
    // Cancel before the synchronous copy enters its bounded loop.
    let task = Task {
      withUnsafeCurrentTask { $0?.cancel() }
      return try StyleArchiveImport.stage(source: source, policy: policy(root))
    }
    do { _ = try await task.value; Issue.record("Expected cancellation") }
    catch { #expect(error is CancellationError) }
    #expect(try FileManager.default.contentsOfDirectory(atPath: root.path) == ["source.zip"])
  }
}
