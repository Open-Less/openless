// Disposable editor fixture for AX live input acceptance. Contains no user data.
import AppKit

// --ask-ab <directory> exposes a file protocol exclusively for this disposable
// fixture. It never discovers, reads, or writes another application's windows.
final class AskABFixture {
    let directory: URL
    let windows: [NSWindow]
    let editors: [NSTextView]
    var commandID = 0
    var timer: Timer?
    let originals = ["A🙂 before after", "B🙂 red red end"]

    init(directory: URL) {
        self.directory = directory
        var windows: [NSWindow] = []
        var editors: [NSTextView] = []
        for index in 0..<2 {
            let window = NSWindow(
                contentRect: NSRect(x: 220 + index * 440, y: 400, width: 420, height: 160),
                styleMask: [.titled, .closable], backing: .buffered, defer: false)
            window.title = "OpenLess Ask fixture \(index == 0 ? "A" : "B")"
            window.isReleasedWhenClosed = false
            let editor = NSTextView(frame: NSRect(x: 20, y: 35, width: 380, height: 90))
            editor.font = NSFont.systemFont(ofSize: 18)
            editor.isAutomaticQuoteSubstitutionEnabled = false
            editor.isAutomaticDashSubstitutionEnabled = false
            editor.isAutomaticTextReplacementEnabled = false
            window.contentView!.addSubview(editor)
            window.orderFront(nil)
            windows.append(window)
            editors.append(editor)
        }
        self.windows = windows
        self.editors = editors
        reset()
        timer = Timer.scheduledTimer(withTimeInterval: 0.025, repeats: true) { [weak self] _ in
            self?.poll()
        }
        Timer.scheduledTimer(withTimeInterval: 120, repeats: false) { _ in
            NSApplication.shared.terminate(nil)
        }
        publish()
    }

    func focus(_ index: Int, range: NSRange) {
        NSApplication.shared.activate(ignoringOtherApps: true)
        windows[index].makeKeyAndOrderFront(nil)
        windows[index].makeFirstResponder(editors[index])
        editors[index].setSelectedRange(range)
    }

    func reset() {
        for index in 0..<2 {
            editors[index].string = originals[index]
        }
        editors[1].setSelectedRange(NSRange(location: 8, length: 3))
        focus(0, range: NSRange(location: 10, length: 0))
    }

    func poll() {
        let command = directory.appendingPathComponent("command.json")
        if let data = try? Data(contentsOf: command),
           let value = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
           let id = value["id"] as? Int, id > commandID,
           let action = value["action"] as? String {
            commandID = id
            switch action {
            case "reset": reset()
            case "focusB": focus(1, range: NSRange(location: 8, length: 3))
            case "moveA": focus(0, range: NSRange(location: 0, length: 0))
            case "changeA": editors[0].string = "A changed after capture"
            case "moveB": focus(1, range: NSRange(location: 4, length: 3))
            case "quit": NSApplication.shared.terminate(nil)
            default: break
            }
        }
        publish()
    }

    func publish() {
        let values = editors.map { editor -> [String: Any] in
            let range = editor.selectedRange()
            return ["text": editor.string, "start": range.location, "length": range.length]
        }
        let state: [String: Any] = [
            "fixture": "openless-ask-native-ab-v1",
            "pid": ProcessInfo.processInfo.processIdentifier,
            "commandId": commandID,
            "a": values[0], "b": values[1],
        ]
        if let data = try? JSONSerialization.data(withJSONObject: state) {
            try? data.write(to: directory.appendingPathComponent("state.json"), options: .atomic)
        }
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.regular)
if let flag = CommandLine.arguments.firstIndex(of: "--ask-ab"),
   CommandLine.arguments.indices.contains(flag + 1) {
    let fixture = AskABFixture(directory: URL(fileURLWithPath: CommandLine.arguments[flag + 1]))
    withExtendedLifetime(fixture) { app.run() }
    exit(0)
}

final class ClickRecorder: NSView {
    private var count = 0
    override func mouseDown(with event: NSEvent) {
        count += 1
        try? "\(count)".write(toFile: "/tmp/openless-fixture-click-count.txt", atomically: true, encoding: .utf8)
    }
}
let window = NSWindow(contentRect: NSRect(x: 300, y: 400, width: 650, height: 180),
                      styleMask: [.titled, .closable], backing: .buffered, defer: false)
window.title = "OpenLess live input fixture"
window.contentView = ClickRecorder(frame: window.contentView!.bounds)
let field = NSTextView(frame: NSRect(x: 24, y: 70, width: 600, height: 36))
field.string = "before 原文 after"
field.font = NSFont.systemFont(ofSize: 20)
window.contentView!.addSubview(field)
window.makeKeyAndOrderFront(nil)
app.activate(ignoringOtherApps: true)
window.makeFirstResponder(field)
field.setSelectedRange(NSRange(location: 7, length: 2))
Timer.scheduledTimer(withTimeInterval: 0.3, repeats: false) { _ in
    app.activate(ignoringOtherApps: true)
    window.makeKeyAndOrderFront(nil)
    window.makeFirstResponder(field)
    field.setSelectedRange(NSRange(location: 7, length: 2))
}
Timer.scheduledTimer(withTimeInterval: 30, repeats: false) { _ in app.terminate(nil) }
if CommandLine.arguments.contains("--ask-selected") {
    Timer.scheduledTimer(withTimeInterval: 0.10, repeats: true) { timer in
        if field.string != "before 原文 after" {
            timer.invalidate()
            return
        }
        field.setSelectedRange(NSRange(location: 7, length: 2))
    }
}
app.run()
