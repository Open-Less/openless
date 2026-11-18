// @ts-nocheck — Node runtime harness; application builds do not include Node types.
import assert from 'node:assert/strict';
import { createInteractionController } from './interactionController';
import { bindNotificationSink } from '../../lib/notifications';
import { emitSaved } from '../../lib/savedEvent';

const controller = createInteractionController();
const first = controller.createScope();
const second = controller.createScope();
try {
  const saving = first.interaction.notify({ key: 'save', state: 'saving', message: 'Saving' });
  const id = controller.getNotifications()[0].id;
  first.interaction.notify({ key: 'save', state: 'saved', message: 'Saved' });
  assert.equal(controller.getNotifications().length, 1);
  assert.equal(controller.getNotifications()[0].id, id);
  saving.update({ state: 'failed', message: 'Could not save' });
  assert.equal(controller.getNotifications()[0].message, 'Could not save');

  second.interaction.notify({ key: 'save', state: 'saving', message: 'New owner saving' });
  const next = controller.getNotifications()[0];
  assert.notEqual(next.id, id);
  saving.update({ state: 'saved', message: 'Stale result' });
  saving.dismiss();
  first.dispose();
  assert.equal(controller.getNotifications()[0], next);
  second.dispose();
  assert.equal(controller.getNotifications().length, 0);
  second.interaction.notify({ state: 'saved', message: 'Disposed owner' });
  assert.equal(controller.getNotifications().length, 0);
} finally {
  first.dispose();
  second.dispose();
  controller.clear();
}

const bridge = controller.createScope();
const unbind = bindNotificationSink({
  notify: bridge.interaction.notify,
  dismissByKey: controller.dismissByKey,
});
try {
  emitSaved('saving', 'First saving', 'first');
  const firstId = controller.getNotifications()[0].id;
  emitSaved('saving', 'Second saving', 'second');
  emitSaved('saved', 'First saved', 'first');
  assert.equal(controller.getNotifications().length, 2);
  assert.equal(
    controller.getNotifications().find((item) => item.message === 'First saved')?.id,
    firstId,
  );
  assert(controller.getNotifications().some((item) => item.message === 'Second saving'));
} finally {
  unbind();
  bridge.dispose();
  controller.clear();
}
