import Foundation
import ObjectiveC

nonisolated final class Activity: @unchecked Sendable { var active = true }

@main
struct PiPCoexistenceRuntimeTests {
  static func main() {
    testLockRecovery()
    let adapter = PCEMakeAdapter()!
    let sibling = PCEMakeAdapter()!
    let proxy = PCEMakeProxy(4)!
    let siblingProxy = PCEMakeProxy(1)!
    PCESetDelegate(proxy, adapter)
    let nativeClass: AnyClass = object_getClass(adapter)!
    let provider = NSSelectorFromString("pictureInPictureProxyContentType:")
    let made = PiPHiddenControls.withPolicy(adapterClass: nativeClass, make: { () -> NSObject in
      precondition(PCEControlsStyle(adapter) == 5)
      precondition(PCEControlsStyle(sibling) == 2)
      return adapter
    }, adapter: { $0 })
    precondition(made === adapter && PCEControlsStyle(adapter) == 2)
    precondition(PiPHiddenControls.withPolicy(adapterClass: nativeClass, make: { () -> NSObject in
      precondition(PCEControlsStyle(adapter) == 5)
      return sibling
    }, adapter: { $0 }) == nil)
    precondition(PCEControlsStyle(adapter) == 2)
    let session = PiPCoexistenceAdapter()
    func prepare() {
      precondition(session.prepare(adapter: adapter) {
        precondition(adapter.responds(to: provider))
        precondition(PCEProviderType(adapter, proxy) == 4)
        return proxy
      })
    }
    func pump(_ seconds: TimeInterval = 0.03) { RunLoop.main.run(until: Date().addingTimeInterval(seconds)) }
    prepare()
    precondition(object_getClass(adapter) === nativeClass && object_getClass(sibling) === nativeClass)
    precondition(!sibling.responds(to: provider))
    precondition(PCEProviderType(adapter, siblingProxy) == 1)
    precondition(adapter.responds(to: NSSelectorFromString("description")))
    precondition(!adapter.responds(to: NSSelectorFromString("unsupportedTestSelector")))
    precondition(!session.prepare(adapter: adapter) { proxy })
    session.scheduleTransition { true }
    session.scheduleTransition(delay: 0) { true }
    pump(0.15)
    precondition(PCEType(proxy) == 4 && PCESendCount(proxy) == 0)
    pump(1.0)
    precondition(PCEType(proxy) == 6 && PCESendCount(proxy) == 1 && session.submitted)
    PCERunAdapterUpdate(adapter)
    precondition(PCEType(proxy) == 4, "Never force type 6 continuously")
    PCERunProxyRecalculate(proxy)
    precondition(PCEType(proxy) == 6)
    session.restore()
    precondition(PCEType(proxy) == 4 && !adapter.responds(to: provider))
    precondition(PCEProviderType(adapter, proxy) == 4 && PCEProviderType(sibling, siblingProxy) == 1)
    precondition(PCESendCount(proxy) == 1, "Restore must not send remote changes")
    print("PASS: constructor scope, native behavior, provider isolation, one second startup, one-shot transition, restore")

    precondition(!session.prepare(adapter: PCEMakeExistingProviderAdapter()!) { proxy })
    precondition(!session.prepare(adapter: adapter) { PCEMakeUnsupportedProxy() })
    precondition(!adapter.responds(to: provider))
    precondition(!session.prepare(adapter: adapter) { PCEMakeProxy(4) }, "Unrelated proxy delegate must be rejected")
    precondition(!session.prepare(adapter: adapter) { PCEMakeProxy(7) })
    precondition(!adapter.responds(to: provider))
    precondition(!session.prepare(adapter: adapter) { nil })
    precondition(!adapter.responds(to: provider))
    precondition(PiPRuntime.method(nativeClass, "_proxyControlsStyle", result: "v") == nil)
    precondition(PiPRuntime.method(nativeClass, "_proxyControlsStyle", result: "q", arguments: ["q"]) == nil)
    print("PASS: missing selectors, changed signature/type, existing provider and failed lookup safely decline")

    prepare()
    let activity = Activity()
    session.scheduleTransition(delay: 0) { activity.active }
    activity.active = false
    pump()
    precondition(PCEType(proxy) == 4 && !session.submitted)
    activity.active = true
    session.scheduleTransition(delay: 0) { activity.active }
    pump()
    precondition(PCEType(proxy) == 6 && session.submitted)
    session.restore()
    prepare()
    session.scheduleTransition(delay: 0) { true }
    session.restore()
    prepare()
    let before = PCESendCount(proxy)
    pump()
    precondition(PCEType(proxy) == 4 && PCESendCount(proxy) == before)
    session.restore()
    print("PASS: suspension retry and replaced-session cancellation")

    prepare()
    PCESetDeferred(proxy, true)
    session.scheduleTransition(delay: 0) { activity.active }
    pump()
    activity.active = false
    PCECompleteDeferred(proxy)
    precondition(PCEType(proxy) == 4 && !session.submitted)
    activity.active = true
    session.scheduleTransition(delay: 0) { activity.active }
    pump()
    PCECompleteDeferred(proxy)
    precondition(PCEType(proxy) == 6 && session.submitted, "A canceled deferred transition can recover on resume")
    session.restore()
    prepare()
    activity.active = true
    session.scheduleTransition(delay: 0) { activity.active }
    pump()
    session.restore()
    PCECompleteDeferred(proxy)
    precondition(PCEType(proxy) == 4)
    PCESetDeferred(proxy, false)
    print("PASS: deferred mutation rechecks activity and session identity")

    for _ in 0..<100 {
      autoreleasepool {
        let a = PCEMakeAdapter()!
        let p = PCEMakeProxy(4)!
        PCESetDelegate(p, a)
        var temporary: PiPCoexistenceAdapter? = PiPCoexistenceAdapter()
        precondition(temporary!.prepare(adapter: a) { p })
        temporary = nil
        precondition(!a.responds(to: provider) && PCEType(p) == 4)
      }
    }
    weak var weakAdapter: NSObject?
    weak var weakProxy: NSObject?
    autoreleasepool {
      let a = PCEMakeAdapter()!
      let p = PCEMakeProxy(4)!
        PCESetDelegate(p, a)
      weakAdapter = a; weakProxy = p
      precondition(session.prepare(adapter: a) { p })
    }
    precondition(weakAdapter == nil && weakProxy == nil)
    session.restore()
    print("PASS: repeated cleanup and weak ownership; remote iOS behavior remains device-only")
  }

  static func testLockRecovery() {
    var recovery = PiPLockRecovery()
    let original = UUID()
    let replacement = UUID()
    recovery.didStop(generation: original)
    precondition(!recovery.consumeOnUnlock(generation: original, active: false),
      "A video PiP displacement without a lock cannot request background restart")
    recovery.didLock(generation: original, active: true, recovering: false)
    recovery.didStop(generation: original)
    precondition(recovery.consumeOnUnlock(generation: replacement, active: false),
      "A stopped locked session survives controller disposal")
    precondition(!recovery.consumeOnUnlock(generation: replacement, active: false),
      "The unlock recovery grant is consumed exactly once")
    recovery.didLock(generation: original, active: true, recovering: false)
    precondition(recovery.consumeOnUnlock(generation: original, active: false),
      "An active-state flip before didStop still recovers the locked controller")
    recovery.didStop(generation: original)
    precondition(!recovery.consumeOnUnlock(generation: replacement, active: false),
      "A delayed old callback cannot create another recovery grant")
    recovery.didLock(generation: original, active: true, recovering: false)
    precondition(!recovery.consumeOnUnlock(generation: original, active: true),
      "A surviving active PiP is not restarted")
    recovery.didLock(generation: original, active: false, recovering: true)
    recovery.didLock(generation: replacement, active: false, recovering: false)
    precondition(recovery.consumeOnUnlock(generation: replacement, active: false),
      "A lock-interrupted startup recovers once, including duplicate lock notifications")
    recovery.didLock(generation: original, active: true, recovering: false)
    recovery.didStop(generation: replacement)
    precondition(!recovery.consumeOnUnlock(generation: replacement, active: false),
      "An unrelated replacement cannot inherit the original active controller's grant")
    recovery.didLock(generation: original, active: true, recovering: false)
    recovery.didStop(generation: original)
    recovery.reset()
    precondition(!recovery.consumeOnUnlock(generation: replacement, active: false),
      "Explicit disable clears pending lock recovery")
    print("PASS: one-shot lock recovery, both callback orders, interrupted startup, no video-PiP restart, explicit disable")
  }

}
