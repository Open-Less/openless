import { withStartupDeadline } from './startupProbe';

function assert(value: unknown, message: string): asserts value {
  if (!value) throw new Error(message);
}
async function rejection(promise: Promise<unknown>, message: string) {
  try {
    await promise;
  } catch (error) {
    assert(error instanceof Error && error.message === message, `Unexpected rejection: ${error}`);
    return;
  }
  throw new Error('Expected the probe to reject');
}

const signal = new AbortController().signal;
const credentials = { asrConfigured: true };
assert(
  (await withStartupDeadline(Promise.resolve(credentials), signal)) === credentials,
  'A responsive store must preserve its actual credential status',
);
await rejection(
  withStartupDeadline(Promise.reject(new Error('Store unavailable')), signal),
  'Store unavailable',
);
await rejection(
  withStartupDeadline(new Promise(() => {}), signal, 5),
  'Startup credential probe timed out',
);

const cancelled = new AbortController();
const pending = withStartupDeadline(new Promise(() => {}), cancelled.signal);
cancelled.abort();
await rejection(pending, 'Startup probe cancelled');
await rejection(
  withStartupDeadline(Promise.resolve(credentials), cancelled.signal),
  'Startup probe cancelled',
);

let finish!: (value: typeof credentials) => void;
const late = withStartupDeadline(
  new Promise<typeof credentials>((resolve) => {
    finish = resolve;
  }),
  signal,
  5,
);
await rejection(late, 'Startup credential probe timed out');
finish(credentials);
await rejection(late, 'Startup credential probe timed out');
console.log('Startup probes preserve success and errors, bound hangs, and release on cancellation');
