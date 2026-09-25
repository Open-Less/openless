import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
const here=path.dirname(fileURLToPath(import.meta.url));
const body=fs.readFileSync(path.join(here,'bridge.js'),'utf8');
for(const [name,versions,imports,entry] of [
    ['legacy',['42','43','44'], 'const {Gio, GLib, Meta, Shell, Clutter} = imports.gi;\nconst Main = imports.ui.main;\n', 'let bridge;\nfunction init() {}\nfunction enable() { bridge = new DesktopBridge(); }\nfunction disable() { bridge?.destroy(); bridge = null; }\n'],
    ['modern',['45','46','47','48','49','50'], `import Gio from 'gi://Gio';\nimport GLib from 'gi://GLib';\nimport Meta from 'gi://Meta';\nimport Shell from 'gi://Shell';\nimport Clutter from 'gi://Clutter';\nimport * as Main from 'resource:///org/gnome/shell/ui/main.js';\n`, 'export default class OpenLessExtension { enable() { this.bridge = new DesktopBridge(); } disable() { this.bridge?.destroy(); this.bridge = null; } }\n'],
]) {
    const out=path.join(here,name);fs.mkdirSync(out,{recursive:true});
    fs.writeFileSync(path.join(out,'metadata.json'),JSON.stringify({uuid:'openless@openless.app',name:'OpenLess Desktop Bridge',description:'Native OpenLess hotkeys and window placement',version:1,'shell-version':versions},null,2)+'\n','utf8');
    fs.writeFileSync(path.join(out,'extension.js'),imports+body+'\n'+entry,'utf8');
}
