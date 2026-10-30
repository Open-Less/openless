// Third-party source attribution is bundled in Resources/Licenses.
import Foundation
import ObjectiveC
#if os(iOS)
import AVKit
#endif

/// Private compatibility seam. Every selector is checked before invocation;
/// missing/changed runtime contracts disable the hidden route.
nonisolated enum PiPRuntime {
  static func method(_ type: AnyClass, _ name: String, result: String, arguments: [String] = []) -> Method? {
    guard let method = class_getInstanceMethod(type, NSSelectorFromString(name)),
      method_getNumberOfArguments(method) == arguments.count + 2 else { return nil }
    let returnType = method_copyReturnType(method)
    defer { free(returnType) }
    guard String(cString: returnType) == result else { return nil }
    for (index, expected) in arguments.enumerated() {
      guard let encoding = method_copyArgumentType(method, UInt32(index + 2)) else { return nil }
      let actual = String(cString: encoding)
      free(encoding)
      guard actual == expected else { return nil }
    }
    return method
  }

  static func object(_ owner: NSObject, _ name: String) -> NSObject? {
    typealias Getter = @convention(c) (AnyObject, Selector) -> Unmanaged<AnyObject>?
    guard let type = object_getClass(owner), let method = method(type, name, result: "@") else { return nil }
    return unsafeBitCast(method_getImplementation(method), to: Getter.self)(owner, NSSelectorFromString(name))?
      .takeUnretainedValue() as? NSObject
  }

  static func contentType(_ proxy: NSObject) -> Int? {
    typealias Getter = @convention(c) (AnyObject, Selector) -> Int
    guard let state = object(proxy, "playbackState"), let type = object_getClass(state),
      let getter = method(type, "contentType", result: "q") else { return nil }
    return unsafeBitCast(method_getImplementation(getter), to: Getter.self)(state, NSSelectorFromString("contentType"))
  }
}

/// Overrides only the adapter captured during our synchronous construction.
/// The native getter is restored for that instance immediately afterwards.
nonisolated enum PiPHiddenControls {
  private final class Construction { var adapter: NSObject? }
  nonisolated(unsafe) private static var construction: Construction?
  nonisolated(unsafe) private static var installed: Set<ObjectIdentifier> = []
  private typealias Getter = @convention(c) (AnyObject, Selector) -> Int

  #if os(iOS)
  static func makeController(source: AVPictureInPictureController.ContentSource) -> AVPictureInPictureController? {
    guard let adapterClass = NSClassFromString("AVPictureInPicturePlatformAdapter") else { return nil }
    return withPolicy(adapterClass: adapterClass, make: { AVPictureInPictureController(contentSource: source) }, adapter: {
      guard let cls = object_getClass($0), let ivar = class_getInstanceVariable(cls, "_platformAdapter"),
        let encoding = ivar_getTypeEncoding(ivar), encoding.pointee == 64 else { return nil }
      return object_getIvar($0, ivar) as? NSObject
    })
  }
  #endif

  static func withPolicy<T>(adapterClass: AnyClass, make: () -> T, adapter: (T) -> NSObject?) -> T? {
    guard Thread.isMainThread, construction == nil, install(adapterClass) else { return nil }
    let context = Construction()
    construction = context
    defer { construction = nil }
    let value = make()
    guard let captured = context.adapter, captured === adapter(value) else { return nil }
    return value
  }

  private static func install(_ type: AnyClass) -> Bool {
    let key = ObjectIdentifier(type)
    if installed.contains(key) { return true }
    let selector = NSSelectorFromString("_proxyControlsStyle")
    guard let method = PiPRuntime.method(type, "_proxyControlsStyle", result: "q"),
      let encoding = method_getTypeEncoding(method) else { return false }
    let original = unsafeBitCast(method_getImplementation(method), to: Getter.self)
    let block: @convention(block) (AnyObject) -> Int = { object in
      if Thread.isMainThread, let context = construction, let target = object as? NSObject {
        if context.adapter == nil { context.adapter = target }
        if context.adapter === target { return 5 }
      }
      return original(object, selector)
    }
    let replacement = imp_implementationWithBlock(block)
    if !class_addMethod(type, selector, replacement, encoding) { method_setImplementation(method, replacement) }
    installed.insert(key)
    return true
  }
}

/// Ordinary VideoCall type 4 during startup, one type 6 request after didStart.
/// Local submission is observable; remote coexistence is not an API guarantee.
nonisolated final class PiPCoexistenceAdapter: @unchecked Sendable {
  private final class Configuration: NSObject, @unchecked Sendable {
    weak var proxy: NSObject?
    var obtainingProxy = false
    private let lock = NSLock()
    private var requestedType = 4
    var contentType: Int {
      get { lock.lock(); defer { lock.unlock() }; return requestedType }
      set { lock.lock(); requestedType = newValue; lock.unlock() }
    }
  }
  nonisolated(unsafe) private static var associationKey: UInt8 = 0
  nonisolated(unsafe) private static var providers: [String: IMP] = [:]
  private typealias Setter = @convention(c) (AnyObject, Selector, Int) -> Void
  private typealias Mutation = @convention(block) (AnyObject) -> Void
  private typealias Updater = @convention(c) (AnyObject, Selector, Mutation) -> Void
  private typealias Responds = @convention(c) (AnyObject, Selector, Selector) -> Bool
  static let transitionDelay: TimeInterval = 1
  private weak var adapter: NSObject?
  private weak var proxy: NSObject?
  private var configuration: Configuration?
  private var generation: UUID?
  private var scheduled = false
  private(set) var submitted = false

  var diagnosticReport: String {
    "provider=\(configuration != nil) localType=\(proxy.flatMap(PiPRuntime.contentType).map(String.init) ?? "unknown") transitionScheduled=\(scheduled) transitionSubmitted=\(submitted)"
  }

  #if os(iOS)
  func prepare(controller: AVPictureInPictureController) -> Bool {
    guard (15...27).contains(ProcessInfo.processInfo.operatingSystemVersion.majorVersion),
      let adapter = PiPRuntime.object(controller, "platformAdapter") else { return false }
    return prepare(adapter: adapter) {
      guard let proxy = PiPRuntime.object(adapter, "pegasusProxy"),
        let expected = NSClassFromString("PGPictureInPictureProxy"), proxy.isKind(of: expected) else { return nil }
      return proxy
    }
  }
  #endif

  /// Install before the lazy proxy getter so its first content-type query sees 4.
  func prepare(adapter candidate: NSObject, obtainProxy: () -> NSObject?) -> Bool {
    guard Thread.isMainThread, adapter == nil,
      objc_getAssociatedObject(candidate, &Self.associationKey) == nil,
      let type = object_getClass(candidate), Self.installProvider(type) else { return false }
    let config = Configuration()
    objc_setAssociatedObject(candidate, &Self.associationKey, config, .OBJC_ASSOCIATION_RETAIN_NONATOMIC)
    config.obtainingProxy = true
    let candidateProxy = obtainProxy()
    config.obtainingProxy = false
    guard let candidateProxy, config.proxy == nil || config.proxy === candidateProxy,
      let proxyType = object_getClass(candidateProxy),
      PiPRuntime.method(proxyType, "updatePlaybackStateUsingBlock:", result: "v", arguments: ["@?"]) != nil,
      let state = PiPRuntime.object(candidateProxy, "playbackState"), let stateType = object_getClass(state),
      PiPRuntime.method(stateType, "setContentType:", result: "v", arguments: ["q"]) != nil,
      PiPRuntime.object(candidateProxy, "delegate") === candidate,
      PiPRuntime.contentType(candidateProxy) == 4 else {
      objc_setAssociatedObject(candidate, &Self.associationKey, nil, .OBJC_ASSOCIATION_RETAIN_NONATOMIC)
      return false
    }
    config.proxy = candidateProxy
    adapter = candidate
    proxy = candidateProxy
    configuration = config
    generation = UUID()
    return true
  }

  func scheduleTransition(delay: TimeInterval = transitionDelay, isStillActive: @escaping @Sendable () -> Bool) {
    guard Thread.isMainThread, let token = generation, !scheduled else { return }
    scheduled = true
    DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [weak self] in
      guard let self, self.generation == token else { return }
      // Suspension can happen during the startup second. Allow a later resume
      // event to schedule again, but never continuously force the private type.
      guard isStillActive() else { self.scheduled = false; return }
      self.submit(token: token, isStillActive: isStillActive)
    }
  }

  private func submit(token: UUID, isStillActive: @escaping @Sendable () -> Bool) {
    guard let adapter, let proxy, let config = configuration, generation == token, !submitted,
      let proxyType = object_getClass(proxy),
      let update = PiPRuntime.method(proxyType, "updatePlaybackStateUsingBlock:", result: "v", arguments: ["@?"]),
      let state = PiPRuntime.object(proxy, "playbackState"), let stateType = object_getClass(state),
      let setter = PiPRuntime.method(stateType, "setContentType:", result: "v", arguments: ["q"]) else { return }
    let set = unsafeBitCast(method_getImplementation(setter), to: Setter.self)
    let mutation: Mutation = { [weak self, weak adapter] currentState in
      guard Thread.isMainThread else { return }
      guard let self, self.generation == token else { return }
      guard isStillActive(), let adapter,
        (objc_getAssociatedObject(adapter, &Self.associationKey) as? Configuration) === config,
        object_getClass(currentState) === stateType else { self.scheduled = false; return }
      config.contentType = 6
      set(currentState, NSSelectorFromString("setContentType:"), 6)
      self.submitted = true
    }
    unsafeBitCast(method_getImplementation(update), to: Updater.self)(proxy, NSSelectorFromString("updatePlaybackStateUsingBlock:"), mutation)
  }

  private static func installProvider(_ type: AnyClass) -> Bool {
    let selector = NSSelectorFromString("pictureInPictureProxyContentType:")
    if let existing = class_getInstanceMethod(type, selector) {
      return providers.values.contains { $0 == method_getImplementation(existing) }
    }
    guard let responds = PiPRuntime.method(type, "respondsToSelector:", result: "B", arguments: [":"]),
      let encoding = method_getTypeEncoding(responds) else { return false }
    let original = unsafeBitCast(method_getImplementation(responds), to: Responds.self)
    let respondsBlock: @convention(block) (AnyObject, Selector) -> Bool = { object, queried in
      if queried == selector { return objc_getAssociatedObject(object, &associationKey) is Configuration }
      return original(object, NSSelectorFromString("respondsToSelector:"), queried)
    }
    let provider: @convention(block) (AnyObject, AnyObject?) -> Int = { object, queried in
      guard let config = objc_getAssociatedObject(object, &associationKey) as? Configuration else {
        return (queried as? NSObject).flatMap(PiPRuntime.contentType) ?? 4
      }
      if Thread.isMainThread, config.proxy == nil, config.obtainingProxy { config.proxy = queried as? NSObject }
      guard let proxy = config.proxy, queried === proxy else {
        return (queried as? NSObject).flatMap(PiPRuntime.contentType) ?? 4
      }
      return config.contentType
    }
    let providerIMP = imp_implementationWithBlock(provider)
    let respondsIMP = imp_implementationWithBlock(respondsBlock)
    guard class_addMethod(type, selector, providerIMP, "q@:@") else {
      imp_removeBlock(providerIMP); imp_removeBlock(respondsIMP); return false
    }
    class_replaceMethod(type, NSSelectorFromString("respondsToSelector:"), respondsIMP, encoding)
    providers[NSStringFromClass(type)] = providerIMP
    return true
  }

  /// Remove instance policy; class wrappers remain dormant and forward native behavior.
  func restore() {
    generation = nil
    scheduled = false
    submitted = false
    if let adapter { objc_setAssociatedObject(adapter, &Self.associationKey, nil, .OBJC_ASSOCIATION_RETAIN_NONATOMIC) }
    if let proxy, PiPRuntime.contentType(proxy) == 6,
      let state = PiPRuntime.object(proxy, "playbackState"), let type = object_getClass(state),
      let setter = PiPRuntime.method(type, "setContentType:", result: "v", arguments: ["q"]) {
      unsafeBitCast(method_getImplementation(setter), to: Setter.self)(state, NSSelectorFromString("setContentType:"), 4)
    }
    adapter = nil
    proxy = nil
    configuration = nil
  }

  deinit { restore() }
}
