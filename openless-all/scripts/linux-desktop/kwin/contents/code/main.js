/* SPDX-License-Identifier: AGPL-3.0-only */
var bus = 'org.openless.Desktop1', path = '/org/openless/Desktop1';
var polling = false;
function windows() { return workspace.windowList ? workspace.windowList() : workspace.clientList(); }
function active() { return workspace.activeWindow !== undefined ? workspace.activeWindow : workspace.activeClient; }
function identity(window) { return String(window.internalId || window.windowId); }
function update() {
    var window = active();
    if (!window) return;
    var area = workspace.clientArea(KWin.MaximizeArea, window);
    callDBus(bus, path, bus, 'Update', JSON.stringify({version: 1, target: identity(window), application: String(window.resourceClass), x: area.x, y: area.y, width: area.width, height: area.height, scale: 1}));
}
function next() {
    if (polling) return;
    polling = true;
    callDBus(bus, path, bus, 'Next', function (json) {
        polling = false;
        // A missing helper must not turn a failed D-Bus call into a busy loop.
        // A subsequent focus/screen event starts the connection again.
        if (typeof json !== 'string') return;
        var command, success = false;
        try {
            command = JSON.parse(json || '{}');
            if (command.op === 'restore') {
                var target = windows().filter(function (window) { return identity(window) === command.target; })[0];
                if (target) { if (workspace.activeWindow !== undefined) workspace.activeWindow = target; else workspace.activeClient = target; success = true; }
            } else if (command.op === 'place' && command.title.indexOf('OpenLess ') === 0) {
                var popup = windows().filter(function (window) { return window.caption === command.title && String(window.resourceClass).toLowerCase().indexOf('openless') >= 0; })[0];
                if (popup) { var rect = popup.frameGeometry; rect.x = command.x; rect.y = command.y; popup.frameGeometry = rect; success = true; }
            }
        } catch (error) { print('OpenLess desktop bridge: ' + error); }
        if (command && command.id) callDBus(bus, path, bus, 'Ack', command.id, success);
        update(); next();
    });
}
function changed() { update(); next(); }
if (workspace.windowActivated) workspace.windowActivated.connect(changed);
else workspace.clientActivated.connect(changed);
if (workspace.screensChanged) workspace.screensChanged.connect(changed);
update(); next();
