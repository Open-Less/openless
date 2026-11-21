# Provider channel selection

For the separate ASR and LLM channel lists, the first enabled channel is the
selected channel. Order is ascending, with channel ID breaking equal-order ties.
The settings list, credential status and new requests must follow this rule.

The stored active channel ID is a compatibility cache, not an independent
selection. Loading desktop credentials reconciles it with the channel list,
including already migrated vaults and restored data. Channel mutations update
the cache. Reconciliation changes the loaded copy; the next authorized credential
mutation persists it through the existing storage path.

New requests resolve both channel ID and provider type from the same channel
list snapshot, then look up that channel's model and credentials. A provider type
such as `local-qwen3-mlx` must not bypass configured channels or their enabled
state. A nonempty list with every channel disabled has no selected channel and
must not fall back to an old preference. Legacy active values and preferences
remain supported when there is no channel configuration. Omni keeps its own
explicit selection.

The local model page reads credential status and listens for credential changes
to display the current provider. Local model preferences still remember the
model configuration. Switching away from a local channel does not delete its
downloaded model or credentials; enable it and move it first to use it again.

Restart reconciliation checks local selection metadata. It does not send a
provider validation request or automatically choose another enabled provider
after a network failure. An ongoing recording retains the channel identity,
protocol and model selected at the start of that recording.


## Use your ChatGPT plan for text polishing

Add a **ChatGPT Plan** LLM channel and choose **Continue with ChatGPT**. Complete
sign-in and plan-usage consent in the system browser, return to OpenLess, and
select a model from the account's catalog. Enable the channel and move it first
to use it for polishing and translation. Codex and an OpenAI API key are not
required. Speech recognition keeps its existing ASR provider.

Model discovery does not run inference. **Test connection** consumes a small
amount of ChatGPT plan usage. **Manage usage** opens ChatGPT's usage settings;
limits may apply to the plan or this app. OpenLess never switches a failed
ChatGPT Plan request to an API-key billing path automatically.

Each channel selects a saved account registration or creates a new one. Matching
email addresses do not merge registrations or workspaces. **Disconnect** stops
that registration's requests, attempts remote session revocation, and removes
its local tokens without changing channel order. If remote revocation cannot be
confirmed, disconnect the app in ChatGPT settings. Reauthorization reuses the
saved registration. OAuth tokens and host identifiers are local-only and are
excluded from provider synchronization, backup, and migration; a channel copied
to another device requires a new local sign-in.

On mobile, keep the authorization within the system's background-execution
window. If the process is terminated or the attempt expires, start sign-in again.
