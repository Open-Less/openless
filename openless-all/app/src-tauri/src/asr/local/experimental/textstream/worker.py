"""OpenLess-owned TextStream adapter: PCM in, Draft/Finalized snapshots out.

Streaming settings follow TextStream (199-biotechnologies/textstream-asr,
revision cacf67acadbd90789b631d9ee8f2e76f1a6cd7c2, MIT). Unlike its standalone
server, this module NEVER opens a microphone, downloads models, opens a browser
or writes into ~/Documents. OpenLess owns capture, history and process lifetime.
Only run() imports native inference libraries, after explicit recording starts.
"""

from __future__ import annotations

import argparse
import base64
import contextlib
import json
import math
import os
import queue
import re
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlparse

MAX_LINE = 1_000_000
MAX_AUDIO_BYTES = 32_000 * 600
CONTEXT_ENV = "OPENLESS_TEXTSTREAM_CONTEXT"
RECITED_PHRASES = 4
_CONTROL_MARKUP = str.maketrans("", "", "<>")


def hotword_phrases() -> list[str]:
    """Dictionary phrases OpenLess froze for this recording; empty when unused.

    OpenLess joins them as "A, B." and already bounds the text. Angle brackets
    would be tokenized as control tokens such as `<|audio_pad|>` and break the
    prompt, so they never reach the tokenizer.
    """
    text = os.environ.get(CONTEXT_ENV, "").strip().removesuffix(".")
    phrases = (
        " ".join(phrase.translate(_CONTROL_MARKUP).replace(",", " ").split())
        for phrase in text.split(", ")
    )
    return [phrase for phrase in phrases if phrase]



def recitation(phrases: list[str]) -> re.Pattern | None:
    """Match the context transcribed back instead of speech.

    For a window without speech the decoder writes out its system prompt: several
    dictionary phrases in a row, in dictionary order. Speech does not come out
    that way. Sentence starts are capitalised and separators may vary.
    """
    terms = [r"\s+".join(map(re.escape, phrase.split())) for phrase in phrases]
    runs = [
        r"[\s,，、]*".join(terms[start:end])
        for end in range(len(terms), RECITED_PHRASES - 1, -1)
        for start in range(end - RECITED_PHRASES + 1)
    ]
    if not runs:
        return None
    runs.sort(key=len, reverse=True)
    return re.compile(
        "(?<![0-9A-Za-z])(?:" + "|".join(runs) + r")(?![0-9A-Za-z])[。.]?\s*",
        re.IGNORECASE,
    )


def join_utterances(done: str, text: str) -> str:
    """Chinese and Japanese join directly; spaced scripts, Korean included, get a space."""
    if not done or not text:
        return done or text

    def spaceless(character: str) -> bool:
        code = ord(character)
        hangul = 0x3130 <= code <= 0x318F or 0xAC00 <= code <= 0xD7AF
        return code >= 0x2E80 and not hangul

    joiner = "" if spaceless(done[-1]) or spaceless(text[0]) else " "
    return done + joiner + text


def metal_memory_limits(memory_bytes: int, recommended_bytes: int) -> tuple[int, int]:
    """Reserve RAM for the host; MLX's memory limit is a guideline, not an OS quota."""
    if memory_bytes <= 0 or recommended_bytes <= 0:
        raise ValueError("Metal memory information is unavailable")
    # MLX defaults to 1.5x the recommended working set and retains freed buffers
    # up to that same limit. That is inappropriate beside a live recorder/UI.
    memory_limit = min(4 * 1024**3, memory_bytes // 3, recommended_bytes)
    cache_limit = min(128 * 1024**2, memory_limit // 16)
    return memory_limit, cache_limit


class CpuBudget:
    """Cooperative CPU-seconds budget; GPU wait is not charged as CPU work."""

    def __init__(self, cores: float) -> None:
        if not math.isfinite(cores) or not 0 < cores <= (os.cpu_count() or 1):
            raise ValueError("invalid CPU budget")
        self.cores = cores
        self.cpu_start = time.process_time()
        self.wall_start = time.monotonic()

    def checkpoint(self) -> None:
        """Yield between decoder steps without suspending OpenLess capture/UI."""
        debt = max(
            0.0,
            (time.process_time() - self.cpu_start) / self.cores
            - (time.monotonic() - self.wall_start),
        )
        resume = time.monotonic() + debt
        while True:
            remaining = resume - time.monotonic()
            if remaining <= 0:
                break
            time.sleep(min(remaining, 0.05))
        # Watchdog/SSE CPU during the wait must not create an endless debt loop.
        self.cpu_start = time.process_time()
        self.wall_start = time.monotonic()


def configure_cpu_threads() -> float:
    """Set native-library limits before NumPy/MLX/VAD are imported."""
    cores = float(os.environ.get("OPENLESS_CPU_BUDGET_CORES", "1"))
    if not math.isfinite(cores) or not 0 < cores <= (os.cpu_count() or 1):
        raise ValueError("invalid CPU budget")
    cores = min(cores, 2.0)
    threads = str(max(1, min(2, math.floor(cores))))
    for name in (
        "OMP_NUM_THREADS",
        "OPENBLAS_NUM_THREADS",
        "MKL_NUM_THREADS",
        "VECLIB_MAXIMUM_THREADS",
        "NUMEXPR_NUM_THREADS",
    ):
        os.environ[name] = threads
    os.environ["TOKENIZERS_PARALLELISM"] = "false"
    return cores


class Snapshots:
    """Whole snapshots, not append-only deltas: drafts can shrink or be replaced."""

    def __init__(self):
        self.finalized = ""
        self.draft = ""
        self.sequence = 0

    def update(self, finalized: str, full: str, terminal: bool = False) -> dict:
        # A decoder may revise its tail during accuracy-mode finalization. The
        # final result is authoritative; drafts are never silently promoted by us.
        if terminal:
            finalized = full
        if not full.startswith(finalized):
            # A decode may rewrite text that an earlier snapshot reported as
            # stable (script or normalisation flips). The protocol carries whole
            # previews, and the terminal snapshot is authoritative, so keep the
            # authoritative text and re-commit only the prefix both agree on
            # instead of failing the whole transcription.
            agreed = 0
            for previous, current in zip(finalized, full):
                if previous != current:
                    break
                agreed += 1
            finalized = finalized[:agreed]
        self.finalized, self.draft = finalized, full[len(finalized) :]
        self.sequence += 1
        return {
            "type": "final" if terminal else "stream",
            "sequence": self.sequence,
            "finalized": self.finalized,
            "draft": self.draft,
        }


class Broadcast:
    def __init__(self):
        self.lock = threading.Lock()
        self.clients: list[queue.Queue] = []
        self.latest = None
        self.closed = threading.Event()

    def publish(self, event):
        # Public SSE stays compatible with TextStream's type=stream snapshots.
        # The private stdout protocol additionally distinguishes terminal output.
        if event.get("type") == "final":
            event = {**event, "type": "stream", "done": True}
        with self.lock:
            self.latest = event
            for client in self.clients:
                try:
                    client.put_nowait(event)
                except queue.Full:
                    try:
                        client.get_nowait()
                    except queue.Empty:
                        pass
                    client.put_nowait(event)

    def subscribe(self):
        with self.lock:
            if len(self.clients) >= 16:
                return None
            client = queue.Queue(maxsize=1)
            self.clients.append(client)
            if self.latest is not None:
                client.put_nowait(self.latest)
            return client

    def unsubscribe(self, client):
        with self.lock:
            if client in self.clients:
                self.clients.remove(client)


def sse_server(events: Broadcast, port: int):
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            # Loopback bind + Host/Origin checks prevent LAN access and browser
            # DNS rebinding. CLI/local apps (no Origin) may subscribe explicitly.
            host = self.headers.get("Host", "").split(":")[0]
            origin = self.headers.get("Origin")
            if host not in ("localhost", "127.0.0.1") or (
                origin and urlparse(origin).hostname not in ("localhost", "127.0.0.1")
            ):
                self.send_error(403)
                return
            if self.path != "/stream":
                self.send_error(404)
                return
            client = events.subscribe()
            if client is None:
                self.send_error(429)
                return
            try:
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream; charset=utf-8")
                self.send_header("Cache-Control", "no-store")
                if origin:
                    self.send_header("Access-Control-Allow-Origin", origin)
                    self.send_header("Vary", "Origin")
                self.end_headers()
                while True:
                    try:
                        event = client.get(timeout=0.5)
                    except queue.Empty:
                        if events.closed.is_set():
                            break
                        continue
                    self.wfile.write(
                        (
                            "data: " + json.dumps(event, ensure_ascii=False) + "\n\n"
                        ).encode()
                    )
                    self.wfile.flush()
                    if event.get("done"):
                        break
            except (BrokenPipeError, ConnectionResetError, OSError):
                pass
            finally:
                events.unsubscribe(client)

        def log_message(self, *_args):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    server.daemon_threads = True
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server


def parent_watch(parent: int):
    while True:
        if os.getppid() != parent:
            os._exit(0)  # Parent crashed: never leave MLX/SSE running unattended.
        time.sleep(1)


def run(model_dir: Path, port: int, parent: int):
    if (
        not (model_dir / "model.safetensors").is_file()
        or not (model_dir / "config.json").is_file()
    ):
        raise ValueError("model files are missing; enable/download explicitly first")
    # Offline is mandatory even if the parent environment was misconfigured.
    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"
    os.environ["HF_HUB_DISABLE_TELEMETRY"] = "1"
    budget = CpuBudget(configure_cpu_threads())
    # This is the dedicated worker, never the GUI/audio process. Lowering its
    # priority lets foreground work win even during one indivisible native call.
    os.nice(10)
    threading.Thread(target=parent_watch, args=(parent,), daemon=True).start()
    events = Broadcast()
    server = sse_server(events, port)  # Fail a port conflict BEFORE native imports.
    output = sys.stdout

    def emit(event):
        output.write(json.dumps(event, ensure_ascii=False) + "\n")
        output.flush()
        if event["type"] in ("stream", "final"):
            events.publish(event)

    try:
        with contextlib.redirect_stdout(sys.stderr):
            import mlx.core as mx

            if not mx.metal.is_available():
                raise RuntimeError(
                    "TextStream requires Metal GPU; CPU fallback is disabled"
                )
            mx.set_default_device(mx.gpu)
            device = mx.metal.device_info()
            memory_limit, cache_limit = metal_memory_limits(
                int(device["memory_size"]),
                int(device["max_recommended_working_set_size"]),
            )
            mx.set_memory_limit(memory_limit)
            mx.set_cache_limit(cache_limit)
            import numpy as np
            from mlx_qwen3_asr import Session
            from silero_vad_lite import SileroVAD

            session = Session(model=str(model_dir))
            phrases = hotword_phrases()
            # Qwen3-ASR documents space-separated terms; a comma list invites the
            # decoder to continue it in the middle of ordinary speech.
            context = " ".join(phrases)
            recited = recitation(phrases)

            def new_stream():
                return session.init_streaming(
                    context=context,
                    chunk_size_sec=2.0,
                    max_context_sec=30.0,
                    sample_rate=16000,
                    unfixed_chunk_num=2,
                    unfixed_token_num=5,
                    language=None,
                    finalization_mode="accuracy",
                )

            state = new_stream()
            vad = SileroVAD(16000)
            mx.clear_cache()
        budget.checkpoint()
        emit({"type": "ready", "protocol": 1})
        snapshots = Snapshots()
        pending = bytearray()
        total = 0
        heard_speech = False
        done = ""  # Transcript from before the decoder was last restarted.

        def spoken(text: str) -> str:
            text = recited.sub("", text).strip() if recited else text
            return join_utterances(done, text)

        def decode(audio, stream):
            stream = session.feed_audio(audio, stream)
            if stream.language == "None":
                # With context, a chunk with little speech can be labelled "None";
                # kept, it is forced on every later window and joins them with
                # spaces.
                stream.language = "unknown"
            return stream

        def feed(raw):
            nonlocal state, heard_speech, done
            audio = np.frombuffer(raw, dtype="<i2").astype(np.float32) / 32768.0
            if not heard_speech:
                for start in range(0, len(audio) - 511, 512):
                    frame = np.ascontiguousarray(audio[start : start + 512])
                    if vad.process(memoryview(frame.data)) >= 0.4:
                        heard_speech = True
                        break
            # Once speech starts, keep silence for the decoder's own endpointing.
            if heard_speech:
                with contextlib.redirect_stdout(sys.stderr):
                    state = decode(audio, state)
                    if recited and recited.search(state.text):
                        # The recitation stays in the decoder's forced prefix and
                        # swallows the speech that follows. Keep what was said and
                        # decode this block once more on its own, in case speech
                        # begins in it; otherwise wait for speech as at the start.
                        done = spoken(state.text)
                        state = decode(audio, new_stream())
                        if recited.search(state.text):
                            state = new_stream()
                            heard_speech = False
                    # Release dead allocations as the rolling window grows;
                    # live model weights/KV state remain owned by the session.
                    mx.clear_cache()
                budget.checkpoint()
                emit(snapshots.update(spoken(state.stable_text), spoken(state.text)))

        while True:
            line = sys.stdin.buffer.readline(MAX_LINE + 1)
            if not line:
                return  # EOF/cancel is NOT a successful finalization.
            if len(line) > MAX_LINE:
                raise ValueError("oversized PCM request")
            request = json.loads(line)
            if request.get("type") == "audio":
                raw = base64.b64decode(request["pcm"], validate=True)
                total += len(raw)
                if len(raw) % 2 or total > MAX_AUDIO_BYTES:
                    raise ValueError("invalid PCM or recording exceeds ten minutes")
                pending.extend(raw)
                while len(pending) >= 64_000:
                    feed(bytes(pending[:64_000]))
                    del pending[:64_000]
            elif request.get("type") == "finish":
                if pending:
                    feed(bytes(pending))
                if heard_speech:
                    with contextlib.redirect_stdout(sys.stderr):
                        state = session.finish_streaming(state)
                        mx.clear_cache()
                    budget.checkpoint()
                    # finish_streaming's completed text, not our last draft.
                    emit(
                        snapshots.update(
                            spoken(state.stable_text), spoken(state.text), terminal=True
                        )
                    )
                else:
                    emit(snapshots.update("", done, terminal=True))
                return
            else:
                raise ValueError("unknown worker request")
    finally:
        events.closed.set()
        server.shutdown()
        deadline = time.monotonic() + 2
        while events.clients and time.monotonic() < deadline:
            time.sleep(0.01)
        server.server_close()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model-dir", required=True, type=Path)
    parser.add_argument("--parent", required=True, type=int)
    parser.add_argument("--port", type=int, default=7890)
    args = parser.parse_args()
    try:
        run(args.model_dir, args.port, args.parent)
    except Exception as error:  # noqa: BLE001
        # Native-library failures must cross IPC as structured protocol errors.
        print(json.dumps({"type": "error", "message": str(error)}), flush=True)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
