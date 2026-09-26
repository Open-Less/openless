// @ts-nocheck — Node-only runtime harness; production UI and IPC remain strictly typed.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
import * as api from '../../lib/ipc/cloud-sync-e2ee';
import type { EncryptedSyncStatus, RestorePreview } from '../../lib/ipc/cloud-sync-e2ee';

// Replay the real component handlers; only React rendering/hooks and IPC/event
// boundaries are replaced. No browser vault, password store or network is used.
type Node = { type: unknown; props: Record<string, any> };
type Hook = { value?: any; dependencies?: unknown[]; cleanup?: () => void };
class Hooks {
  slots: Hook[] = [];
  cursor = 0;
  pending: Array<() => void> = [];
  constructor(seed: unknown[] = []) {
    this.slots = seed.map((value) => ({ value }));
  }
  state(initial: unknown) {
    const index = this.cursor++;
    this.slots[index] ??= { value: initial };
    return [
      this.slots[index].value,
      (value: any) => {
        this.slots[index].value =
          typeof value === 'function' ? value(this.slots[index].value) : value;
      },
    ];
  }
  ref(initial: unknown) {
    return this.state({ current: initial })[0];
  }
  effect(effect: () => void | (() => void), dependencies?: unknown[]) {
    const index = this.cursor++;
    const previous = this.slots[index];
    if (
      previous &&
      dependencies &&
      previous.dependencies?.length === dependencies.length &&
      dependencies.every((value, i) => Object.is(value, previous.dependencies?.[i]))
    )
      return;
    const entry: Hook = { dependencies };
    this.slots[index] = entry;
    this.pending.push(() => {
      previous?.cleanup?.();
      entry.cleanup = effect() || undefined;
    });
  }
  render<T>(fn: () => T): T {
    this.cursor = 0;
    active = this;
    const result = fn();
    this.pending.splice(0).forEach((effect) => effect());
    return result;
  }
  unmount() {
    this.slots.forEach((slot) => slot.cleanup?.());
  }
}
let active: Hooks;
function nodes(value: unknown): Node[] {
  if (Array.isArray(value)) return value.flatMap(nodes);
  if (!value || typeof value !== 'object' || !('props' in value)) return [];
  const node = value as Node;
  return [node, ...nodes(node.props.children)];
}
function find(tree: unknown, predicate: (node: Node) => boolean): Node {
  const node = nodes(tree).find(predicate);
  assert(node, 'expected rendered control');
  return node;
}
const settle = () => new Promise<void>((resolve) => setImmediate(resolve));
const source = readFileSync(new URL('./CloudSyncSection.tsx', import.meta.url), 'utf8');
const parsed = ts.createSourceFile(
  'CloudSyncSection.tsx',
  source,
  ts.ScriptTarget.ES2020,
  true,
  ts.ScriptKind.TSX,
);
const imports = parsed.statements.filter(ts.isImportDeclaration).flatMap((node) => {
  const bindings = node.importClause?.namedBindings;
  return bindings && ts.isNamedImports(bindings)
    ? bindings.elements.map((item) => item.name.text)
    : [];
});
const body = parsed.statements
  .filter((node) => !ts.isImportDeclaration(node))
  .map((node) => node.getFullText(parsed))
  .join('\n')
  .replace("import('@tauri-apps/api/event')", 'Promise.resolve({listen: __listen})');
const compiled = ts.transpileModule(body, {
  compilerOptions: {
    target: ts.ScriptTarget.ES2020,
    module: ts.ModuleKind.CommonJS,
    jsx: ts.JsxEmit.ReactJSX,
  },
}).outputText;
const factory = new Function(
  ...imports,
  'exports',
  'require',
  '__listen',
  `${compiled};return {CloudSyncSection,PasswordForm,ConsentForm,ProtocolWarning,RestoreReview};`,
);

const initial: EncryptedSyncStatus = {
  sequence: '1',
  enabled: false,
  authState: 'signed_in',
  keyState: 'locked',
  syncState: 'disabled',
  account: { githubId: 'owner', login: 'fixture' },
  vaultId: 'vault',
  keyId: 'key',
  localGeneration: '1',
  lastSyncedLocalGeneration: null,
  remoteRevision: '7',
  lastSuccessfulSyncAt: null,
  pendingOperationId: null,
  lastError: null,
  recoveryRequired: false,
  hasCloudSnapshot: true,
  taskId: null,
  serviceOrigin: 'https://sync.example',
  backupRetentionDays: null,
  consentVersion: null,
};
const preview: RestorePreview = {
  previewId: 'review',
  observedRevision: '7',
  localGeneration: '1',
  counts: { history: 2 },
  deviceSettingsToReview: [],
  conflicts: [{ id: 'opaque-conflict', kind: 'history', reason: 'no_common_baseline' }],
};
const events = new Map<string, (event: { payload: unknown }) => void>();
const calls: Array<[string, any?]> = [];
let server = { ...initial };
let deferredStatus: Promise<EncryptedSyncStatus> | null = null;
let nextStep: api.EnablePreparation['nextStep'] = 'unlock';
const update = (value: Partial<EncryptedSyncStatus>) => {
  server = { ...server, ...value, sequence: (BigInt(server.sequence) + 1n).toString() };
  return { ...server };
};
const dependencies: Record<string, any> = {
  ...api,
  CONSENT_VERSION: api.CLOUD_SYNC_E2EE_CONSENT_VERSION,
  useState: (value: unknown) => active.state(value),
  useRef: (value: unknown) => active.ref(value),
  useEffect: (effect: () => void | (() => void), deps?: unknown[]) => active.effect(effect, deps),
  useId: () => 'fixture-id',
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: 'en' } }),
  useHotkeySettings: () => ({
    prefs: { marketplaceDevLogin: 'fixture' },
    refresh: async () => {
      calls.push(['refresh']);
    },
    updatePrefs: async () => {},
  }),
  Icon: 'icon',
  GithubLoginModal: 'login',
  Modal: 'modal',
  Btn: 'button',
  Card: 'card',
  isTauri: true,
  marketplaceAuthStatus: async () => ({ signedIn: true }),
  cloudSyncE2eeStatus: async () => deferredStatus ?? { ...server },
  mirrorEncryptedSyncUiPreferences: async () => {
    calls.push(['mirror']);
  },
  cloudSyncE2eePrepareEnable: async (consent: string) => {
    calls.push(['prepare', consent]);
    return { nextStep, status: update({ consentVersion: consent }) };
  },
  cloudSyncE2eeCreate: async (value: unknown) => {
    assert.equal(server.hasCloudSnapshot, false, 'must not create over an existing cloud backup');
    calls.push(['create', value]);
    return update({
      hasCloudSnapshot: true,
      vaultId: 'new-vault',
      keyState: 'unlocked',
      enabled: true,
    });
  },
  cloudSyncE2eeUnlock: async (value: unknown) => {
    calls.push(['unlock', value]);
    nextStep = 'restore_review';
    return update({ keyState: 'unlocked' });
  },
  cloudSyncE2eePreviewRestore: async (revision: string) => {
    calls.push(['preview', revision]);
    return preview;
  },
  cloudSyncE2eeApplyRestore: async (value: unknown) => {
    calls.push(['apply', value]);
    return update({});
  },
  cloudSyncE2eeSetEnabled: async (enabled: boolean) => {
    calls.push(['enable', enabled]);
    return update({ enabled });
  },
};
const jsx = (type: unknown, props: Record<string, unknown>) => ({ type, props });
const components = factory(
  ...imports.map((name) => dependencies[name]),
  {},
  () => ({ jsx, jsxs: jsx }),
  async (name: string, callback: (event: { payload: unknown }) => void) => {
    events.set(name, callback);
    return () => events.delete(name);
  },
);
Object.defineProperty(globalThis, 'window', { configurable: true, value: new EventTarget() });

try {
  for (const mode of ['create', 'unlock', 'password'] as const) {
    const hooks = new Hooks(['LongFixture1Aa', 'LongFixture1Aa', 'OldFixture1Aa', true]);
    let submitted = 0;
    const form = hooks.render(() =>
      components.PasswordForm({
        mode,
        busy: false,
        onSubmit: (value: any) => {
          assert.deepEqual(
            hooks.slots.slice(0, 3).map((slot) => slot.value),
            ['', '', ''],
          );
          assert.equal(value.password, 'LongFixture1Aa');
          assert.equal(value.rememberKey, true);
          submitted += 1;
        },
      }),
    );
    form.props.onCompositionStart();
    form.props.onSubmit({ preventDefault() {} });
    assert.equal(submitted, 0, 'IME confirmation must not submit or clear the password');
    let prevented = false;
    form.props.onKeyDown({
      key: 'Enter',
      nativeEvent: { isComposing: true },
      keyCode: 13,
      preventDefault() {
        prevented = true;
      },
    });
    assert(prevented);
    form.props.onCompositionEnd();
    form.props.onSubmit({ preventDefault() {} });
    assert.equal(submitted, 1);
  }
  const passwordHooks = new Hooks();
  const blankForm = passwordHooks.render(() =>
    components.PasswordForm({
      mode: 'create',
      busy: false,
      onSubmit() {
        assert.fail('blank passwords must not submit');
      },
    }),
  );
  assert(
    find(blankForm, (node) => node.type === 'input' && node.props.type === 'checkbox').props
      .checked,
  );
  assert(
    find(blankForm, (node) => node.type === 'button' && node.props.type === 'submit').props
      .disabled,
  );
  const consentHooks = new Hooks();
  const consent = consentHooks.render(() =>
    components.ConsentForm({ busy: false, onConfirm() {} }),
  );
  assert.equal(find(consent, (node) => node.type === 'input').props.checked, false);
  assert.equal(find(consent, (node) => node.type === 'button').props.disabled, true);

  const protocolHooks = new Hooks();
  const protocolProps = { busy: false, onConfirm() {}, onBack() {} };
  let protocolTree = protocolHooks.render(() => components.ProtocolWarning(protocolProps));
  const confirmProtocol = () =>
    find(protocolTree, (node) => node.props.children === 'cloudSyncE2ee.protocolConfirm');
  assert.equal(confirmProtocol().props.disabled, true);
  find(protocolTree, (node) => node.type === 'input').props.onChange({
    currentTarget: { checked: true },
  });
  protocolTree = protocolHooks.render(() => components.ProtocolWarning(protocolProps));
  assert.equal(confirmProtocol().props.disabled, false);
  protocolTree = protocolHooks.render(() =>
    components.ProtocolWarning({ ...protocolProps, busy: true }),
  );
  assert.equal(confirmProtocol().props.disabled, true);

  const hooks = new Hooks();
  const render = () => hooks.render(() => components.CloudSyncSection());
  render();
  await settle();
  let tree = render();
  assert.equal(find(tree, (node) => node.props.role === 'switch').props.checked, false);
  assert.deepEqual(calls, [], 'mount must not enable, upload or consent');
  find(tree, (node) => node.props.role === 'switch').props.onChange({
    currentTarget: { checked: true },
  });
  tree = render();
  find(tree, (node) => node.type === components.ConsentForm).props.onConfirm();
  await settle();
  tree = render();
  assert.deepEqual(
    calls,
    [],
    'scope consent alone must not prepare or upload before the privacy warning',
  );
  find(tree, (node) => node.type === components.ProtocolWarning).props.onBack();
  tree = render();
  assert.deepEqual(calls, [], 'returning from the warning must not perform native sync work');
  find(tree, (node) => node.type === components.ConsentForm).props.onConfirm();
  tree = render();
  find(tree, (node) => node.type === components.ProtocolWarning).props.onConfirm();
  await settle();
  tree = render();
  assert.deepEqual(
    calls.slice(0, 2).map(([name]) => name),
    ['mirror', 'prepare'],
  );
  const unlock = find(tree, (node) => node.type === components.PasswordForm);
  assert.equal(unlock.props.mode, 'unlock');
  unlock.props.onSubmit({
    password: 'LongFixture1Aa',
    confirmation: '',
    currentPassword: '',
    rememberKey: true,
  });
  await settle();
  tree = render();
  const review = find(tree, (node) => node.type === components.RestoreReview);
  assert.equal(review.props.stale, false);
  assert(
    !calls.some(([name]) => name === 'enable' || name === 'apply'),
    'existing cloud must be reviewed before enable',
  );
  const reviewHooks = new Hooks();
  let reviewTree = reviewHooks.render(() => components.RestoreReview(review.props));
  const applyButton = () =>
    find(reviewTree, (node) => node.props.children === 'cloudSyncE2ee.applyRestore');
  assert.equal(applyButton().props.disabled, true);
  find(
    reviewTree,
    (node) => node.type === 'input' && node.props.name?.startsWith('e2ee-conflict'),
  ).props.onChange();
  reviewTree = reviewHooks.render(() => components.RestoreReview(review.props));
  find(
    reviewTree,
    (node) => node.type === 'input' && node.props.type === 'checkbox',
  ).props.onChange({ currentTarget: { checked: true } });
  reviewTree = reviewHooks.render(() => components.RestoreReview(review.props));
  assert.equal(applyButton().props.disabled, false);
  applyButton().props.onClick();
  await settle();
  tree = render();
  assert.deepEqual(calls.find(([name]) => name === 'apply')?.[1], {
    previewId: 'review',
    mode: 'merge',
    conflictChoices: [{ id: 'opaque-conflict', side: 'local' }],
  });
  assert(
    calls.findIndex(([name]) => name === 'enable') > calls.findIndex(([name]) => name === 'apply'),
  );

  // A delayed status RPC must never roll back a newer scoped state event.
  let resolveStatus!: (value: EncryptedSyncStatus) => void;
  deferredStatus = new Promise((resolve) => {
    resolveStatus = resolve;
  });
  find(tree, (node) => node.props['aria-label'] === 'cloudSyncE2ee.refresh').props.onClick();
  const old = { ...server };
  const newer = update({ enabled: true });
  events.get('cloud-sync-e2ee:state')!({
    payload: {
      sequence: newer.sequence,
      accountId: 'owner',
      vaultId: 'vault',
      taskId: null,
      status: newer,
    },
  });
  resolveStatus({ ...old, enabled: false });
  await settle();
  deferredStatus = null;
  tree = render();
  assert.equal(find(tree, (node) => node.props.role === 'switch').props.checked, true);
  events.get('cloud-sync-e2ee:conflict')!({
    payload: {
      sequence: (BigInt(newer.sequence) + 1n).toString(),
      accountId: 'owner',
      vaultId: 'vault',
      taskId: 'old-task',
      preview,
    },
  });
  tree = render();
  assert(!nodes(tree).some((node) => node.type === components.RestoreReview));
  const pending = update({
    enabled: false,
    keyState: 'locked',
    syncState: 'outcome_unknown',
    pendingOperationId: 'operation',
  });
  events.get('cloud-sync-e2ee:state')!({
    payload: {
      sequence: pending.sequence,
      accountId: 'owner',
      vaultId: 'vault',
      taskId: null,
      status: pending,
    },
  });
  tree = render();
  assert.equal(
    find(tree, (node) => node.props.children === 'cloudSyncE2ee.checkPending').props.disabled,
    false,
    'an uncertain create/delete result must remain reconcilable while sync is off',
  );
  hooks.unmount();
  await settle();
  assert.equal(events.size, 0);

  // Empty/deleted cloud slots keep the observed revision; never invent zero.
  server = {
    ...initial,
    sequence: '20',
    vaultId: null,
    keyId: null,
    hasCloudSnapshot: false,
    remoteRevision: '42',
  };
  nextStep = 'create';
  calls.length = 0;
  const freshHooks = new Hooks();
  const fresh = () => freshHooks.render(() => components.CloudSyncSection());
  fresh();
  await settle();
  tree = fresh();
  assert.deepEqual(calls, []);
  find(tree, (node) => node.props.role === 'switch').props.onChange({
    currentTarget: { checked: true },
  });
  tree = fresh();
  find(tree, (node) => node.type === components.ConsentForm).props.onConfirm();
  await settle();
  tree = fresh();
  assert.deepEqual(calls, [], 'a new vault also requires the second privacy acknowledgement');
  find(tree, (node) => node.type === components.ProtocolWarning).props.onConfirm();
  await settle();
  tree = fresh();
  const createForm = find(tree, (node) => node.type === components.PasswordForm);
  assert.equal(createForm.props.mode, 'create');
  createForm.props.onSubmit({
    password: 'LongFixture1Aa',
    confirmation: 'LongFixture1Aa',
    currentPassword: '',
    rememberKey: true,
  });
  await settle();
  assert.deepEqual(calls.find(([name]) => name === 'create')?.[1], {
    password: 'LongFixture1Aa',
    passwordConfirmation: 'LongFixture1Aa',
    rememberKey: true,
    consentVersion: api.CLOUD_SYNC_E2EE_CONSENT_VERSION,
    observedRevision: '42',
  });
  freshHooks.unmount();
  await settle();
  assert.equal(events.size, 0);
} finally {
  Reflect.deleteProperty(globalThis, 'window');
}
const resources = await Promise.all([
  import('../../i18n/zh-CN').then((module) => module.zhCN.cloudSyncE2ee),
  import('../../i18n/zh-TW').then((module) => module.zhTW.cloudSyncE2ee),
  import('../../i18n/en').then((module) => module.en.cloudSyncE2ee),
  import('../../i18n/de').then((module) => module.de.cloudSyncE2ee),
  import('../../i18n/fr').then((module) => module.fr.cloudSyncE2ee),
  import('../../i18n/es').then((module) => module.es.cloudSyncE2ee),
  import('../../i18n/ja').then((module) => module.ja.cloudSyncE2ee),
  import('../../i18n/ko').then((module) => module.ko.cloudSyncE2ee),
]);
function strings(value: object, prefix = ''): Record<string, string> {
  return Object.fromEntries(
    Object.entries(value).flatMap(([key, entry]) =>
      typeof entry === 'string'
        ? [[`${prefix}${key}`, entry]]
        : Object.entries(strings(entry, `${prefix}${key}.`)),
    ),
  );
}
const canonical = strings(resources[0]);
for (const resource of resources.slice(1)) {
  const translated = strings(resource);
  assert.deepEqual(Object.keys(translated).sort(), Object.keys(canonical).sort());
  for (const [key, value] of Object.entries(translated)) {
    assert(value.trim().length > 0);
    assert.deepEqual(
      value.match(/{{\w+}}/g)?.sort() ?? [],
      canonical[key].match(/{{\w+}}/g)?.sort() ?? [],
    );
  }
}
console.log('CloudSyncSection.test.ts passed');
