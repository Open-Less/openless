// Bounded Dev launch smoke against the application extracted from the final DMG.
import AppKit
import ApplicationServices
import CoreGraphics
import Foundation
import ImageIO
import Vision

let args = CommandLine.arguments
precondition(args.count == 4, "usage: macos-artifact-acceptance.swift APP VERSION EVIDENCE_DIR")
precondition(ProcessInfo.processInfo.environment["CI"] == "true", "CI only; do not launch on a user's host")
let app = URL(fileURLWithPath: args[1])
let expectedVersion = args[2]
let evidence = URL(fileURLWithPath: args[3])
try FileManager.default.createDirectory(at: evidence, withIntermediateDirectories: true)
let started = Date()
let process = Process()
var fields: [String: String] = ["status": "incomplete", "runtime_sha": "not_probed"]
var events: [String] = []
func record(_ message: String) {
    let line = String(format: "%.2fs %@", Date().timeIntervalSince(started), message)
    events.append(line)
    print(line)
}
func attribute(_ element: AXUIElement, _ key: String) -> CFTypeRef? {
    var value: CFTypeRef?
    guard AXUIElementCopyAttributeValue(element, key as CFString, &value) == .success else { return nil }
    return value
}
struct Node {
    let element: AXUIElement
    let role: String
    let text: String
    let enabled: Bool?
}
func nodes() -> [Node] {
    var result: [Node] = []
    func walk(_ element: AXUIElement, _ depth: Int) {
        guard depth < 35, result.count < 2500 else { return }
        let role = attribute(element, kAXRoleAttribute) as? String ?? ""
        let text = [kAXTitleAttribute, kAXDescriptionAttribute, kAXValueAttribute]
            .compactMap { attribute(element, $0) as? String }.filter { !$0.isEmpty }.joined(separator: " | ")
        result.append(Node(element: element, role: role, text: text,
                           enabled: attribute(element, kAXEnabledAttribute) as? Bool))
        for child in attribute(element, kAXChildrenAttribute) as? [AXUIElement] ?? [] { walk(child, depth + 1) }
    }
    walk(AXUIElementCreateApplication(process.processIdentifier), 0)
    return result
}
func dump(_ name: String, _ tree: [Node]) {
    let text = tree.map { "\($0.role) enabled=\($0.enabled.map(String.init) ?? "unknown") \($0.text)" }.joined(separator: "\n")
    try? text.write(to: evidence.appendingPathComponent("\(name)-ax.txt"), atomically: true, encoding: .utf8)
}
func window() -> (UInt32, CGRect)? {
    let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
    for item in list {
        guard (item[kCGWindowOwnerPID as String] as? NSNumber)?.int32Value == process.processIdentifier,
              (item[kCGWindowLayer as String] as? NSNumber)?.intValue == 0,
              let number = item[kCGWindowNumber as String] as? NSNumber,
              let raw = item[kCGWindowBounds as String] as? [String: Any],
              let rect = CGRect(dictionaryRepresentation: raw as CFDictionary),
              rect.width >= 700, rect.height >= 480 else { continue }
        return (number.uint32Value, rect)
    }
    return nil
}
@discardableResult func capture(_ name: String) -> String {
    guard CGPreflightScreenCaptureAccess(), let (id, _) = window() else { return "" }
    let path = evidence.appendingPathComponent("\(name).png")
    let child = Process()
    child.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
    child.arguments = ["-x", "-o", "-l", String(id), path.path]
    child.environment = process.environment
    try? child.run()
    child.waitUntilExit()
    guard child.terminationStatus == 0 else { return "" }
    let request = VNRecognizeTextRequest()
    request.recognitionLevel = .accurate
    request.recognitionLanguages = ["en-US", "zh-Hans"]
    try? VNImageRequestHandler(url: path).perform([request])
    let text = (request.results ?? []).compactMap { $0.topCandidates(1).first?.string }.joined(separator: "\n")
    try? text.write(to: evidence.appendingPathComponent("\(name)-ocr.txt"), atomically: true, encoding: .utf8)
    record("screenshot=\(name) OCR_lines=\(request.results?.count ?? 0)")
    return text
}
func finish(_ status: String, _ reason: String) -> Never {
    fields["status"] = status
    fields["reason"] = reason
    fields["process_before_cleanup"] = process.isRunning ? "running" : "exited"
    if process.processIdentifier > 0 {
        dump("final", nodes())
        capture("final")
    }
    if process.isRunning {
        process.terminate()
        for _ in 0..<20 {
            if !process.isRunning { break }
            Thread.sleep(forTimeInterval: 0.1)
        }
        if process.isRunning { kill(process.processIdentifier, SIGKILL) }
        process.waitUntilExit()
    }
    fields["process_reaped"] = "true"
    try? events.joined(separator: "\n").write(to: evidence.appendingPathComponent("timeline.txt"), atomically: true, encoding: .utf8)
    let report = fields.keys.sorted().map { "\($0)=\(fields[$0]!)" }.joined(separator: "\n") + "\n"
    try? report.write(to: evidence.appendingPathComponent("result.txt"), atomically: true, encoding: .utf8)
    print(report)
    exit(status == "PASS" ? 0 : 1)
}
guard let bundle = Bundle(url: app), bundle.bundleIdentifier == "com.openless.app.dev",
      let executable = bundle.executableURL else { finish("FAIL", "invalid_bundle") }
let version = bundle.infoDictionary?["CFBundleShortVersionString"] as? String ?? ""
fields["bundle_version"] = version
guard version == expectedVersion else { finish("FAIL", "bundle_version_mismatch") }
process.executableURL = executable
process.currentDirectoryURL = app.deletingLastPathComponent()
let allowed = Set(["HOME", "PATH", "TMPDIR", "USER", "LOGNAME", "LANG"])
process.environment = ProcessInfo.processInfo.environment.filter { allowed.contains($0.key) }
process.standardInput = FileHandle.nullDevice
for name in ["stdout", "stderr"] {
    let path = evidence.appendingPathComponent("\(name).log")
    FileManager.default.createFile(atPath: path.path, contents: nil)
    let handle = try FileHandle(forWritingTo: path)
    if name == "stdout" { process.standardOutput = handle } else { process.standardError = handle }
}
do { try process.run() } catch { finish("FAIL", "spawn_failed: \(error)") }
record("spawned pid=\(process.processIdentifier) bundle_version=\(version)")
fields["ax_trusted"] = String(AXIsProcessTrusted())
fields["screen_capture_allowed"] = String(CGPreflightScreenCaptureAccess())
var tree: [Node] = []
func waitFor(_ label: String, seconds: Double = 40, predicate: ([Node]) -> Bool) -> [Node] {
    let deadline = Date().addingTimeInterval(seconds)
    while Date() < deadline {
        guard process.isRunning else { finish("FAIL", "process_exited_\(process.terminationStatus)") }
        let current = nodes()
        if predicate(current) { record("ready=\(label)"); dump(label, current); return current }
        RunLoop.current.run(until: Date().addingTimeInterval(0.5))
    }
    finish("BLOCKED", "content_or_control_not_ready: \(label)")
}
func button(_ tree: [Node], labels: [String]) -> Node? {
    tree.first { node in node.role == kAXButtonRole && labels.contains(where: { label in
        node.text == label || node.text.split(separator: "|").map { $0.trimmingCharacters(in: .whitespaces) }.contains(label)
    }) }
}
func press(_ node: Node, _ label: String) {
    guard node.enabled != false else { finish("FAIL", "control_disabled: \(label)") }
    let error = AXUIElementPerformAction(node.element, kAXPressAction as CFString)
    record("AXPress \(label) result=\(error.rawValue)")
    guard error == .success else { finish("BLOCKED", "AXPress_failed: \(label)") }
}

let source = ProcessInfo.processInfo.environment["OPENLESS_BUILD_ID"] ?? ""
let home = FileManager.default.homeDirectoryForCurrentUser
let log = home.appendingPathComponent("Library/Logs/OpenLess Dev/openless.log")
let deadline = Date().addingTimeInterval(80)
var readyLog = ""
while process.isRunning && Date() < deadline {
    readyLog = (try? String(contentsOf: log, encoding: .utf8)) ?? ""
    if readyLog.contains("[dev-ready] version=\(expectedVersion) source=\(source) updater_enabled=false") &&
        readyLog.contains("[dev-update] check_disabled") && window() != nil { break }
    RunLoop.current.run(until: Date().addingTimeInterval(0.5))
}
guard source.count == 40, process.isRunning,
      readyLog.contains("[dev-ready] version=\(expectedVersion) source=\(source) updater_enabled=false"),
      readyLog.contains("[dev-update] check_disabled"),
      readyLog.contains("data=\(home.path)/Library/Application Support/OpenLess Dev keychain=com.openless.app.dev siwc=com.openless.app.dev.siwc")
else { finish("FAIL", "frontend_backend_ready_or_dev_isolation_missing") }
fields["runtime_sha"] = source
fields["updater"] = "IPC_check_returned_none; plugin_disabled"
fields["storage"] = "Dev_data_log_keychain_namespaces"
fields["content_initialized"] = "frontend_first_frame_and_backend_ready"
let visible = capture("initialized-dev")
guard !visible.isEmpty else { finish("BLOCKED", "visible_screenshot_or_OCR_unavailable") }
guard visible.contains("Explore first") || visible.contains("先去看看") || visible.contains("Overview") || visible.contains("概览") || visible.contains("Settings") || visible.contains("设置")
else { finish("FAIL", "initialized_window_content_not_recognized") }
if !AXIsProcessTrusted() {
    fields["navigation"] = "not_tested_AX_unavailable"
    fields["ui_version"] = "not_tested; bundle_and_runtime_version_verified"
    finish("PASS", "ready_IPC_and_visible_content_verified; AX_navigation_not_tested")
}
tree = nodes()
if let explore = button(tree, labels: ["Explore first", "先去看看"]) {
    press(explore, "Explore first")
}
tree = waitFor("main") { button($0, labels: ["Settings", "设置"]) != nil }
press(button(tree, labels: ["Settings", "设置"])!, "Settings")
tree = waitFor("settings") { button($0, labels: ["About & updates", "关于与更新"]) != nil }
press(button(tree, labels: ["About & updates", "关于与更新"])!, "About & updates")
tree = waitFor("about-version") { $0.contains { $0.text.contains(expectedVersion) } }
guard !capture("about-dev-version").isEmpty else { finish("BLOCKED", "about_screenshot_unavailable") }
fields["navigation"] = "main_settings_about_verified"
fields["ui_version"] = expectedVersion
finish("PASS", "initialized_content_navigation_version_and_dev_isolation_verified")
