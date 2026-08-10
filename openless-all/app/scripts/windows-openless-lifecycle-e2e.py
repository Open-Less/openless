import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
import uuid
from pathlib import Path

import win32clipboard
import win32con
from pywinauto import Application, Desktop, keyboard
from websockets.sync.client import connect


def set_clipboard_text(text: str | None) -> None:
    if text is None:
        os.system("echo off | clip")
        return
    win32clipboard.OpenClipboard()
    try:
        win32clipboard.EmptyClipboard()
        win32clipboard.SetClipboardText(text)
    finally:
        win32clipboard.CloseClipboard()


def get_clipboard_text() -> str | None:
    try:
        win32clipboard.OpenClipboard()
        return win32clipboard.GetClipboardData(win32con.CF_UNICODETEXT)
    except Exception:
        return None
    finally:
        try:
            win32clipboard.CloseClipboard()
        except Exception:
            pass


class CdpClient:
    def __init__(self, websocket_url: str):
        self.ws = connect(websocket_url)
        self.next_id = 1
        self._send("Runtime.enable")

    def _send(self, method: str, params: dict | None = None) -> dict:
        msg_id = self.next_id
        self.next_id += 1
        payload = {"id": msg_id, "method": method}
        if params is not None:
            payload["params"] = params
        self.ws.send(json.dumps(payload))
        while True:
            message = json.loads(self.ws.recv())
            if message.get("id") == msg_id:
                return message

    def evaluate(self, expression: str):
        response = self._send(
            "Runtime.evaluate",
            {
                "expression": expression,
                "returnByValue": True,
                "awaitPromise": True,
            },
        )
        if "exceptionDetails" in response.get("result", {}):
            raise RuntimeError(json.dumps(response["result"]["exceptionDetails"], ensure_ascii=False))
        return response["result"]["result"].get("value")

    def invoke(self, command: str, args: dict | None = None):
        args_json = json.dumps(args or {}, ensure_ascii=False)
        expression = f"""
        (async () => {{
          const value = await window.__TAURI_INTERNALS__.invoke({json.dumps(command)}, {args_json});
          return JSON.stringify(value ?? null);
        }})()
        """
        raw = self.evaluate(expression)
        return json.loads(raw) if raw else None

    def close(self):
        self.ws.close()


def cdp_page_ws(port: int) -> str:
    deadline = time.time() + 20
    last_targets = []
    while time.time() < deadline:
        try:
            response = subprocess.run(
                [
                    "powershell",
                    "-NoProfile",
                    "-Command",
                    f"(Invoke-WebRequest -UseBasicParsing http://127.0.0.1:{port}/json/list).Content",
                ],
                capture_output=True,
                text=True,
                check=True,
            )
            targets = json.loads(response.stdout)
            last_targets = [target.get("url", "") for target in targets]
            for target in targets:
                url = target.get("url", "")
                if url.startswith("http://tauri.localhost") and "?window=" not in url:
                    return target["webSocketDebuggerUrl"]
        except Exception:
            pass
        time.sleep(0.5)
    raise RuntimeError(f"Main Tauri page target was not found. last_targets={last_targets}")


def speak_phrase(phrase: str) -> None:
    ps = f"""
Add-Type -AssemblyName System.Speech
$speaker = New-Object System.Speech.Synthesis.SpeechSynthesizer
$speaker.Rate = -1
$speaker.Volume = 100
$speaker.Speak(@'
{phrase}
'@)
"""
    subprocess.run(["powershell", "-NoProfile", "-Command", ps], check=True)


def wait_for_history_growth(client: CdpClient, baseline: int, timeout_seconds: int):
    deadline = time.time() + timeout_seconds
    while time.time() < deadline:
        history = client.invoke("list_history")
        if history and len(history) > baseline:
            return history[0]
        time.sleep(0.5)
    raise TimeoutError("History did not receive a new dictation session")


def configure_preferences(client: CdpClient) -> dict:
    prefs = client.invoke("get_settings")
    previous = json.loads(json.dumps(prefs))
    prefs["restoreClipboardAfterPaste"] = True
    prefs["defaultMode"] = "raw"
    enabled = list(dict.fromkeys((prefs.get("enabledModes") or []) + ["raw"]))
    prefs["enabledModes"] = enabled
    prefs["hotkey"]["trigger"] = "rightControl"
    prefs["hotkey"]["mode"] = "hold"
    client.invoke("set_settings", {"prefs": prefs})
    return previous


def focus_terminal_window(target: str):
    title = "C:\\WINDOWS\\system32\\cmd.exe" if target == "wt-cmd" else "Windows PowerShell"
    win = None
    for candidate in Desktop(backend="uia").windows():
        try:
            if candidate.class_name() == "CASCADIA_HOSTING_WINDOW_CLASS" and candidate.window_text() == title:
                win = candidate
                break
        except Exception:
            continue
    if win is None:
        raise RuntimeError(f"terminal window not found for title={title}")
    win.set_focus()
    time.sleep(0.5)
    keyboard.send_keys("{ESC}")
    time.sleep(0.1)
    return {"kind": "terminal", "title": title, "window": win}


def focus_notepad_window():
    fixture = Path(tempfile.gettempdir()) / f"openless-lifecycle-{uuid.uuid4().hex}.txt"
    fixture.write_text("", encoding="utf-8")
    app = Application(backend="uia").start(f"notepad.exe {fixture}")
    time.sleep(2.5)
    title = f"{fixture.name} - Notepad"
    win = Desktop(backend="uia").window(title=title)
    doc = next(d for d in win.descendants() if d.class_name() == "RichEditD2DPT")
    doc.set_focus()
    time.sleep(0.4)
    return {"kind": "notepad", "title": title, "window": win, "doc": doc, "app": app, "fixture": fixture}


def start_target(target: str):
    if target == "wt-cmd":
        subprocess.run(["wt.exe", "new-tab", "cmd.exe"], check=True)
        time.sleep(2.5)
        return focus_terminal_window(target)
    if target == "wt-powershell":
        subprocess.run(["wt.exe", "new-tab", "powershell.exe"], check=True)
        time.sleep(2.5)
        return focus_terminal_window(target)
    if target == "notepad":
        return focus_notepad_window()
    raise ValueError(target)


def read_target_text(target_info: dict) -> str:
    if target_info["kind"] == "terminal":
        for descendant in target_info["window"].descendants():
            if descendant.class_name() == "TermControl":
                return descendant.window_text()
        return ""
    return target_info["doc"].window_text()


def cleanup_target(target_info: dict):
    if target_info["kind"] == "notepad":
        try:
            target_info["app"].kill()
        except Exception:
            pass
        try:
            target_info["fixture"].unlink(missing_ok=True)
        except Exception:
            pass


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe-path", required=True)
    parser.add_argument("--target", choices=["notepad", "wt-cmd", "wt-powershell"], required=True)
    parser.add_argument("--phrase", default="openless terminal regression success")
    parser.add_argument("--injected-transcript-text", default="")
    parser.add_argument("--remote-debugging-port", type=int, default=9223)
    parser.add_argument("--timeout-seconds", type=int, default=120)
    args = parser.parse_args()

    debug_transcript_path = ""
    if args.injected_transcript_text.strip():
        debug_transcript_path = str(Path(tempfile.gettempdir()) / "openless-debug-transcript-e2e.txt")
        Path(debug_transcript_path).write_text(args.injected_transcript_text, encoding="utf-8")

    launch_ps = f"""
$env:OPENLESS_SHOW_MAIN_ON_START='1'
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port={args.remote_debugging_port}'
$env:OPENLESS_DEBUG_TRANSCRIPT_FILE='{debug_transcript_path}'
$proc = Start-Process -FilePath '{args.exe_path}' -PassThru
$proc.Id
"""
    app_process = subprocess.run(
        ["powershell", "-NoProfile", "-Command", launch_ps],
        check=True,
        capture_output=True,
        text=True,
    )
    app_pid = int(app_process.stdout.strip().splitlines()[-1])
    client = None
    target_info = None
    previous_settings = None
    previous_clipboard = get_clipboard_text()
    clipboard_sentinel = f"OPENLESS_OLD_CLIPBOARD_SENTINEL_{uuid.uuid4().hex}"

    try:
        time.sleep(5)
        client = CdpClient(cdp_page_ws(args.remote_debugging_port))
        previous_settings = configure_preferences(client)
        history = client.invoke("list_history") or []
        baseline_count = len(history)

        target_info = start_target(args.target)
        set_clipboard_text(clipboard_sentinel)

        client.invoke("start_dictation")
        time.sleep(1.0)
        if args.injected_transcript_text.strip():
            time.sleep(1.0)
        else:
            speak_phrase(args.phrase)
            time.sleep(0.8)
        client.invoke("stop_dictation")

        latest = wait_for_history_growth(client, baseline_count, args.timeout_seconds)
        target_text = read_target_text(target_info)

        result = {
            "target": args.target,
            "phrase": args.phrase,
            "historyFinalText": latest.get("finalText"),
            "historyRawTranscript": latest.get("rawTranscript"),
            "insertStatus": latest.get("insertStatus"),
            "targetContainsFinalText": bool(latest.get("finalText") and latest["finalText"] in target_text),
            "targetContainsClipboardSentinel": clipboard_sentinel in target_text,
            "targetTextTail": target_text[-400:],
        }
        print(json.dumps(result, ensure_ascii=False, indent=2))
    finally:
        if client and previous_settings is not None:
            try:
                client.invoke("set_settings", {"prefs": previous_settings})
            except Exception:
                pass
        if client:
            client.close()
        cleanup_target(target_info) if target_info else None
        set_clipboard_text(previous_clipboard)
        subprocess.run(
            ["powershell", "-NoProfile", "-Command", f"Stop-Process -Id {app_pid} -Force -ErrorAction SilentlyContinue"],
            check=False,
        )
        if debug_transcript_path:
            try:
                Path(debug_transcript_path).unlink(missing_ok=True)
            except Exception:
                pass


if __name__ == "__main__":
    main()
