// Disposable editor fixture for AX live input acceptance. Contains no user data.
import AppKit
let app = NSApplication.shared
app.setActivationPolicy(.regular)
let window = NSWindow(contentRect: NSRect(x: 300, y: 400, width: 650, height: 180),
                      styleMask: [.titled, .closable], backing: .buffered, defer: false)
window.title = "OpenLess live input fixture"
let field = NSTextField(frame: NSRect(x: 24, y: 70, width: 600, height: 36))
field.stringValue = "before 原文 after"
field.font = NSFont.systemFont(ofSize: 20)
window.contentView!.addSubview(field)
window.makeKeyAndOrderFront(nil)
app.activate(ignoringOtherApps: true)
window.makeFirstResponder(field)
(field.currentEditor() as? NSTextView)?.setSelectedRange(NSRange(location: 7, length: 2))
Timer.scheduledTimer(withTimeInterval: 0.3, repeats: false) { _ in
    app.activate(ignoringOtherApps: true)
    window.makeKeyAndOrderFront(nil)
    window.makeFirstResponder(field)
    (field.currentEditor() as? NSTextView)?.setSelectedRange(NSRange(location: 7, length: 2))
}
Timer.scheduledTimer(withTimeInterval: 30, repeats: false) { _ in app.terminate(nil) }
app.run()
