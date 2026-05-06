package com.openless.android;

import org.json.JSONArray;
import org.json.JSONObject;

import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.UUID;

final class VolcengineStreamingSession implements AutoCloseable {
    private static final String ENDPOINT = "wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_async";
    private static final int CHUNK_BYTES = 6400;

    private final SettingsStore.Settings settings;
    private final List<String> hotwords;
    private final Object lock = new Object();
    private final ArrayList<Byte> pending = new ArrayList<>();
    private SimpleWebSocket ws;
    private Thread reader;
    private int nextSeq = 1;
    private String lastPartial = "";
    private RawTranscript finalTranscript;
    private Exception terminalError;
    private long startedAtMs;
    private volatile boolean connected;

    VolcengineStreamingSession(SettingsStore.Settings settings, List<String> hotwords) {
        this.settings = settings;
        this.hotwords = hotwords;
    }

    void open() throws Exception {
        if (settings.volcengineAppKey.trim().isEmpty()
                || settings.volcengineAccessKey.trim().isEmpty()
                || settings.volcengineResourceId.trim().isEmpty()) {
            throw new IllegalStateException("缺少火山 ASR 凭据。");
        }
        String connectId = UUID.randomUUID().toString();
        Map<String, String> headers = new LinkedHashMap<>();
        headers.put("X-Api-App-Key", settings.volcengineAppKey);
        headers.put("X-Api-Access-Key", settings.volcengineAccessKey);
        headers.put("X-Api-Resource-Id", settings.volcengineResourceId);
        headers.put("X-Api-Connect-Id", connectId);
        ws = new SimpleWebSocket(ENDPOINT, headers);
        connected = true;
        startedAtMs = System.currentTimeMillis();
        send(VolcengineFrameCodec.build(
                VolcengineFrameCodec.FULL_CLIENT_REQUEST,
                VolcengineFrameCodec.FLAG_POSITIVE_SEQUENCE,
                VolcengineFrameCodec.SERIALIZATION_JSON,
                firstFramePayload(connectId).toString().getBytes(StandardCharsets.UTF_8),
                nextSeq++));
        reader = new Thread(this::readLoop, "openless-volcengine-asr");
        reader.start();
    }

    void consume(byte[] pcm, int length) {
        if (!connected || length <= 0) {
            return;
        }
        List<byte[]> frames = new ArrayList<>();
        try {
            synchronized (lock) {
                for (int i = 0; i < length; i++) {
                    pending.add(pcm[i]);
                }
                while (pending.size() >= CHUNK_BYTES) {
                    byte[] chunk = takePending(CHUNK_BYTES);
                    frames.add(VolcengineFrameCodec.build(
                            VolcengineFrameCodec.AUDIO_ONLY_REQUEST,
                            VolcengineFrameCodec.FLAG_POSITIVE_SEQUENCE,
                            VolcengineFrameCodec.SERIALIZATION_NONE,
                            chunk,
                            nextSeq++));
                }
            }
            for (byte[] frame : frames) {
                send(frame);
            }
        } catch (Exception e) {
            fail(e);
        }
    }

    RawTranscript finish(long fallbackDurationMs) throws Exception {
        List<byte[]> frames = new ArrayList<>();
        synchronized (lock) {
            if (!pending.isEmpty()) {
                byte[] chunk = takePending(pending.size());
                frames.add(VolcengineFrameCodec.build(
                        VolcengineFrameCodec.AUDIO_ONLY_REQUEST,
                        VolcengineFrameCodec.FLAG_POSITIVE_SEQUENCE,
                        VolcengineFrameCodec.SERIALIZATION_NONE,
                        chunk,
                        nextSeq++));
            }
            frames.add(VolcengineFrameCodec.build(
                    VolcengineFrameCodec.AUDIO_ONLY_REQUEST,
                    VolcengineFrameCodec.FLAG_NEGATIVE_SEQUENCE,
                    VolcengineFrameCodec.SERIALIZATION_NONE,
                    new byte[0],
                    -nextSeq++));
        }
        for (byte[] frame : frames) {
            send(frame);
        }
        synchronized (lock) {
            long deadline = System.currentTimeMillis() + 12000;
            while (finalTranscript == null && terminalError == null && System.currentTimeMillis() < deadline) {
                lock.wait(Math.max(1, deadline - System.currentTimeMillis()));
            }
            connected = false;
            if (finalTranscript != null) {
                return finalTranscript;
            }
            if (!lastPartial.isEmpty()) {
                return new RawTranscript(lastPartial, fallbackDurationMs);
            }
            if (terminalError != null) {
                throw terminalError;
            }
            throw new IllegalStateException("火山 ASR 未返回最终结果。");
        }
    }

    @Override
    public void close() {
        connected = false;
        if (ws != null) {
            ws.close();
        }
    }

    private void readLoop() {
        try {
            while (connected) {
                byte[] data = ws.readBinary();
                VolcengineFrameCodec.Parsed parsed = VolcengineFrameCodec.parse(data);
                if (parsed == null) {
                    continue;
                }
                if (parsed.messageType == VolcengineFrameCodec.ERROR_MESSAGE) {
                    String body = new String(parsed.payload, StandardCharsets.UTF_8);
                    fail(new IllegalStateException("ASR 错误 " + parsed.errorCode + "：" + body));
                    return;
                }
                if (parsed.messageType != VolcengineFrameCodec.FULL_SERVER_RESPONSE) {
                    continue;
                }
                String text = VolcengineAsrProvider.extractText(new JSONObject(new String(parsed.payload, StandardCharsets.UTF_8)));
                synchronized (lock) {
                    if (!parsed.isFinal() && !text.isEmpty()) {
                        lastPartial = text;
                    }
                    if (parsed.isFinal()) {
                        finalTranscript = new RawTranscript(text, System.currentTimeMillis() - startedAtMs);
                        connected = false;
                        lock.notifyAll();
                        return;
                    }
                }
            }
        } catch (Exception e) {
            fail(e);
        }
    }

    private void fail(Exception e) {
        synchronized (lock) {
            terminalError = e;
            connected = false;
            lock.notifyAll();
        }
    }

    private void send(byte[] frame) throws Exception {
        SimpleWebSocket socket;
        synchronized (lock) {
            if (ws == null) {
                throw new IllegalStateException("WebSocket 尚未连接。");
            }
            socket = ws;
        }
        socket.sendBinary(frame);
    }

    private byte[] takePending(int size) {
        byte[] out = new byte[size];
        for (int i = 0; i < size; i++) {
            out[i] = pending.remove(0);
        }
        return out;
    }

    private JSONObject firstFramePayload(String connectId) throws Exception {
        JSONObject request = new JSONObject()
                .put("model_name", "bigmodel")
                .put("enable_itn", true)
                .put("enable_punc", true)
                .put("show_utterances", true);
        String context = VolcengineAsrProvider.hotwordContext(hotwords);
        if (context != null) {
            request.put("context", context);
        }
        return new JSONObject()
                .put("user", new JSONObject().put("uid", connectId))
                .put("audio", new JSONObject()
                        .put("format", "pcm")
                        .put("rate", 16000)
                        .put("bits", 16)
                        .put("channel", 1)
                        .put("codec", "raw"))
                .put("request", request);
    }
}
