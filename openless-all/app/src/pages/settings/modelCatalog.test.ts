import { ModelCatalog } from './modelCatalog';
import { CredentialDraft } from './credentialDraft';

function assert(condition: unknown, message: string) { if (!condition) throw new Error(message); }
function deferred() {
  let resolve!: (value: { models: string[] }) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<{ models: string[] }>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
async function main() {
  const catalog = new ModelCatalog();
  let version = 0;
  const current = () => version;
  for (const failOld of [false, true]) {
    const a = deferred(); const b = deferred();
    const first = catalog.load(version, () => a.promise, current);
    const second = catalog.load(version, () => b.promise, current);
    b.resolve({ models: ['new'] }); await second;
    if (failOld) a.reject(new Error('old authentication error')); else a.resolve({ models: ['old'] });
    await first;
    assert(catalog.snapshot()?.models.join() === 'new' && catalog.snapshot()?.status === 'success', 'old response or error must be ignored');
  }
  const editing = deferred();
  const pending = catalog.load(version, () => editing.promise, current);
  version += 1; // endpoint/key/protocol edit invalidates immediately, before saving.
  editing.resolve({ models: ['stale-config'] }); await pending;
  assert(catalog.snapshot()?.version !== version, 'old candidates must not belong to the edited configuration');
  const unmounted = deferred();
  const closed = catalog.load(version, () => unmounted.promise, current);
  catalog.invalidate(); unmounted.reject(new Error('after close')); await closed;
  assert(catalog.snapshot()?.status === 'loading', 'unmounted error must not publish');

  const writes: string[] = [];
  const draft = new CredentialDraft(async () => 'missing-from-catalog', async value => { writes.push(value); });
  await draft.load();
  const directory = deferred();
  const fetching = catalog.load(version, () => directory.promise, current);
  draft.edit('typed-during-request'); await draft.flush();
  directory.resolve({ models: ['different-model'] }); await fetching;
  await catalog.load(version, async () => ({ models: [] }), current);
  await catalog.load(version, async () => { throw new Error('401'); }, current);
  assert(draft.snapshot().value === 'typed-during-request' && writes.join() === 'typed-during-request', 'success, empty and failure cannot write or clear the model');
  assert(!draft.snapshot().dirty, 'catalog failure cannot block a saved manual model');
  draft.dispose();
}
void main();
