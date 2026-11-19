# Component attribution

## Rare UI GitHub Activity

The overview yearly activity card is a port of Rare UI's GitHub Activity component.

- Page: https://rareui.com/components/githubactivity
- Upstream: https://github.com/swamimalode07/rare-ui
- License: MIT License + Commons Clause
- Copyright (c) 2026 Swami Malode

The port keeps the original geometry: 11px cells, 3px gap, 28px corner radius, 16px padding, week and month layout, the hover tooltip, and the expandable bottom panel. Local dictation counts replace GitHub contributions. The software with the most voice input (name and count) replaces repositories. Cells are not redrawn or resized to fit a narrow column.

The in-app credit is Settings → About and updates → Component credits. The repository READMEs (`openless-all/README.md` and `openless-all/README.zh.md`) carry the same credit.

## Rare UI Prompt Box

The standalone Ask window adapts Rare UI's [Prompt Box demo](https://rareui.com/components/promptbox). OpenLess retains its layout and animations, adds a small attachment slot, and replaces scripted responses with the configured provider's responses. The upstream files in `openless-all/app/third_party/rare-ui` come from [`33f7ce51`](https://github.com/swamimalode07/rare-ui/tree/33f7ce51e5a41fb7caef66b7aad0812b5bb2ab17). They retain their upstream license and are included as part of the app, not offered as a component library.

- Rare UI code: Copyright (c) 2026 Swami Malode, [MIT + Commons Clause + Attribution](../openless-all/app/public/licenses/rare-ui-LICENSE.txt).
- Open Runde fonts: [SIL Open Font License 1.1](../openless-all/app/public/licenses/open-runde-OFL.txt).
