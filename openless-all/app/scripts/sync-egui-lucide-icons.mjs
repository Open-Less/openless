// Reproduce the Linux interface icons from the exact lucide-react dependency
// used by the Tauri shell. Run with --check to verify checked-in SVG assets.
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import * as lucide from 'lucide-react';
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { resolve } from 'node:path';

const root = resolve(fileURLToPath(new URL('..', import.meta.url)));
const dest = resolve(root, 'linux-egui/assets/lucide');
// Names match src/components/Icon.tsx. Stop/Send/Pin/Chat are popup
// controls and use the corresponding Lucide shape from the same version.
export const icons = {
  Overview: 'ChartNoAxesColumn', History: 'History', Vocab: 'BookOpenText',
  Style: 'SlidersHorizontal', SelectionAsk: 'MessageSquareText', Settings: 'Settings',
  Mic: 'Mic', Sparkle: 'Sparkles', Hash: 'Hash', Clock: 'Clock3', Bolt: 'Zap',
  Copy: 'Copy', Search: 'Search', Trash: 'Trash2', Refresh: 'RefreshCw',
  Download: 'Download', Upload: 'Upload', Plus: 'Plus', Play: 'Play',
  Stop: 'Square', Close: 'X', Check: 'Check', Send: 'ArrowUp', Pin: 'Pin',
  Chat: 'MessageSquare', More: 'Ellipsis', ChevronRight: 'ChevronRight',
  Feather: 'Feather', Layout: 'PanelLeft', Doc: 'FileText', Pencil: 'Pencil',
  Cloud: 'Cloud', Shield: 'ShieldCheck', Info: 'Info', Help: 'CircleHelp',
  External: 'ExternalLink', Monitor: 'Monitor',
};
const version = JSON.parse(readFileSync(resolve(root, 'node_modules/lucide-react/package.json'), 'utf8')).version;
const check = process.argv.includes('--check');
if (!check) mkdirSync(dest, { recursive: true });
for (const [name, component] of Object.entries(icons)) {
  if (!lucide[component]) throw new Error(`Unknown Lucide icon ${component}`);
  // White masks are tinted at draw time by egui. The shape, stroke width,
  // caps, joins and viewBox are otherwise the same as Tauri's Icon component.
  const svg = `${renderToStaticMarkup(createElement(lucide[component], { size: 24, color: '#ffffff', strokeWidth: 1.75 }))}\n`;
  const path = resolve(dest, `${name}.svg`);
  if (check) {
    if (readFileSync(path, 'utf8') !== svg) throw new Error(`${name}.svg differs from lucide-react ${version}`);
  } else {
    writeFileSync(path, svg);
  }
}
console.log(`${Object.keys(icons).length} Linux icons match lucide-react ${version}`);
