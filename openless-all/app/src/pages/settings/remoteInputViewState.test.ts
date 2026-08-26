import { getRemoteInputViewState } from './remoteInputViewState'

function assertEqual(actual: unknown, expected: unknown): void {
  if (actual !== expected) {
    throw new Error(`expected ${String(expected)}, got ${String(actual)}`)
  }
}

const status = (overrides: Partial<Parameters<typeof getRemoteInputViewState>[1]> = {}) => ({
  running: false,
  starting: false,
  port: 8443,
  pin: '000000',
  urls: [],
  urlsStale: false,
  ...overrides,
})

assertEqual(getRemoteInputViewState(false, null, null), 'disabled')
assertEqual(getRemoteInputViewState(true, null, null), 'loading')
assertEqual(getRemoteInputViewState(true, status({ starting: true }), null), 'starting')
assertEqual(getRemoteInputViewState(true, status({ running: true }), null), 'running')
assertEqual(getRemoteInputViewState(true, status({ running: true, urlsStale: true }), null), 'stale')
assertEqual(getRemoteInputViewState(true, status(), null), 'waiting')
assertEqual(getRemoteInputViewState(true, status(), { reason: 'port-in-use', port: 8443 }), 'error')

console.log('remote input view state tests passed')
