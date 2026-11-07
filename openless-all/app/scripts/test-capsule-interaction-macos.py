#!/usr/bin/env python3
"""Click the production capsule in an isolated Tauri host and inert native underlay."""
import argparse
import ctypes as C
import json
import os
from pathlib import Path
import plistlib
import queue
import signal
import subprocess
import sys
import tempfile
import threading
import time

parser = argparse.ArgumentParser()
parser.add_argument('--fixture', required=True, type=Path)
parser.add_argument('--screen-index',type=int,default=0)
parser.add_argument('--mirror',action='store_true')
args = parser.parse_args()
if sys.platform != 'darwin':
    parser.error('this GUI acceptance runs on macOS')

cg = C.CDLL('/System/Library/Frameworks/ApplicationServices.framework/ApplicationServices')
cf = C.CDLL('/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation')
P = C.c_void_p
class Point(C.Structure):
    _fields_ = [('x', C.c_double), ('y', C.c_double)]
class Size(C.Structure):
    _fields_ = [('width', C.c_double), ('height', C.c_double)]
class Rect(C.Structure):
    _fields_ = [('origin', Point), ('size', Size)]
for name, inputs, output in [
    ('CGWindowListCopyWindowInfo', [C.c_uint32, C.c_uint32], P),
    ('CGRectMakeWithDictionaryRepresentation', [P, C.POINTER(Rect)], C.c_bool),
    ('CGEventCreateMouseEvent', [P, C.c_uint32, Point, C.c_uint32], P),
    ('CGEventPost', [C.c_uint32, P], None),
]:
    method = getattr(cg, name); method.argtypes = inputs; method.restype = output
for name, inputs, output in [
    ('CFArrayGetCount', [P], C.c_long), ('CFArrayGetValueAtIndex', [P, C.c_long], P),
    ('CFDictionaryGetValue', [P, P], P), ('CFStringCreateWithCString', [P, C.c_char_p, C.c_uint32], P),
    ('CFNumberGetValue', [P, C.c_int, P], C.c_bool), ('CFRelease', [P], None),
]:
    method = getattr(cf, name); method.argtypes = inputs; method.restype = output

def bounds(pid, width=None, point=None):
    keys = [cf.CFStringCreateWithCString(None, key, 0x08000100) for key in [b'kCGWindowOwnerPID', b'kCGWindowBounds']]
    windows = cg.CGWindowListCopyWindowInfo(1, 0)
    try:
        for index in range(cf.CFArrayGetCount(windows)):
            data = cf.CFArrayGetValueAtIndex(windows, index); owner = C.c_int()
            cf.CFNumberGetValue(cf.CFDictionaryGetValue(data, keys[0]), 3, C.byref(owner))
            if owner.value != pid: continue
            rect = Rect(); cg.CGRectMakeWithDictionaryRepresentation(cf.CFDictionaryGetValue(data, keys[1]), C.byref(rect))
            if (width is None or abs(rect.size.width - width) < 1) and (point is None or (rect.origin.x < point[0] < rect.origin.x + rect.size.width and rect.origin.y < point[1] < rect.origin.y + rect.size.height)):
                return rect.origin.x, rect.origin.y, rect.size.width, rect.size.height
    finally:
        cf.CFRelease(windows)
        for key in keys: cf.CFRelease(key)
    raise AssertionError(f'fixture window absent for PID {pid}')

def click(x, y):
    for kind in [5, 1, 2]:
        event = cg.CGEventCreateMouseEvent(None, kind, Point(x, y), 0)
        cg.CGEventPost(0, event); cf.CFRelease(event)
        time.sleep(.12)
    time.sleep(.15)

with tempfile.TemporaryDirectory(prefix='openless-capsule-acceptance-') as folder:
    root = Path(folder); counts = root / 'clicks'; ready = root / 'ready'; app = root / 'Underlay.app'; contents = app / 'Contents'; (contents / 'MacOS').mkdir(parents=True)
    (contents / 'Info.plist').write_bytes(plistlib.dumps({'CFBundleExecutable':'underlay','CFBundleIdentifier':f'com.openless.input-underlay.{os.getpid()}','CFBundleName':'OpenLess input test underlay','CFBundlePackageType':'APPL'}))
    swift = root / 'underlay.swift'
    swift.write_text('''import AppKit
var clickCount=0
final class Recorder:NSView {
 override func acceptsFirstMouse(for event:NSEvent?)->Bool {true}
 override func mouseDown(with event:NSEvent){clickCount+=1;try? String(clickCount).write(toFile:COUNT_PATH,atomically:true,encoding:.utf8)}
}
let app=NSApplication.shared
app.setActivationPolicy(.regular)
var windows:[NSWindow]=[]
for screen in NSScreen.screens {
 let w=NSWindow(contentRect:screen.frame.insetBy(dx:30,dy:30),styleMask:[.titled,.closable],backing:.buffered,defer:false)
 w.title="OpenLess isolated input underlay"
 w.level=NSWindow.Level(rawValue:29)
 w.collectionBehavior=[.canJoinAllSpaces,.fullScreenAuxiliary]
 w.contentView=Recorder(frame:NSRect(origin:.zero,size:screen.frame.insetBy(dx:30,dy:30).size))
 w.makeKeyAndOrderFront(nil)
 windows.append(w)
}
Timer.scheduledTimer(withTimeInterval:0.3,repeats:false){_ in
 app.activate(ignoringOtherApps:true)
 try? String(ProcessInfo.processInfo.processIdentifier).write(toFile:READY_PATH,atomically:true,encoding:.utf8)
}
Timer.scheduledTimer(withTimeInterval:60,repeats:false){_ in app.terminate(nil)}
app.run()
'''.replace('COUNT_PATH', json.dumps(str(counts))).replace('READY_PATH',json.dumps(str(ready))))
    subprocess.run(['swiftc', str(swift), '-o', str(contents/'MacOS/underlay')], check=True)
    subprocess.run(['codesign', '--force', '--deep', '--sign', '-', str(app)], check=True, capture_output=True)
    subprocess.run(['open','-a',str(app)],check=True)
    deadline=time.monotonic()+8
    while not ready.exists() and time.monotonic()<deadline:time.sleep(.05)
    assert ready.exists(),'native underlay did not launch'
    underlay_pid=int(ready.read_text())
    native = subprocess.Popen([str(args.fixture.resolve()),str(args.screen_index)]+(['--mirror'] if args.mirror else []), stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, bufsize=1)
    events = queue.Queue()
    def read():
        for line in native.stdout:
            events.put(line.strip())
    threading.Thread(target=read, daemon=True).start()
    def until(predicate, timeout=15):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try: line = events.get(timeout=min(.3, max(.01, deadline-time.monotonic())))
            except queue.Empty: continue
            print(line[:1200], flush=True)
            if line.startswith('SKIP screen'):raise SystemExit(77)
            result = predicate(line)
            if result: return result
        raise AssertionError('native fixture did not produce the expected action')
    def command(value): native.stdin.write(value+'\n'); native.stdin.flush()
    def geometry():
        command('geometry')
        return until(lambda line: json.loads(line[9:]) if line.startswith('GEOMETRY ') else None)
    def hit(node):
        x,y,_,_ = bounds(native.pid,460)
        click(x+node['left']+node['width']/2,y+node['top']+node['height']/2)
    def count(): return int(counts.read_text()) if counts.exists() else 0
    try:
        until(lambda line: line.startswith('REGIONS ') and len(json.loads(line[8:]))>=3)
        frame = geometry(); x,y,_,_ = bounds(native.pid,460)
        subprocess.run(['open','-a',str(app)],check=True);time.sleep(.3)
        ux,uy,uw,uh = bounds(underlay_pid,point=(x+25,y+20))
        print("WINDOWS",{"capsule":bounds(native.pid,460),"underlay":(ux,uy,uw,uh)},flush=True)
        assert ux < x+25 < ux+uw and uy < y+20 < uy+uh, 'underlay must cover the test point'
        click(x+25,y+20); assert count()==1, ('transparent space intercepted', count())
        click(x+230,y+140); assert count()==2, ('waveform intercepted',count())
        buttons = frame['buttons']
        hit(next(node for node in buttons if 'ol-default-confirm' in node['class']))
        until(lambda line: line=='ACTION confirm'); assert count()==2
        hit(next(node for node in buttons if 'ol-default-cancel' in node['class']))
        until(lambda line: line=='ACTION cancel'); assert count()==2
        hit(next(node for node in buttons if node['role']=='combobox'))
        until(lambda line: line=='MENU true')
        deadline=time.monotonic()+5
        choices=[]
        while not choices and time.monotonic()<deadline:
            frame=geometry()
            wx,wy,_,_=bounds(native.pid,460)
            choices=[node for node in frame['buttons'] if node['role']=='option' and node['height']>0 and not node['selected'] and ux < wx+node['left']+node['width']/2 < ux+uw and uy < wy+node['top']+node['height']/2 < uy+uh]
            if not choices:time.sleep(.05)
        assert choices, 'language menu must expose a visible alternative'
        hit(choices[0]); selected=until(lambda line: line if line.startswith('ACTION language=') else None)
        until(lambda line: line=='MENU false')
        assert count()==2, ('a control click reached the underlay',count())
        print('PRODUCTION_CAPSULE_NATIVE_ACCEPTANCE=PASS',selected,flush=True)
    finally:
        if native.poll() is None:
            command('quit')
            try: native.wait(timeout=5)
            except subprocess.TimeoutExpired: native.kill(); native.wait()
        try: os.kill(underlay_pid,signal.SIGTERM)
        except ProcessLookupError: pass
