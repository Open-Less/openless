import { CapsulePickerMenu, type CapsulePickerDeps } from './capsulePickerMenu';

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
  'language:true:first', 'render:true', 'render:false', 'language:false:first',
  'style:true:first', 'render:true', 'render:false', 'style:false:first',
  'style:true:next', 'render:true', 'render:false', 'style:false:next',
  'language:true:next', 'render:true',
]);

log.length = 0;
const openingController = new CapsulePickerMenu();
let finishGeometry: () => void = () => {};
const opening = openingController.open(style, {
  ...deps,
  waitForGeometry: () => new Promise((resolve) => { finishGeometry = resolve; }),
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
  'style:true:first', 'escape', 'render:false', 'style:false:first',
  'language:true:first', 'render:true',
]);
