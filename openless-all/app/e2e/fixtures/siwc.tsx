import React from 'react';
import { createRoot } from 'react-dom/client';
import { i18nReady } from '../../src/i18n';
import { SiwcChannelFields } from '../../src/pages/settings/SiwcChannelFields';
import type { SiwcStatus } from '../../src/lib/ipc/siwc';
import '../../src/styles/tokens.css';
import '../../src/styles/global.css';
import '../../src/styles/refinements.css';
const account = {
  id: 'fixture-account',
  label: 'tester@example.test · 000001',
  email: 'tester@example.test',
};
const otherAccount = {
  id: 'fixture-other-account',
  label: 'other@example.test · 000002',
  email: 'other@example.test',
};
const state: SiwcStatus = {
  state: 'signed_out',
  account: null,
  accounts: [],
  attemptId: null,
  error: null,
  welcomePending: false,
};
let model = '';
let quota = false;
const calls: { command: string; args: Record<string, unknown> }[] = [];
const invoke = async (command: string, args: Record<string, unknown> = {}) => {
  calls.push({ command, args });
  if (command === 'get_startup_snapshot')
    return { contractVersion: '2.0.0', backend: { running: true } };
  if (command === 'siwc_status') return structuredClone(state);
  if (command === 'siwc_models') return [{ slug: 'fixture-model', displayName: '账号可用模型' }];
  if (command === 'read_credential') return model;
  if (command === 'set_credential') {
    model = String(args.value);
    return;
  }
  if (command === 'validate_provider_credentials') {
    if (quota) {
      state.state = 'usage_limited';
      state.error = { code: 'subscription_sharing_usage_limit_exceeded' };
      throw new Error('siwc:' + JSON.stringify(state.error));
    }
    return { ok: true };
  }
  if (command === 'record_channel_test' || command === 'open_external_url') return;
  if (command === 'siwc_action') {
    switch (args.action) {
      case 'sign_in':
      case 'add_account':
      case 'enable_usage':
        state.state = 'authorizing';
        state.attemptId = 'fixture-attempt';
        return { attemptId: state.attemptId };
      case 'cancel':
        state.state = state.account ? 'connected' : 'signed_out';
        state.attemptId = null;
        break;
      case 'select':
        state.account = state.accounts.find((saved) => saved.id === args.registrationId) ?? null;
        state.state = state.account ? 'connected' : 'signed_out';
        break;
      case 'acknowledge':
        state.welcomePending = false;
        break;
      case 'retry':
        quota = false;
        state.state = 'connected';
        state.error = null;
        break;
      case 'disconnect':
        state.state = 'reauthorization_required';
        return { remoteRevoked: false };
    }
    return null;
  }
  throw Error('Unexpected command: ' + command);
};
Object.assign(window, {
  __TAURI_INTERNALS__: { invoke },
  siwcFixture: {
    calls,
    complete: () => {
      Object.assign(state, {
        state: 'connected',
        account,
        accounts: [account, otherAccount],
        attemptId: null,
        welcomePending: true,
      });
      window.dispatchEvent(new Event('focus'));
    },
    quota: () => {
      quota = true;
    },
  },
});
await i18nReady;
createRoot(document.getElementById('root')!).render(
  <main
    style={{
      maxWidth: 620,
      margin: '32px auto',
      padding: 24,
      background: 'var(--ol-surface)',
      color: 'var(--ol-ink)',
    }}
  >
    <h1>ChatGPT Plan</h1>
    <SiwcChannelFields channelId="fixture-channel" />
  </main>,
);
