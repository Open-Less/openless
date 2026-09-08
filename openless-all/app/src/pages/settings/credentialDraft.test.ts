import { CredentialDraft } from './credentialDraft';

function assert(condition: unknown, message: string) { if (!condition) throw new Error(message); }
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const tick = () => new Promise(resolve => setTimeout(resolve, 0));

async function main() {
  const writes: string[] = [];
  const first = deferred<void>();
  const second = deferred<void>();
  const draft = new CredentialDraft(async () => 'saved', value => {
    writes.push(`channel-A:ark.model_id:${value}`);
    return writes.length === 1 ? first.promise : second.promise;
  });
  await draft.load();
  assert(draft.snapshot().value === 'saved' && writes.length === 0, 'reading must not write');
  draft.edit('m1');
  const save1 = draft.flush();
  assert(draft.flush() === save1, 'blur and close must share the pending save');
  await tick();
  draft.edit('m2');
  const save2 = draft.flush();
  assert(writes.length === 1, 'm2 must wait for m1');
  first.reject(new Error('old failure'));
  await save1;
  await tick();
  assert(draft.snapshot().value === 'm2' && draft.snapshot().status === 'saving', 'old failure must not replace new status');
  assert(writes.join(',') === 'channel-A:ark.model_id:m1,channel-A:ark.model_id:m2', 'writes must capture account and order');
  second.resolve();
  assert(await save2, 'new value must save after old failure');
  assert(!draft.snapshot().dirty && draft.snapshot().value === 'm2', 'only latest save clears dirty');

  const sequence: string[] = [];
  const closeDraft = new CredentialDraft(async () => '', async value => { sequence.push(value); });
  await closeDraft.load();
  closeDraft.edit('debounced'); closeDraft.schedule();
  closeDraft.edit('selected'); await closeDraft.flush();
  await new Promise(resolve => setTimeout(resolve, 350));
  assert(sequence.join(',') === 'selected', 'selection cancels an older debounce');
  closeDraft.edit('before-switch'); closeDraft.schedule();
  if (await closeDraft.flush()) sequence.push('new-provider-default');
  closeDraft.dispose();
  assert(sequence.slice(-2).join(',') === 'before-switch,new-provider-default', 'switch must follow the pending draft');

  let failed = true;
  const retry = new CredentialDraft(async () => 'original', async () => { if (failed) throw new Error('save'); });
  await retry.load(); retry.edit('custom');
  assert(!await retry.flush() && retry.snapshot().dirty, 'failed write must block close and retain the draft');
  failed = false;
  assert(await retry.flush() && !retry.snapshot().dirty, 'same edit must be retryable');

  const oldRead = deferred<string | null>();
  let reads = 0;
  const reading = new CredentialDraft(() => ++reads === 1 ? oldRead.promise : Promise.resolve('new'), async () => undefined);
  const initial = reading.load();
  reading.edit('too-early');
  assert(reading.snapshot().value === '', 'reading must disable editing');
  reading.dispose(); await reading.load(); reading.edit('typed');
  oldRead.resolve('stale'); await initial;
  assert(reading.snapshot().value === 'typed', 'old read must not replace a new scope or edit');
  draft.dispose(); retry.dispose(); reading.dispose();
}
void main();
