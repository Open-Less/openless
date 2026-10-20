# Local ASR lifecycle

Model files, channel selection and a live inference engine are separate states.

**macOS:** the legacy Qwen/Whisper/Apple Speech implementation and model manager are retired. The disabled placeholder redirects users to the three opt-in [experimental models](experimental-local-asr.md). Old IDs are metadata compatibility only; they cannot start an engine or download old models. Windows/cloud routes are not retired.

| Action | Files / metadata | Inference |
| --- | --- | --- |
| Start/restart or open settings | Inspect files; migrate retired metadata; restore saved channel state | Unloaded, even when an experimental model was selected |
| Explicitly enable an experiment | Download and validate; install optional runtime components; register a channel | Not loaded; no inference-library imports during setup |
| Select/enable a downloaded channel | Metadata only; existing channel priorities are not overwritten by downloads | Deferred |
| Record or permitted explicit test | Require enabled/selected channel, installed files and current generation | Load only on this explicit action |
| Disable/delete a channel or switch away | Persist the choice; keep files | Release the corresponding engine/worker |
| Turn off an experiment | Cancel pending work; remove only its owned channel; keep files | Release |
| Explicitly delete its package | Require disabled experiment; remove its managed files after release | Never load merely to delete |

Persisted interrupted `downloadPending`/`installPending`/`deletePending` operations may resume because they represent a prior explicit action. `enabled=true` alone is not permission to download or reinstall missing files. Downloads/status/settings never invoke inference preparation.

TextStream's worker exists only during recording and receives PCM from OpenLess. Its independent Python/MLX environment is optional, hash locked and kept below the managed package. Finalized text, not the most recent draft, is committed. Cancellation, EOF, disabled leases and timeout cannot promote a draft. SenseVoice/Gemma are offline adapters and do not claim native streaming.

`LocalAsrActivationResult.preparedModel` remains for wire compatibility: it identifies a selected model, not a successful native load.

## Windows native setup exception: Foundry

The Foundry SDK's explicit download/setup path can initialize native components. It therefore sets a process-local restart-required flag, acquires/verifies the package without enabling a channel, writes a filesystem-only receipt, attempts unloading on success or failure, and blocks activation/inference until OpenLess restarts. UI restart guidance remains visible if deferred.

Catalog/status/cold deletion use receipts rather than starting the SDK manager. Existing installations without a receipt require one explicit Download/verify and restart; settings must not silently initialize the SDK for migration.

## Retirement safety

macOS startup removes only catalog-known legacy Qwen/Whisper installation/staging paths in configured/default managed roots. It keeps renamed channel identities as disabled entries. Root/model/parent symlinks must not grant deletion authority outside the managed tree. No macOS system speech assets, recordings, experimental packages or another application's files are removed.

## Verification

Run the Core, native, backend, frontend and browser checks documented in [experimental-local-asr.md](experimental-local-asr.md). Native acceptance has an explicit isolated debug-bundle entry point; it never uses the installed application's data, OS credential vault or microphone.

Checks govern OpenLess's own engine handles and child processes. They do not promise unrelated macOS/Windows system services disappear. Browser mocks and a debug bundle are not evidence of signed/notarized release acceptance or Windows hardware behavior.
