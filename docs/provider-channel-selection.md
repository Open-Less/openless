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
