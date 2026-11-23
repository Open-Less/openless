// Controlled macOS WKWebView rendering of the production capsule fixture.
// No installed app, microphone, keyboard injection, or user preferences are used.
import AppKit
import WebKit

let app = NSApplication.shared
app.setActivationPolicy(.accessory)
let output = URL(fileURLWithPath: CommandLine.arguments[2], isDirectory: true)
let web = WKWebView(frame: NSRect(x: 0, y: 0, width: 460, height: 492))
let window = NSWindow(contentRect: web.frame, styleMask: [.borderless], backing: .buffered, defer: false)
window.contentView = web
window.orderBack(nil)
@MainActor func evaluate(_ js: String) async throws -> Any? {
    try await web.evaluateJavaScript(js)
}
@MainActor func capture(_ name: String) async throws {
    let config = WKSnapshotConfiguration()
    if !name.hasSuffix("menu") { config.rect = CGRect(x: 0, y: 320, width: 460, height: 172) }
    let image = try await web.takeSnapshot(configuration: config)
    let rep = NSBitmapImageRep(data: image.tiffRepresentation!)!
    try rep.representation(using: .png, properties: [:])!.write(to: output.appendingPathComponent(name + ".png"))
}
func settle() async throws { try await Task.sleep(nanoseconds: 400_000_000) }
Task { @MainActor in
    do {
        for _ in 0..<150 {
            if (try? await evaluate("Boolean(window.fixture?.hasListener('dictation:jobs'))")) as? Bool == true { break }
            try await Task.sleep(nanoseconds: 100_000_000)
        }
        _ = try await evaluate("""
        window.fixture.emit('capsule:state', {state:'recording',sessionId:'recording-a',level:.4,translation:true});
        window.fixture.emit('backend:event', {sequence:1,sessionId:'recording-a',kind:{type:'dictation_state_changed',payload:{sessionId:'recording-a',phase:'recording'}}});
        window.geometry = () => Object.fromEntries(['.ol-default-capsule','.ol-capsule-transcript-rail','.ol-classic-translation','.ol-processing-satellite'].map(s=>{let r=document.querySelector(s)?.getBoundingClientRect();return [s,r?{x:r.x,y:r.y,width:r.width,height:r.height}:null]})); void 0;
        """)
        try await settle()
        var evidence: [String: Any] = [:]
        try await capture("translation-before")
        _ = try await evaluate("""
        window.translationNode=document.querySelector('.ol-classic-translation');
        window.motionTrace=[]; window.motionStart=performance.now();
        window.sampleMotion=()=>{let e=document.querySelector('.ol-classic-translation');window.motionTrace.push({time:performance.now()-window.motionStart,y:e.getBoundingClientRect().y,same:e===window.translationNode});if(performance.now()-window.motionStart<400)requestAnimationFrame(window.sampleMotion)};
        window.sampleMotion();
        window.fixture.emit('backend:event', {sequence:2,sessionId:'recording-a',kind:{type:'transcript_delta',payload:{text:'当前录音保持前景，上一段正在转录。',offset:0,isFinal:false}}});
        void 0;
        """)
        try await Task.sleep(nanoseconds: 70_000_000)
        try await capture("translation-mid")
        try await settle()
        try await capture("translation-after")
        evidence["translationMotion"] = try await evaluate("window.motionTrace")
        evidence["before"] = try await evaluate("window.geometry()")
        try await capture("native-webkit-before")
        _ = try await evaluate("window.fixture.emit('dictation:jobs',[{sessionId:'old-a'}])")
        try await settle()
        evidence["overlap"] = try await evaluate("window.geometry()")
        try await capture("native-webkit-overlap")
        _ = try await evaluate("document.querySelector('[role=combobox]').click()")
        try await settle()
        evidence["menu"] = try await evaluate("window.geometry()")
        try await capture("native-webkit-menu")
        _ = try await evaluate("document.querySelector('[role=combobox]').click();window.fixture.emit('dictation:jobs',[])")
        try await settle()
        evidence["after"] = try await evaluate("window.geometry()")
        try await capture("native-webkit-after")
        _ = try await evaluate("window.fixture.emit('capsule:state',{state:'transcribing',sessionId:'recording-a',translation:true})")
        try await settle()
        try await capture("translation-after-stop")
        let pendingLabel = try await evaluate("document.querySelector('.ol-classic-translation').textContent.trim()") as? String
        precondition(pendingLabel == "正在翻译")
        try JSONSerialization.data(withJSONObject: evidence, options: [.prettyPrinted,.sortedKeys]).write(to: output.appendingPathComponent("native-webkit-geometry.json"))
        for name in ["overlap", "menu", "after"] {
            for selector in [".ol-default-capsule", ".ol-capsule-transcript-rail", ".ol-classic-translation"] {
                let before = (evidence["before"] as! NSDictionary)[selector] as! NSDictionary
                let current = (evidence[name] as! NSDictionary)[selector] as! NSDictionary
                precondition(before.isEqual(current), "Moved \(selector) in \(name)")
            }
        }
        print("PASS: native WKWebView captured before, overlap, menu, after; foreground, transcript and translation coordinates unchanged")
        app.terminate(nil)
    } catch { print("FAIL: \(error)"); exit(1) }
}
web.load(URLRequest(url: URL(string: CommandLine.arguments[1])!))
app.run()
