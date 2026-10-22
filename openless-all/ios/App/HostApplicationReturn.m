#import "HostApplicationReturn.h"
#import <UIKit/UIKit.h>
#import <objc/message.h>
#import <objc/runtime.h>

// Validate each runtime method's complete ABI before invoking it. A return
// destination belongs only to the current activation; never enumerate apps.
static BOOL OLMethodMatches(id object, SEL selector, const char *result,
                            const char *argument) {
  if (!object) return NO;
  Method method = class_getInstanceMethod(object_getClass(object), selector);
  if (!method) return NO;
  NSMethodSignature *signature = [NSMethodSignature signatureWithObjCTypes:method_getTypeEncoding(method)];
  return signature.numberOfArguments == (argument ? 3 : 2)
      && strcmp(signature.methodReturnType, result) == 0
      && strcmp([signature getArgumentTypeAtIndex:0], @encode(id)) == 0
      && strcmp([signature getArgumentTypeAtIndex:1], @encode(SEL)) == 0
      && (!argument || strcmp([signature getArgumentTypeAtIndex:2], argument) == 0);
}

static BOOL OLValidBundleIdentifier(id value) {
  if (![value isKindOfClass:NSString.class]) return NO;
  NSString *identifier = value;
  NSString *ownBundle = NSBundle.mainBundle.bundleIdentifier;
  if (identifier.length == 0 || identifier.length > 255 || ![identifier containsString:@"."]
      || [identifier isEqualToString:ownBundle]
      || (ownBundle && [identifier hasPrefix:[ownBundle stringByAppendingString:@"."]])) return NO;
  NSCharacterSet *allowed = [NSCharacterSet characterSetWithCharactersInString:
      @"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789.-"];
  return [identifier rangeOfCharacterFromSet:allowed.invertedSet].location == NSNotFound;
}

@interface OLHostApplicationReturnTarget ()
@property(nonatomic, readwrite, copy) NSString *bundleIdentifier;
@property(nonatomic, strong, nullable) id navigationAction;
@property(nonatomic) NSUInteger destination;
@property(nonatomic) BOOL consumed;
- (instancetype)initWithBundleIdentifier:(NSString *)identifier;
@end

@implementation OLHostApplicationReturnTarget
- (instancetype)initWithBundleIdentifier:(NSString *)identifier {
  if ((self = [super init])) _bundleIdentifier = [identifier copy];
  return self;
}
@end

OLHostApplicationReturnTarget *OLHostApplicationCaptureReturnTarget(NSString *expectedBundleID) {
  if (!NSThread.isMainThread) return nil;
  @try {
    if (expectedBundleID && !OLValidBundleIdentifier(expectedBundleID)) return nil;
    OLHostApplicationReturnTarget *target = expectedBundleID
        ? [[OLHostApplicationReturnTarget alloc] initWithBundleIdentifier:expectedBundleID] : nil;
    UIApplication *application = UIApplication.sharedApplication;
    SEL actionSelector = NSSelectorFromString(@"_systemNavigationAction");
    if (!OLMethodMatches(application, actionSelector, @encode(id), NULL)) {
      NSLog(@"OPENLESS_HOST_RETURN capture navigation-selector-unavailable expected=%@", expectedBundleID ?: @"nil");
      return target;
    }
    id action = ((id (*)(id, SEL))objc_msgSend)(application, actionSelector);
    Class actionClass = NSClassFromString(@"UISystemNavigationAction");
    SEL destinationsSelector = NSSelectorFromString(@"destinations");
    SEL bundleSelector = NSSelectorFromString(@"bundleIdForDestination:");
    SEL responseSelector = NSSelectorFromString(@"sendResponseForDestination:");
    if (!actionClass || ![action isKindOfClass:actionClass]
        || !OLMethodMatches(action, destinationsSelector, @encode(id), NULL)
        || !OLMethodMatches(action, bundleSelector, @encode(id), @encode(NSUInteger))
        || !OLMethodMatches(action, responseSelector, @encode(BOOL), @encode(NSUInteger))) {
      NSLog(@"OPENLESS_HOST_RETURN capture navigation-action-unavailable expected=%@", expectedBundleID ?: @"nil");
      return target;
    }
    id destinations = ((id (*)(id, SEL))objc_msgSend)(action, destinationsSelector);
    if (![destinations isKindOfClass:NSArray.class]) return target;
    // An unknown host cannot be resolved by arbitrarily choosing a destination.
    if (!expectedBundleID && [destinations count] != 1) {
      NSLog(@"OPENLESS_HOST_RETURN capture ambiguous-destinations count=%lu", (unsigned long)[destinations count]);
      return nil;
    }
    for (id number in destinations) {
      if (![number isKindOfClass:NSNumber.class]) continue;
      NSUInteger destination = [number unsignedIntegerValue];
      id identifier = ((id (*)(id, SEL, NSUInteger))objc_msgSend)(action, bundleSelector, destination);
      if (!OLValidBundleIdentifier(identifier)
          || (expectedBundleID && ![identifier isEqualToString:expectedBundleID])) continue;
      if (!target) target = [[OLHostApplicationReturnTarget alloc] initWithBundleIdentifier:identifier];
      target.navigationAction = action;
      target.destination = destination;
      break;
    }
    NSLog(@"OPENLESS_HOST_RETURN capture expected=%@ target=%@ navigation=%d", expectedBundleID ?: @"nil",
          target.bundleIdentifier ?: @"nil", target.navigationAction != nil);
    return target;
  } @catch (__unused NSException *exception) {
    return nil;
  }
}

BOOL OLHostApplicationReturn(OLHostApplicationReturnTarget *target) {
  if (!NSThread.isMainThread || !target || target.consumed) return NO;
  target.consumed = YES;
  @try {
    if (!OLValidBundleIdentifier(target.bundleIdentifier)) return NO;
    id action = target.navigationAction;
    SEL responseSelector = NSSelectorFromString(@"sendResponseForDestination:");
    SEL bundleSelector = NSSelectorFromString(@"bundleIdForDestination:");
    if (OLMethodMatches(action, responseSelector, @encode(BOOL), @encode(NSUInteger))
        && OLMethodMatches(action, bundleSelector, @encode(id), @encode(NSUInteger))) {
      id identifier = ((id (*)(id, SEL, NSUInteger))objc_msgSend)(action, bundleSelector, target.destination);
      if ([identifier isKindOfClass:NSString.class] && [identifier isEqualToString:target.bundleIdentifier]) {
        BOOL accepted = ((BOOL (*)(id, SEL, NSUInteger))objc_msgSend)(action, responseSelector, target.destination);
        NSLog(@"OPENLESS_HOST_RETURN navigation target=%@ accepted=%d", target.bundleIdentifier, accepted);
        if (accepted) return YES;
      }
    }
    // Use only the exact host captured for this request, never an app list.
    Class workspaceClass = NSClassFromString(@"LSApplicationWorkspace");
    SEL workspaceSelector = NSSelectorFromString(@"defaultWorkspace");
    SEL openSelector = NSSelectorFromString(@"openApplicationWithBundleID:");
    if (!OLMethodMatches(workspaceClass, workspaceSelector, @encode(id), NULL)) {
      NSLog(@"OPENLESS_HOST_RETURN workspace-unavailable target=%@", target.bundleIdentifier); return NO;
    }
    id workspace = ((id (*)(id, SEL))objc_msgSend)(workspaceClass, workspaceSelector);
    if (!OLMethodMatches(workspace, openSelector, @encode(BOOL), @encode(id))) {
      NSLog(@"OPENLESS_HOST_RETURN open-selector-unavailable target=%@", target.bundleIdentifier); return NO;
    }
    BOOL accepted = ((BOOL (*)(id, SEL, id))objc_msgSend)(workspace, openSelector, target.bundleIdentifier);
    NSLog(@"OPENLESS_HOST_RETURN workspace target=%@ accepted=%d", target.bundleIdentifier, accepted);
    return accepted;
  } @catch (__unused NSException *exception) {
    return NO;
  }
}
