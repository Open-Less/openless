# Remote input pairing and reconnection

Pairing admission is independent of established WebSocket connections and audio
sessions. The Core owns these rules for both the Tauri and Linux hosts.

- A six-digit PIN is generated in memory. It expires after ten minutes and is
  consumed atomically by one successful pairing. A new PIN is guaranteed to differ
  from the code it replaces. Legacy persisted PINs are no longer loaded.
- A host-owned background task refreshes an expired code and publishes a public
  status-change event. The event contains no PIN or reconnect credential; the
  desktop settings page reads the new code through local IPC.
- Authentication also checks expiration, independently of timer scheduling.
  Failed attempts neither consume a PIN nor reset the existing lockout counters.
- Refreshing a code does not restart the listener, remove authenticated phones,
  cancel audio, change stream sequence numbers, or revoke result recovery keys.
- The phone receives a random 256-bit reconnect token in its successful pairing
  reply. Core retains only a SHA-256 verifier. A token admits reconnects for up to
  24 hours within the current application process; it is not renewed by reconnects.
  Expiration affects new connections, not a recording already in progress.
- The phone saves the reconnect token instead of the PIN. A browser storage
  failure still allows reconnecting within the same page. A temporarily locked
  authentication attempt does not erase the token.
- The in-memory store accepts up to 64 unexpired pairing credentials. Reaching
  that limit rejects additional pairings instead of evicting another phone.
- **Refresh code** calls `rotate_pairing_pin` and preserves connections.
  **Revoke all pairings** keeps `regenerate_pairing_pin`'s explicit revocation
  behavior: replace the PIN, invalidate reconnect/recovery credentials, cancel
  active audio and restart the listener. Disabling the listener or changing its
  port also revokes its connection credentials.

## WebSocket handshake

First pairing sends `{"type":"hello","pin":"123456"}`. The direct successful
reply includes `{"type":"auth","ok":true,"reconnectToken":"<64 hex characters>"}`.
Reconnection sends `{"type":"hello","reconnectToken":"<saved token>"}`.
Invalid tokens produce `reason:"bad-token"`; they never fall back to a supplied
PIN. Rate limiting continues to use `reason:"locked"`.

Tokens must not be placed in URLs, backend status snapshots, replay events or
logs. Session recovery remains separately authorized by the existing per-session
recovery key, after connection authentication.

## Regression checks

From `openless-all/app`:

```sh
cargo test --locked -p openless-core --test remote_input_contract
cargo test --locked --manifest-path src-tauri/backend-tests/Cargo.toml --test remote_websocket
node --test scripts/remote-input-pairing.test.mjs scripts/remote-input-audio-generation.test.mjs scripts/remote-input-audio-queue.test.mjs scripts/remote-input-locales.test.mjs scripts/pin-persistence-security-contract.test.mjs
npm run build
```

Core tests advance virtual time and cover concurrent single-use admission,
expiration without timer execution, active PCM and subsequent recordings,
in-flight finalization, credential isolation, reconnection and explicit revocation.
The Tauri transport test opens two real loopback WebSockets and verifies PCM,
final-result delivery and subsequent recordings across admission rotations.
This transport test does not exercise phone hardware or TLS trust installation.
