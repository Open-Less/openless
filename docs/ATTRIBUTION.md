# Component attribution

## Rare UI GitHub Activity

The Overview annual activity card adapts the calendar and expandable-panel code from Rare UI's GitHub Activity component. OpenLess uses local dictation activity and native desktop app icons instead of GitHub data. The card also supports a fitted calendar and an in-flow ranking panel.

- Upstream source: [github-activity.tsx](https://github.com/swamimalode07/rare-ui/blob/main/components/ui/github-activity.tsx)
- Copyright (c) 2026 Swami Malode
- License: [MIT + Commons Clause + Attribution](../openless-all/app/public/licenses/rare-ui-LICENSE.txt)

The adapted component lives in `openless-all/app/src/ui/controls/GitHubActivity.tsx`, with styles in `openless-all/app/src/ui/styles/github-activity.css`. The in-app credit is Settings → About and updates → Component credits.

## Rare UI Password Input

Rare UI Password Input is the visual reference for OpenLess's shared input fields. `openless-all/app/src/ui/controls/PasswordEye.tsx` adapts the animated eye geometry from [password-input.tsx](https://github.com/swamimalode07/rare-ui/blob/main/components/ui/password-input.tsx) (Copyright (c) 2026 Swami Malode, [MIT + Commons Clause + Attribution](../openless-all/app/public/licenses/rare-ui-LICENSE.txt)). OpenLess uses native text rendering and masking, not the demo's per-character overlay or password strength meter. The credit appears in Settings → About and updates → Component credits.

## Rare UI Toast

The shared toast surface adapts Rare UI's Toast panel, glyphs, and tile transitions from [toast.tsx](https://github.com/swamimalode07/rare-ui/blob/main/components/ui/toast.tsx) (Copyright (c) 2026 Swami Malode, [MIT + Commons Clause + Attribution](../openless-all/app/public/licenses/rare-ui-LICENSE.txt)). OpenLess uses its existing per-window notification controller and viewport instead of Sonner's global store. `useToast()` exposes non-modal variants and task updates; `confirm`, `warn`, and `openDialog` remain separate modal APIs. The credit appears in Settings → About and updates → Component credits.

## Rare UI AI Prompt Box and Streaming Response

[AI Prompt Box](https://github.com/swamimalode07/rare-ui/blob/main/components/ui/prompt-box.tsx) and
[Streaming Response](https://github.com/swamimalode07/rare-ui/blob/main/components/ui/streaming-response.tsx) are the
interaction references for OpenLess Ask Anything. The OpenLess implementation uses
its own native shortcut, voice, and streaming contracts rather than the demo's mock backend.

The upstream Prompt Box files in `openless-all/app/third_party/rare-ui` come from
[`33f7ce51`](https://github.com/swamimalode07/rare-ui/tree/33f7ce51e5a41fb7caef66b7aad0812b5bb2ab17)
and are included as part of the app, not offered as a component library.

- Rare UI code: Copyright (c) 2026 Swami Malode, [MIT + Commons Clause + Attribution](../openless-all/app/public/licenses/rare-ui-LICENSE.txt).
- Open Runde fonts: [SIL Open Font License 1.1](../openless-all/app/public/licenses/open-runde-OFL.txt).
