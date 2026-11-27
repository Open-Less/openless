import { CapsulePickerMenu, type CapsulePickerDeps } from './capsulePickerMenu';
import type { CapsuleStyleMenuKey, CapsuleStyleMenuOwner } from '../../api/contracts';

function expect(actual: unknown, expected: unknown) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`Expected ${JSON.stringify(expected)}, received ${JSON.stringify(actual)}`);
  }
}

const controller = new CapsulePickerMenu();
const log: string[] = [];
const language = { kind: 'language', sessionId: 'first' } as const;
const style = { kind: 'style', sessionId: 'first', requestId: 'open-1' } as const;
const deps: CapsulePickerDeps = {
  native: true,
  setNativeMenu: async (open, owner) => {
    log.push(`${owner.kind}:${open}:${owner.sessionId}`);
  },
  waitForGeometry: async () => {},
  onOpenChange: (open) => log.push(`render:${open}`),
  onFailure: () => log.push('error'),
};

await controller.open(language, deps);
expect(controller.sessionForChoice('language'), 'first');
controller.close('language');
// Base UI can deliver the choice during the language popup's exit.
expect(controller.sessionForChoice('language'), 'first');
await controller.open(style, deps);
controller.openChangeComplete('language', false);
expect(controller.owns('style', 'first', 'open-1'), true);
expect(controller.owns('style', 'first', 'older-open'), false);
expect(controller.sessionForChoice('language'), undefined);
controller.update('style', true, { sessionId: 'next', mutable: true });
expect(controller.owns('style', 'first', 'open-1'), false);
await controller.open({ ...style, sessionId: 'next', requestId: 'open-2' }, deps);
controller.close('style');
await controller.open({ ...language, sessionId: 'next' }, deps);
expect(log, [
  'language:true:first',
  'render:true',
  'render:false',
  'language:false:first',
  'style:true:first',
  'render:true',
  'render:false',
  'style:false:first',
  'style:true:next',
  'render:true',
  'render:false',
  'style:false:next',
  'language:true:next',
  'render:true',
]);

const inputController = new CapsulePickerMenu();
const nativeOwner: CapsuleStyleMenuOwner = {
  sessionId: style.sessionId,
  menuId: 'menu-1',
  windowLabel: 'capsule',
  keyboardAvailable: true,
};
const key: CapsuleStyleMenuKey = {
  sessionId: style.sessionId,
  menuId: nativeOwner.menuId,
  key: 'ArrowDown',
  repeat: false,
};
const received: string[] = [];
const closedByFrontend: string[] = [];
let startReserve = () => {};
let finishReserve = (_: CapsuleStyleMenuOwner) => {};
const reserveStarted = new Promise<void>((resolve) => {
  startReserve = resolve;
});
const reserve = inputController.open(style, {
  ...deps,
  setNativeMenu: async (open, _, reserved) => {
    if (!open) {
      closedByFrontend.push(reserved!.menuId);
      return;
    }
    return new Promise((resolve) => {
      finishReserve = resolve;
      startReserve();
    });
  },
});
await reserveStarted;
inputController.receiveStyleKey({ ...key, menuId: 'menu-old' });
inputController.receiveStyleKey(key);
inputController.receiveStyleClose({ ...key, menuId: 'menu-old' });
finishReserve(nativeOwner);
await reserve;
expect(received, []);
inputController.enableStyleKeys(style.sessionId, style.requestId, (event) =>
  received.push(event.key),
);
expect(received, ['ArrowDown']);
inputController.receiveStyleKey({ ...key, sessionId: 'previous' });
inputController.receiveStyleKey({ ...key, menuId: 'menu-old' });
inputController.receiveStyleClose({ ...key, menuId: 'menu-old' });
inputController.receiveStyleKey({ ...key, key: 'ArrowUp' });
expect(received, ['ArrowDown', 'ArrowUp']);

// A foreground move can revoke ownership without focusing or blurring the WebView.
inputController.receiveStyleClose(nativeOwner);
inputController.receiveStyleClose(nativeOwner);
inputController.receiveStyleKey(key);
expect(inputController.owns('style', style.sessionId, style.requestId), false);
expect(received, ['ArrowDown', 'ArrowUp']);

const nativeDeps: CapsulePickerDeps = {
  ...deps,
  setNativeMenu: async (open, owner, reserved) => {
    if (open) return { ...nativeOwner, menuId: owner.requestId!, keyboardAvailable: false };
    if (reserved) closedByFrontend.push(reserved.menuId);
  },
};
const replacement = { ...style, requestId: 'menu-2' };
await inputController.open(replacement, nativeDeps);
inputController.receiveStyleClose(nativeOwner);
inputController.enableStyleKeys(style.sessionId, replacement.requestId, (event) =>
  received.push(event.key),
);
inputController.receiveStyleKey({ ...key, menuId: replacement.requestId });
expect(received, ['ArrowDown', 'ArrowUp']);
expect(inputController.owns('style', style.sessionId, replacement.requestId), true);
expect(closedByFrontend, []);
inputController.close('style');
inputController.receiveStyleClose({ ...nativeOwner, menuId: replacement.requestId });
await inputController.open(language, deps);
expect(closedByFrontend, ['menu-2']);

const closedBeforeResponse = new CapsulePickerMenu();
let acknowledgeReservation = (_: CapsuleStyleMenuOwner) => {};
const hostClosedEarly = closedBeforeResponse.open(style, {
  ...nativeDeps,
  setNativeMenu: async (open, _, reserved) => {
    if (!open) {
      if (reserved) closedByFrontend.push(reserved.menuId);
      return;
    }
    return new Promise((resolve) => {
      acknowledgeReservation = resolve;
    });
  },
  onOpenChange: (open) => received.push(`render:${open}`),
});
await new Promise((resolve) => setTimeout(resolve, 0));
closedBeforeResponse.receiveStyleClose(nativeOwner);
acknowledgeReservation(nativeOwner);
await hostClosedEarly;
await closedBeforeResponse.open(language, deps);
expect(received, ['ArrowDown', 'ArrowUp', 'render:false']);
expect(closedByFrontend, ['menu-2']);

log.length = 0;
const openingController = new CapsulePickerMenu();
let finishGeometry: () => void = () => {};
const opening = openingController.open(style, {
  ...deps,
  waitForGeometry: () =>
    new Promise((resolve) => {
      finishGeometry = resolve;
    }),
});
await new Promise((resolve) => setTimeout(resolve, 0));
openingController.handleKeydown('style', {
  key: 'Escape',
  preventDefault: () => log.push('escape'),
});
finishGeometry();
await opening;
await openingController.open(language, deps);
expect(log, [
  'style:true:first',
  'escape',
  'render:false',
  'style:false:first',
  'language:true:first',
  'render:true',
]);
