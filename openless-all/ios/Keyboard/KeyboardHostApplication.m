#import "KeyboardHostApplication.h"
#import <objc/message.h>
#import <objc/runtime.h>
#import <os/lock.h>

NSString *OLKeyboardHostBundleIdentifier(UIInputViewController *controller) {
  if (!NSThread.isMainThread || !controller.isViewLoaded || !controller.view.window) return nil;
  @try {
    // Probe the attached extension parent and validate the optional getter's
    // ABI. A missing getter falls through to the request-scoped host query.
    SEL selector = NSSelectorFromString(@"_hostBundleID");
    UIViewController *candidate = controller;
    for (NSUInteger depth = 0; candidate && depth < 4; depth++, candidate = candidate.parentViewController) {
      Method method = class_getInstanceMethod(object_getClass(candidate), selector);
      if (!method) continue;
      NSMethodSignature *signature = [NSMethodSignature signatureWithObjCTypes:method_getTypeEncoding(method)];
      if (signature.numberOfArguments != 2
          || strcmp(signature.methodReturnType, @encode(id)) != 0
          || strcmp([signature getArgumentTypeAtIndex:0], @encode(id)) != 0
          || strcmp([signature getArgumentTypeAtIndex:1], @encode(SEL)) != 0) continue;
      id value = ((id (*)(id, SEL))objc_msgSend)(candidate, selector);
      if (![value isKindOfClass:NSString.class]) continue;
      NSString *identifier = value;
      if (identifier.length == 0 || identifier.length > 255 || ![identifier containsString:@"."]
          || [identifier isEqualToString:NSBundle.mainBundle.bundleIdentifier]) continue;
      NSCharacterSet *allowed = [NSCharacterSet characterSetWithCharactersInString:
          @"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789.-"];
      if ([identifier rangeOfCharacterFromSet:allowed.invertedSet].location == NSNotFound) return [identifier copy];
    }
  } @catch (__unused NSException *exception) {
    return nil;
  }
  return nil;
}

// Runtime hooks require matching signatures. Query generations prevent a
// delayed callback from resolving a replacement keyboard's destination.
static os_unfair_lock OLHostLock = OS_UNFAIR_LOCK_INIT;
static uint64_t OLHostGeneration;
static uint64_t OLHostActiveQueryID;
static uint64_t OLHostNextQueryID;
static IMP OLHostOriginalEnabled;
static IMP OLHostOriginalChanged;
static IMP OLHostOriginalHandled;

@interface OLKeyboardHostQuery : NSObject
@property(nonatomic, weak) UIInputViewController *controller;
@property(nonatomic, weak) UIWindow *window;
@property(nonatomic, copy) NSUUID *documentIdentifier;
@property(nonatomic, copy) void (^completion)(NSString * _Nullable);
@property(nonatomic) uint64_t identifier;
@property(nonatomic) uint64_t baseline;
@property(nonatomic) NSUInteger phase;
@property(nonatomic, copy, nullable) NSString *firstBundle;
@property(nonatomic, copy) NSString *finishReason;
@end
@implementation OLKeyboardHostQuery
@end
static OLKeyboardHostQuery *OLHostQuery;

static BOOL OLHostValidBundle(id value) {
  if (![value isKindOfClass:NSString.class]) return NO;
  NSString *identifier = value;
  if (!identifier.length || identifier.length > 255 || ![identifier containsString:@"."]
      || [identifier isEqualToString:NSBundle.mainBundle.bundleIdentifier]) return NO;
  NSCharacterSet *allowed = [NSCharacterSet characterSetWithCharactersInString:
      @"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789.-"];
  return [identifier rangeOfCharacterFromSet:allowed.invertedSet].location == NSNotFound;
}

static BOOL OLHostObjectArgument(const char *type) {
  return strcmp(type, @encode(id)) == 0 || strcmp(type, "@?") == 0;
}

static BOOL OLHostMethodMatches(Method method, const char *result, NSUInteger objects) {
  if (!method) return NO;
  @try {
    NSMethodSignature *signature = [NSMethodSignature signatureWithObjCTypes:method_getTypeEncoding(method)];
    if (signature.numberOfArguments != 2 + objects
        || strcmp(signature.methodReturnType, result) != 0
        || strcmp([signature getArgumentTypeAtIndex:0], @encode(id)) != 0
        || strcmp([signature getArgumentTypeAtIndex:1], @encode(SEL)) != 0) return NO;
    for (NSUInteger index = 2; index < signature.numberOfArguments; index++) {
      if (!OLHostObjectArgument([signature getArgumentTypeAtIndex:index])) return NO;
    }
    return YES;
  } @catch (__unused NSException *exception) {
    return NO;
  }
}

static BOOL OLHostHandledMethodMatches(Method method) {
  if (!method) return NO;
  @try {
    NSMethodSignature *signature = [NSMethodSignature signatureWithObjCTypes:method_getTypeEncoding(method)];
    return signature.numberOfArguments == 5
        && strcmp(signature.methodReturnType, @encode(BOOL)) == 0
        && strcmp([signature getArgumentTypeAtIndex:0], @encode(id)) == 0
        && strcmp([signature getArgumentTypeAtIndex:1], @encode(SEL)) == 0
        && strcmp([signature getArgumentTypeAtIndex:2], @encode(id)) == 0
        && strcmp([signature getArgumentTypeAtIndex:3], @encode(BOOL)) == 0
        && strcmp([signature getArgumentTypeAtIndex:4], @encode(BOOL)) == 0;
  } @catch (__unused NSException *exception) {
    return NO;
  }
}

static void OLHostLogConnectionMethods(Class client) {
  // Metadata only: do not invoke connection/reset methods to force a callback.
  for (NSUInteger kind = 0; kind < 2; kind++) {
    Class inspected = kind == 0 ? client : object_getClass(client);
    unsigned int count = 0;
    Method *methods = class_copyMethodList(inspected, &count);
    for (unsigned int index = 0; index < count; index++) {
      const char *name = sel_getName(method_getName(methods[index]));
      if (strstr(name, "connection") || strstr(name, "Connection")) {
        NSLog(@"OPENLESS_HOST_QUERY connection-method %s%s ABI=%s", kind == 0 ? "-" : "+",
              name, method_getTypeEncoding(methods[index]));
      }
    }
    free(methods);
  }
}

static BOOL OLHostQueryIsAttached(OLKeyboardHostQuery *query) {
  @try {
    UIInputViewController *controller = query.controller;
    UIWindow *window = query.window;
    // Remote keyboard windows do not necessarily mirror their host scene's
    // activation/visibility/geometry. The caller owns viewDidAppear/disappear
    // cancellation; bind this query to the same attached view and document.
    if (!controller || !window || !controller.isViewLoaded || controller.view.window != window) return NO;
    NSUUID *identifier = controller.textDocumentProxy.documentIdentifier;
    return [identifier isKindOfClass:NSUUID.class] && [identifier isEqual:query.documentIdentifier];
  } @catch (__unused NSException *exception) {
    return NO;
  }
}

static void OLHostFinish(OLKeyboardHostQuery *query, NSString *bundle) {
  if (OLHostQuery != query) return;
  if (bundle && !OLHostQueryIsAttached(query)) {
    bundle = nil; query.finishReason = @"detached-before-completion";
  }
  NSLog(@"OPENLESS_HOST_QUERY finish query=%llu phase=%lu reason=%@ bundle=%@ attached=%d",
        (unsigned long long)query.identifier, (unsigned long)query.phase,
        query.finishReason ?: @"unspecified", bundle ?: @"nil", OLHostQueryIsAttached(query));
  OLHostQuery = nil;
  os_unfair_lock_lock(&OLHostLock);
  OLHostActiveQueryID = 0;
  os_unfair_lock_unlock(&OLHostLock);
  void (^completion)(NSString *) = query.completion;
  query.completion = nil;
  if (completion) completion(bundle);
}

static BOOL OLHostEnabled(id object, SEL selector) {
  os_unfair_lock_lock(&OLHostLock);
  BOOL probing = OLHostActiveQueryID != 0;
  os_unfair_lock_unlock(&OLHostLock);
  if (probing) return YES;
  return OLHostOriginalEnabled ? ((BOOL (*)(id, SEL))OLHostOriginalEnabled)(object, selector) : NO;
}

static BOOL OLHostPing(void) {
  @try {
    Class clientClass = NSClassFromString(@"_UIKeyboardArbiterClient");
    SEL shared = NSSelectorFromString(@"automaticSharedArbiterClient");
    SEL check = NSSelectorFromString(@"checkConnection");
    if (!OLHostMethodMatches(class_getClassMethod(clientClass, shared), @encode(id), 0)) return NO;
    id client = ((id (*)(id, SEL))objc_msgSend)(clientClass, shared);
    if (!client || !OLHostMethodMatches(class_getInstanceMethod(object_getClass(client), check), @encode(void), 0)) return NO;
    // Observed iOS 27 runtime: -stopConnection v16@0:8. Reconnect for
    // each observation so checkConnection cannot reuse an old destination.
    SEL stop = NSSelectorFromString(@"stopConnection");
    if (!OLHostMethodMatches(class_getInstanceMethod(object_getClass(client), stop), @encode(void), 0)) return NO;
    ((void (*)(id, SEL))objc_msgSend)(client, stop);
    ((void (*)(id, SEL))objc_msgSend)(client, check);
    return YES;
  } @catch (__unused NSException *exception) {
    return NO;
  }
}

static void OLHostObserve(uint64_t queryID, uint64_t generation, NSString *bundle) {
  OLKeyboardHostQuery *query = OLHostQuery;
  if (!query || query.identifier != queryID || generation <= query.baseline) return;
  if (!OLHostQueryIsAttached(query)) {
    query.finishReason = @"detached-during-observation";
    OLHostFinish(query, nil);
    return;
  }
  // stopConnection/checkConnection can first report a destination being
  // cleared. That is not a completed source lookup; wait for the new host
  // event within the existing deadline. Still require two matching hosts.
  if (!bundle) return;
  if (query.phase == 0) {
    query.firstBundle = bundle;
    query.phase = 1;
    // Exclude every event already captured, including callbacks waiting on the
    // main queue. Require another event after a distinct second ping.
    dispatch_async(dispatch_get_main_queue(), ^{
      if (OLHostQuery != query) return;
      if (!OLHostQueryIsAttached(query)) { query.finishReason = @"detached-before-second-ping"; OLHostFinish(query, nil); return; }
      os_unfair_lock_lock(&OLHostLock);
      query.baseline = OLHostGeneration;
      os_unfair_lock_unlock(&OLHostLock);
      query.phase = 2;
      if (!OLHostPing()) { query.finishReason = @"second-ping-unavailable"; OLHostFinish(query, nil); }
    });
  } else if (![query.firstBundle isEqualToString:bundle]) {
    // A destination change during the gap between pings also invalidates the
    // request; never wait for it to change back to the first observation.
    query.finishReason = @"observed-different-host"; OLHostFinish(query, nil);
  } else if (query.phase == 2) {
    query.finishReason = @"two-fresh-matching-observations";
    OLHostFinish(query, [query.firstBundle isEqualToString:bundle] ? bundle : nil);
  }
}

static void OLHostCaptureObservation(id change, SEL selector) {
  os_unfair_lock_lock(&OLHostLock);
  uint64_t queryID = OLHostActiveQueryID;
  uint64_t generation = ++OLHostGeneration;
  os_unfair_lock_unlock(&OLHostLock);
  if (queryID) {
    NSString *bundle = nil;
    @try {
      id value = [change valueForKey:@"_sourceBundleIdentifier"];
      if (OLHostValidBundle(value)) bundle = [value copy];
    } @catch (__unused NSException *exception) {}
    NSLog(@"OPENLESS_HOST_QUERY event selector=%@ class=%@ bundle=%@ generation=%llu query=%llu",
          NSStringFromSelector(selector), NSStringFromClass(object_getClass(change)), bundle ?: @"nil",
          (unsigned long long)generation, (unsigned long long)queryID);
    dispatch_async(dispatch_get_main_queue(), ^{ OLHostObserve(queryID, generation, bundle); });
  }
}

static void OLHostChanged(id object, SEL selector, id change, id completion) {
  // Always preserve UIKit's original callback, including its completion block.
  if (OLHostOriginalChanged) {
    ((void (*)(id, SEL, id, id))OLHostOriginalChanged)(object, selector, change, completion);
  }
  OLHostCaptureObservation(change, selector);
}

static BOOL OLHostHandled(id object, SEL selector, id change, BOOL snapshot, BOOL local) {
  // iOS 27's observed direct dispatch bypasses queue_keyboardChanged:. Its
  // scalar parameters must match the runtime ABI before this hook is installed.
  BOOL result = NO;
  if (OLHostOriginalHandled) {
    result = ((BOOL (*)(id, SEL, id, BOOL, BOOL))OLHostOriginalHandled)(object, selector, change, snapshot, local);
  }
  OLHostCaptureObservation(change, selector);
  return result;
}

static BOOL OLHostInstallHooks(void) {
  static BOOL attempted;
  static BOOL installed;
  if (attempted) return installed;
  attempted = YES;
  @try {
    Class client = NSClassFromString(@"_UIKeyboardArbiterClient");
    Class destination = NSClassFromString(@"_UIKeyboardArbiterClientInputDestination");
    Method enabled = class_getClassMethod(client, NSSelectorFromString(@"enabled"));
    Method changed = class_getInstanceMethod(destination, NSSelectorFromString(@"queue_keyboardChanged:onComplete:"));
    Method handled = class_getInstanceMethod(destination, NSSelectorFromString(
        @"_didHandleKeyboardClientChange:shouldConsiderSnapshottingKeyboard:isLocalEvent:"));
    Method shared = class_getClassMethod(client, NSSelectorFromString(@"automaticSharedArbiterClient"));
    Method check = class_getInstanceMethod(client, NSSelectorFromString(@"checkConnection"));
    NSLog(@"OPENLESS_HOST_QUERY ABI enabled=%s changed=%s handled=%s shared=%s check=%s",
          enabled ? method_getTypeEncoding(enabled) : "missing",
          changed ? method_getTypeEncoding(changed) : "missing",
          handled ? method_getTypeEncoding(handled) : "missing",
          shared ? method_getTypeEncoding(shared) : "missing",
          check ? method_getTypeEncoding(check) : "missing");
    OLHostLogConnectionMethods(client);
    BOOL canHookChanged = OLHostMethodMatches(changed, @encode(void), 2);
    BOOL canHookHandled = OLHostHandledMethodMatches(handled);
    if (!OLHostMethodMatches(enabled, @encode(BOOL), 0)
        || (!canHookChanged && !canHookHandled)
        || !OLHostMethodMatches(shared, @encode(id), 0)
        || !OLHostMethodMatches(check, @encode(void), 0)) return NO;
    OLHostOriginalEnabled = method_getImplementation(enabled);
    // Observe one dispatch layer. Hooking both could count the same change
    // twice when queue_keyboardChanged: eventually calls the handled method.
    OLHostOriginalChanged = canHookChanged && !canHookHandled ? method_getImplementation(changed) : NULL;
    OLHostOriginalHandled = canHookHandled ? method_getImplementation(handled) : NULL;
    if (!OLHostOriginalEnabled || (!OLHostOriginalChanged && !OLHostOriginalHandled)) return NO;
    if (OLHostOriginalChanged) method_setImplementation(changed, (IMP)OLHostChanged);
    if (OLHostOriginalHandled) method_setImplementation(handled, (IMP)OLHostHandled);
    method_setImplementation(enabled, (IMP)OLHostEnabled);
    installed = YES;
  } @catch (__unused NSException *exception) {
    return NO;
  }
  return installed;
}

static void OLHostBeginQuery(UIInputViewController *controller,
                             void (^completion)(NSString * _Nullable)) {
  // A replacement owns the slot before the previous completion runs, so a
  // completion that itself starts a query cannot be overwritten afterward.
  OLKeyboardHostQuery *previous = OLHostQuery;
  OLKeyboardHostQuery *query = [OLKeyboardHostQuery new];
  query.controller = controller;
  query.completion = completion;
  query.identifier = ++OLHostNextQueryID;
  query.finishReason = @"initial-validation";
  OLHostQuery = query;
  os_unfair_lock_lock(&OLHostLock);
  OLHostActiveQueryID = 0;
  os_unfair_lock_unlock(&OLHostLock);
  if (previous.completion) {
    void (^cancelled)(NSString *) = previous.completion;
    previous.completion = nil;
    cancelled(nil);
  }
  if (OLHostQuery != query) return;
  @try {
    NSDictionary *extension = NSBundle.mainBundle.infoDictionary[@"NSExtension"];
    if (![extension[@"NSExtensionPointIdentifier"] isEqual:@"com.apple.keyboard-service"]
        || !controller.isViewLoaded || !controller.view.window) { OLHostFinish(query, nil); return; }
    query.window = controller.view.window;
    id identifier = controller.textDocumentProxy.documentIdentifier;
    if (![identifier isKindOfClass:NSUUID.class]) { OLHostFinish(query, nil); return; }
    query.documentIdentifier = identifier;
    if (!OLHostQueryIsAttached(query)) { OLHostFinish(query, nil); return; }
    NSString *legacy = OLKeyboardHostBundleIdentifier(controller);
    if (legacy) { query.finishReason = @"legacy-current-host"; OLHostFinish(query, legacy); return; }
    if (@available(iOS 26.4, *)) {
      if (!OLHostInstallHooks()) { query.finishReason = @"unsupported-runtime-ABI"; OLHostFinish(query, nil); return; }
      os_unfair_lock_lock(&OLHostLock);
      query.baseline = OLHostGeneration;
      OLHostActiveQueryID = query.identifier;
      os_unfair_lock_unlock(&OLHostLock);
      dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 2 * NSEC_PER_SEC), dispatch_get_main_queue(), ^{
        if (OLHostQuery == query) { query.finishReason = @"observation-timeout"; OLHostFinish(query, nil); }
      });
      if (!OLHostPing()) { query.finishReason = @"initial-ping-unavailable"; OLHostFinish(query, nil); }
    } else {
      query.finishReason = @"unsupported-system-version"; OLHostFinish(query, nil);
    }
  } @catch (__unused NSException *exception) {
    query.finishReason = @"query-exception";
    OLHostFinish(query, nil);
  }
}

void OLKeyboardResolveHostBundleIdentifier(UIInputViewController *controller,
                                          void (^completion)(NSString * _Nullable)) {
  dispatch_async(dispatch_get_main_queue(), ^{ OLHostBeginQuery(controller, completion); });
}
