# Component attribution

## Rare UI GitHub Activity

The Overview annual activity card adapts the calendar and expandable-panel code from Rare UI's GitHub Activity component. OpenLess uses local dictation activity and native desktop app icons instead of GitHub data. The card also supports a fitted calendar and an in-flow ranking panel.

- Page: https://rareui.com/components/githubactivity
- Upstream source: [github-activity.tsx](https://github.com/swamimalode07/rare-ui/blob/main/components/ui/github-activity.tsx)
- Copyright (c) 2026 Swami Malode
- License: [MIT + Commons Clause + Attribution](../openless-all/app/public/licenses/rare-ui-LICENSE.txt)

The adapted component lives in `openless-all/app/src/ui/controls/GitHubActivity.tsx`, with styles in `openless-all/app/src/ui/styles/github-activity.css`. The in-app credit is Settings → About and updates → Component credits.

## Rare UI Password Input

[Password Input](https://www.rareui.com/components/passwordinput) is the visual reference for OpenLess's shared input fields. `openless-all/app/src/ui/controls/PasswordEye.tsx` adapts the animated eye geometry from [password-input.tsx](https://github.com/swamimalode07/rare-ui/blob/main/components/ui/password-input.tsx) (Copyright (c) 2026 Swami Malode, [MIT + Commons Clause + Attribution](../openless-all/app/public/licenses/rare-ui-LICENSE.txt)). OpenLess uses native text rendering and masking, not the demo's per-character overlay or password strength meter. The credit appears in Settings → About and updates → Component credits.

## Rare UI Toast

The shared toast surface adapts Rare UI's [Toast](https://www.rareui.com/components/toast) panel, glyphs, and tile transitions from [toast.tsx](https://github.com/swamimalode07/rare-ui/blob/main/components/ui/toast.tsx) (Copyright (c) 2026 Swami Malode, [MIT + Commons Clause + Attribution](../openless-all/app/public/licenses/rare-ui-LICENSE.txt)). OpenLess uses its existing per-window notification controller and viewport instead of Sonner's global store. `useToast()` exposes non-modal variants and task updates; `confirm`, `warn`, and `openDialog` remain separate modal APIs. The credit appears in Settings → About and updates → Component credits.

## Rare UI AI Prompt Box and Streaming Response

[AI Prompt Box](https://www.rareui.com/components/promptbox) and
[Streaming Response](https://www.rareui.com/components/streamingresponse) are the
interaction references for OpenLess Ask Anything. The OpenLess implementation uses
its own native shortcut, voice, and streaming contracts rather than the demo's mock backend.

The blue Ask recording and sending strip also uses [Rare UI Voice Note](https://www.rareui.com/components/voicenote)
as its visual reference, with live microphone levels and elapsed time instead of the demo's audio player.
