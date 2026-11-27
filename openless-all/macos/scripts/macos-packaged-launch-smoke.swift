import CoreFoundation
import CoreGraphics
import Darwin
import Foundation
import ImageIO

let arguments = CommandLine.arguments
precondition(arguments.count == 4, "usage: macos-packaged-launch-smoke.swift APP SHA EVIDENCE_DIR")
let app = URL(fileURLWithPath: arguments[1]).standardizedFileURL
let sha = arguments[2]
let evidence = URL(fileURLWithPath: arguments[3]).standardizedFileURL
try FileManager.default.createDirectory(at: evidence, withIntermediateDirectories: true)

func writeResult(_ fields: [String: String]) throws {
    let values = fields.merging(["source_sha": sha, "app": app.lastPathComponent]) { original, _ in original }
    let report = values.keys.sorted().map { "\($0)=\(values[$0]!)" }.joined(separator: "\n") + "\n"
    try report.write(to: evidence.appendingPathComponent("result.txt"), atomically: true, encoding: .utf8)
    print(report, terminator: "")
}

let process = Process()
var launched = false

func cleanup() {
    guard launched, process.isRunning else { return }
    process.terminate()
    for _ in 0..<10 {
        if !process.isRunning { return }
        Thread.sleep(forTimeInterval: 0.1)
    }
    if process.isRunning { kill(process.processIdentifier, SIGKILL) }
}

func finish(_ fields: [String: String], failed: Bool = false) throws -> Never {
    cleanup()
    try writeResult(fields)
    exit(failed ? EXIT_FAILURE : EXIT_SUCCESS)
}

guard let bundle = Bundle(url: app), bundle.bundleIdentifier == "com.openless.app",
      let executable = bundle.executableURL else {
    try finish(["status": "invalid_bundle"], failed: true)
}

func windows() -> [[String: Any]]? {
    CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID)
        as? [[String: Any]]
}

let guiAvailable = CGMainDisplayID() != 0 && windows() != nil
process.executableURL = executable
process.currentDirectoryURL = app.deletingLastPathComponent()
// Leave build and signing credentials out of the child environment and out of artifacts.
let allowed = Set(["HOME", "PATH", "TMPDIR", "USER", "LOGNAME", "LANG"])
process.environment = ProcessInfo.processInfo.environment.filter { allowed.contains($0.key) }
process.standardInput = FileHandle.nullDevice
process.standardOutput = FileHandle.nullDevice
process.standardError = FileHandle.nullDevice

do {
    try process.run()
    launched = true
} catch {
    try finish([
        "status": guiAvailable ? "launch_failed" : "gui_unavailable",
        "process": "could_not_spawn",
        "window": "not_verified",
        "screenshot": "not_attempted",
    ], failed: guiAvailable)
}

if !guiAvailable {
    Thread.sleep(forTimeInterval: 2)
    try finish([
        "status": "gui_unavailable",
        "process": process.isRunning ? "running" : "exited_\(process.terminationStatus)",
        "window": "not_verified",
        "screenshot": "not_attempted",
    ])
}

func appWindow() -> (id: UInt32, bounds: CGRect)? {
    for window in windows() ?? [] {
        guard (window[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value == process.processIdentifier,
              (window[kCGWindowLayer as String] as? NSNumber)?.intValue == 0,
              let number = window[kCGWindowNumber as String] as? NSNumber,
              let rawBounds = window[kCGWindowBounds as String] as? [String: Any],
              let bounds = CGRect(dictionaryRepresentation: rawBounds as CFDictionary),
              // Exclude the caption, tray, and other auxiliary windows.
              bounds.width >= 700, bounds.height >= 480 else { continue }
        return (number.uint32Value, bounds)
    }
    return nil
}

let deadline = Date().addingTimeInterval(35)
var visibleWindow: (id: UInt32, bounds: CGRect)?
var previousWindowID: UInt32?
while Date() < deadline {
    guard process.isRunning else {
        try finish([
            "status": "launch_failed", "process": "exited_\(process.terminationStatus)",
            "window": "not_verified", "screenshot": "not_attempted",
        ], failed: true)
    }
    let current = appWindow()
    if let current, current.id == previousWindowID {
        visibleWindow = current
        break
    }
    previousWindowID = current?.id
    Thread.sleep(forTimeInterval: 0.5)
}

guard let window = visibleWindow, process.isRunning else {
    try finish([
        "status": "window_missing", "process": process.isRunning ? "running" : "exited",
        "window": "not_verified", "screenshot": "not_attempted",
    ], failed: true)
}

var fields = [
    "status": "window_verified",
    "process": "running",
    "window": "\(Int(window.bounds.width))x\(Int(window.bounds.height))",
]
let screenshot = evidence.appendingPathComponent("window.png")
if CGPreflightScreenCaptureAccess() {
    let capture = Process()
    capture.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
    capture.arguments = ["-x", "-o", "-l", String(window.id), screenshot.path]
    capture.environment = process.environment
    capture.standardOutput = FileHandle.nullDevice
    capture.standardError = FileHandle.nullDevice
    do {
        try capture.run()
        capture.waitUntilExit()
        if capture.terminationStatus != 0 { try? FileManager.default.removeItem(at: screenshot) }
    } catch {
        try? FileManager.default.removeItem(at: screenshot)
    }
    if let source = CGImageSourceCreateWithURL(screenshot as CFURL, nil),
       let image = CGImageSourceCreateImageAtIndex(source, 0, nil), image.width > 0, image.height > 0 {
        fields["screenshot"] = "captured"
    } else {
        try? FileManager.default.removeItem(at: screenshot)
        fields["screenshot"] = "capture_unavailable"
    }
} else {
    fields["screenshot"] = "screen_capture_permission_unavailable"
}
try finish(fields)
