// @ts-nocheck — Node harness; the application has no Node types.
import assert from 'node:assert/strict';
import { createInteractionController } from '../overlays/interactionController';
import { createToast, toastVariant } from './toast';

const controller = createInteractionController();
const scope = controller.createScope();
const toast = createToast(scope.interaction.notify);
const dialog = scope.interaction.confirm({ title: 'Delete?' });
const request = controller.getRequest();
for (const variant of ['default', 'info', 'error', 'warning', 'success', 'loading'] as const) {
  const publish = variant === 'default' ? toast : toast[variant];
  const handle = publish(variant);
  assert.equal(toastVariant(controller.getNotifications()[0]), variant);
  assert.equal(controller.getRequest(), request, 'the toast must not affect a modal decision');
  handle.dismiss();
}
assert.equal(toastVariant({ state: 'deleting', message: 'Deleting' }), 'trash');
assert.equal(toastVariant({ state: 'deleted', message: 'Deleted' }), 'trash');
assert.equal(
  toastVariant({ state: 'failed', severity: 'warning', message: 'Delete failed' }),
  'warning',
);

const previous = scope.interaction.notify({ state: 'saved', message: 'First saved' });
const download = toast.loading('Downloading', { progress: true });
const saving = scope.interaction.notify({ state: 'saving', message: 'Saving again' });
assert.deepEqual(
  controller.getNotifications().map((item) => item.message),
  ['Saving again', 'Downloading'],
);
previous.update({ message: 'Stale receipt' });
assert.equal(controller.getNotifications().length, 2);
saving.dismiss();
download.dismiss();

request?.cancel();
assert.equal(await dialog, false);

let resolve!: (value: number) => void;
const pending = new Promise<number>((done) => {
  resolve = done;
});
const operation = toast.promise(
  pending,
  { loading: 'Waiting', success: (n) => `Saved ${n}`, error: 'Failed' },
  {
    progress: true,
    actionLabel: 'Cancel',
    onAction: () => {},
  },
);
const [loading] = controller.getNotifications();
assert.equal(loading.variant, 'loading');
resolve(3);
assert.equal(await operation, 3);
const [success] = controller.getNotifications();
assert.equal(success.id, loading.id, 'settled promises update rather than stack');
assert.equal(success.variant, 'success');
assert.equal(success.message, 'Saved 3');
assert.equal(success.progress, false);
assert.equal(success.actionLabel, undefined);

const failure = new Error('fixture error');
await assert.rejects(
  toast.promise(
    () => {
      throw failure;
    },
    {
      loading: 'Waiting',
      success: 'Saved',
      error: 'Could not save',
    },
  ),
  failure,
);
assert.equal(controller.getNotifications()[0].variant, 'error');
assert.equal(controller.getNotifications()[0].message, 'Could not save');
scope.dispose();
assert.equal(controller.getNotifications().length, 0);
console.log('Toast variants, scoped dismissal, and in-place promise settlement passed');
