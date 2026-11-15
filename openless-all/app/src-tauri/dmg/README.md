# Apple Silicon Installer Screen

[English](README.md) | [简体中文](README.zh.md)

The background is a photographic-style image generated for this OpenLess installer screen and contains no simulated application or folder icons. The composition is a light-coloured coast, early-morning daylight and central negative space, with the title, bilingual install instructions and arrow placed over the background; the real `OpenLess.app` and the `Applications` alias are shown by Finder, so native drag-to-install works.

- `installer-background@2x.png`: 1536 × 1024, the delivered asset for the generated image.
- `installer-background.tiff`: contains both representations, 768 × 512 (72 dpi) and 1536 × 1024 (144 dpi), used as the Finder Retina background.
- `layout.json`: real icon 128 pt, text 14 pt; window and icon positions are governed by `../tauri.macos-mlx.conf.json`.

Package it with `INSTALL=0 ./scripts/build-mac.sh`. The ARM build uses `scripts/macos-dmg-layout.py` to write the Finder layout before Tauri compresses and signs, without relying on Finder or AppleScript on CI. The final image must pass the helper's `verify` check; the image is only used for the installer disk and never enters the app runtime.

Design source: the two-icon install example the user provided on 2026-09-26 plus the new photographic-background requirement, produced with OpenAI's image generation tool. Future generations can follow the same direction — "light limestone coast, soft early-morning light, central space reserved for the real icons"; this note is a design summary, not the original verbatim prompt.
