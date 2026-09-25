import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import Clutter from 'gi://Clutter';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
/* SPDX-License-Identifier: AGPL-3.0-only
 * This body is shared by the GNOME 42–44 and 45+ entry points at packaging.
 */
const IFACE = `<node><interface name="org.openless.Desktop1">
<method name="Version"><arg direction="out" type="u"/></method>
<method name="Snapshot"><arg direction="out" type="s"/></method>
<method name="Bind"><arg direction="in" type="s"/><arg direction="out" type="s"/></method>
<method name="Restore"><arg direction="in" type="s"/><arg direction="out" type="b"/></method>
<method name="Place"><arg direction="in" type="s"/><arg direction="in" type="i"/><arg direction="in" type="i"/><arg direction="out" type="b"/></method>
<signal name="Hotkey"><arg type="s"/><arg type="u"/><arg type="u"/><arg type="b"/></signal>
</interface></node>`;

class DesktopBridge {
    constructor() {
        this.bindings = new Map(); this.held = new Map();
        this.object = Gio.DBusExportedObject.wrapJSObject(IFACE, this);
        this.object.export(Gio.DBus.session, '/org/openless/Desktop1');
        this.owner = Gio.bus_own_name_on_connection(Gio.DBus.session, 'org.openless.Desktop1', Gio.BusNameOwnerFlags.NONE, null, null);
        this.activation = global.display.connect('accelerator-activated', (_display, id) => {
            const binding = this.bindings.get(id);
            if (binding && !this.held.has(id)) { this.held.set(id, binding); this.emit(binding, true); }
        });
        this.release = global.stage.connect('captured-event', (_actor, event) => {
            if (event.type() === Clutter.EventType.KEY_RELEASE) {
                for (const [id, binding] of this.held) {
                    if (event.get_key_symbol() === binding.symbol) { this.held.delete(id); this.emit(binding, false); }
                }
            }
            return Clutter.EVENT_PROPAGATE;
        });
        // Older Mutter versions expose modifier state outside shell focus but
        // no accelerator-deactivated signal. It also releases a held modifier
        // after focus changes or a compositor modal operation.
        this.watch = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 20, () => {
            const state = global.get_pointer()[2];
            for (const [id, binding] of this.held) {
                let mask = binding.states;
                if ([0xffe3, 0xffe4].includes(binding.symbol)) mask |= 4;
                if ([0xffe1, 0xffe2].includes(binding.symbol)) mask |= 1;
                if ([0xffe9, 0xffea].includes(binding.symbol)) mask |= 8;
                if ([0xffeb, 0xffec].includes(binding.symbol)) mask |= 64;
                if (mask && (state & mask) !== mask) { this.held.delete(id); this.emit(binding, false); }
            }
            return GLib.SOURCE_CONTINUE;
        });
        try { this.deactivation = global.display.connect('accelerator-deactivated', (_d, id) => {
            const binding = this.held.get(id); if (binding) { this.held.delete(id); this.emit(binding, false); }
        }); } catch (_) { this.deactivation = 0; }
    }
    Version() { return 1; }
    windows() { return global.get_window_actors().map(actor => actor.meta_window); }
    Snapshot() {
        const window = global.display.focus_window;
        if (!window) return '{}';
        const rect = window.get_work_area_for_monitor(window.get_monitor());
        return JSON.stringify({version: 1, target: String(window.get_stable_sequence()), application: window.get_wm_class() || '',
            x: rect.x, y: rect.y, width: rect.width, height: rect.height, scale: 1});
    }
    emit(binding, pressed) {
        this.object.emit_signal('Hotkey', new GLib.Variant('(suub)', [binding.action, binding.symbol, binding.states, pressed]));
    }
    Bind(json) {
        let requested;
        try { requested = JSON.parse(json); if (!Array.isArray(requested) || requested.length > 64) throw new Error('Invalid bindings'); }
        catch (error) { return String(error); }
        const next = new Map(); const created = [];
        for (const binding of requested) {
            const existing = [...this.bindings].find(([, previous]) => previous.accelerator === binding.accelerator);
            const id = existing ? existing[0] : global.display.grab_accelerator(binding.accelerator, Meta.KeyBindingFlags.NONE);
            if (!id) { for (const newId of created) global.display.ungrab_accelerator(newId); return `Shortcut conflict: ${binding.accelerator}`; }
            if (!existing) created.push(id);
            next.set(id, binding);
        }
        for (const [id, binding] of this.held) this.emit(binding, false);
        this.held.clear();
        for (const id of this.bindings.keys()) if (!next.has(id)) global.display.ungrab_accelerator(id);
        for (const id of next.keys()) Main.wm.allowKeybinding(Meta.external_binding_name_for_action(id), Shell.ActionMode.ALL);
        this.bindings = next;
        return '';
    }
    Restore(target) {
        const window = this.windows().find(window => String(window.get_stable_sequence()) === target);
        if (!window) return false;
        window.activate(global.get_current_time()); return true;
    }
    Place(title, x, y) {
        if (!title.startsWith('OpenLess ')) return false;
        const window = this.windows().find(window => window.get_title() === title && (window.get_wm_class() || '').toLowerCase().includes('openless'));
        if (!window) return false;
        window.move_frame(true, x, y); return true;
    }
    destroy() {
        for (const binding of this.held.values()) this.emit(binding, false);
        for (const id of this.bindings.keys()) global.display.ungrab_accelerator(id);
        global.display.disconnect(this.activation); if (this.deactivation) global.display.disconnect(this.deactivation);
        global.stage.disconnect(this.release); GLib.Source.remove(this.watch);
        this.object.unexport(); Gio.bus_unown_name(this.owner);
    }
}

export default class OpenLessExtension { enable() { this.bridge = new DesktopBridge(); } disable() { this.bridge?.destroy(); this.bridge = null; } }
