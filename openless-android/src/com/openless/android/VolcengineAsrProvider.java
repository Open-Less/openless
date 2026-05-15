package com.openless.android;

import org.json.JSONArray;
import org.json.JSONObject;

import java.nio.charset.StandardCharsets;
import java.net.SocketTimeoutException;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.UUID;

final class VolcengineAsrProvider implements AsrProvider {
    static final String DEFAULT_RESOURCE_ID = "volc.seedasr.sauc.duration";
    private static final String ENDPOINT = "wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_async";
    private static final int CHUNK_BYTES = 6400;

    private final SettingsStore.Settings settings;
    private final List<String> hotwords;

    VolcengineAsrProvider(SettingsStore.Settings settings, List<String> hotwords) {
        this.settings = settings;
        this.hotwords = hotwords;
    }

    @Override
    public RawTranscript transcribe(AudioRecorder.Recording recording) throws Exception {
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

        String lastPartial = "";
        try (SimpleWebSocket ws = new SimpleWebSocket(ENDPOINT, headers)) {
            int seq = 1;
            ws.sendBinary(VolcengineFrameCodec.build(
                    VolcengineFrameCodec.FULL_CLIENT_REQUEST,
                    VolcengineFrameCodec.FLAG_POSITIVE_SEQUENCE,
                    VolcengineFrameCodec.SERIALIZATION_JSON,
                    firstFramePayload(connectId).toString().getBytes(StandardCharsets.UTF_8),
                    seq++));

            for (int offset = 0; offset < recording.pcm.length; offset += CHUNK_BYTES) {
                int len = Math.min(CHUNK_BYTES, recording.pcm.length - offset);
                byte[] chunk = new byte[len];
                System.arraycopy(recording.pcm, offset, chunk, 0, len);
                ws.sendBinary(VolcengineFrameCodec.build(
                        VolcengineFrameCodec.AUDIO_ONLY_REQUEST,
                        VolcengineFrameCodec.FLAG_POSITIVE_SEQUENCE,
                        VolcengineFrameCodec.SERIALIZATION_NONE,
                        chunk,
                        seq++));
                lastPartial = drainAvailable(ws, lastPartial, false);
            }

            ws.sendBinary(VolcengineFrameCodec.build(
                    VolcengineFrameCodec.AUDIO_ONLY_REQUEST,
                    VolcengineFrameCodec.FLAG_NEGATIVE_SEQUENCE,
                    VolcengineFrameCodec.SERIALIZATION_NONE,
                    new byte[0],
                    -seq));

            long deadline = System.currentTimeMillis() + 12000;
            while (System.currentTimeMillis() < deadline) {
                String text = readResult(ws, true);
                if (text != null && !text.isEmpty()) {
                    return new RawTranscript(text, recording.durationMs);
                }
            }
        } catch (Exception e) {
            if (!lastPartial.isEmpty()) {
                return new RawTranscript(lastPartial, recording.durationMs);
            }
            throw e;
        }
        if (!lastPartial.isEmpty()) {
            return new RawTranscript(lastPartial, recording.durationMs);
        }
        throw new IllegalStateException("火山 ASR 未返回最终结果。");
    }

    private String drainAvailable(SimpleWebSocket ws, String fallback, boolean waitFinal) {
        if (waitFinal) {
            return fallback;
        }
        String latest = fallback;
        try {
            ws.setReadTimeoutMs(10);
            while (true) {
                String text = readResult(ws, false);
                if (text != null && !text.isEmpty()) {
                    latest = text;
                }
            }
        } catch (SocketTimeoutException ignored) {
            return latest;
        } catch (Exception e) {
            return latest;
        } finally {
            try {
                ws.setReadTimeoutMs(15000);
            } catch (Exception ignored) {
            }
        }
    }

    private String readResult(SimpleWebSocket ws, boolean requireFinal) throws Exception {
        byte[] data = ws.readBinary();
        VolcengineFrameCodec.Parsed parsed = VolcengineFrameCodec.parse(data);
        if (parsed == null) {
            return null;
        }
        if (parsed.messageType == VolcengineFrameCodec.ERROR_MESSAGE) {
            String body = new String(parsed.payload, StandardCharsets.UTF_8);
            throw new IllegalStateException("ASR 错误 " + parsed.errorCode + "：" + body);
        }
        if (parsed.messageType != VolcengineFrameCodec.FULL_SERVER_RESPONSE) {
            return null;
        }
        String text = extractText(new JSONObject(new String(parsed.payload, StandardCharsets.UTF_8)));
        if (requireFinal && !parsed.isFinal()) {
            return null;
        }
        return text;
    }

    private JSONObject firstFramePayload(String connectId) throws Exception {
        JSONObject request = new JSONObject()
                .put("model_name", "bigmodel")
                .put("enable_itn", true)
                .put("enable_punc", true)
                .put("show_utterances", true);
        String context = hotwordContext();
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

    private String hotwordContext() throws Exception {
        return hotwordContext(hotwords);
    }

    static String hotwordContext(List<String> hotwords) throws Exception {
        if (hotwords == null || hotwords.isEmpty()) {
            return null;
        }
        JSONArray array = new JSONArray();
        java.util.HashSet<String> seen = new java.util.HashSet<>();
        for (String word : hotwords) {
            String trimmed = word == null ? "" : word.trim();
            String key = trimmed.toLowerCase(java.util.Locale.ROOT);
            if (trimmed.isEmpty() || seen.contains(key)) {
                continue;
            }
            seen.add(key);
            array.put(new JSONObject().put("word", trimmed));
            if (array.length() >= 80) {
                break;
            }
        }
        return array.length() == 0 ? null : new JSONObject().put("hotwords", array).toString();
    }

    static String extractText(JSONObject json) throws Exception {
        JSONObject result = null;
        if (json.opt("result") instanceof JSONObject) {
            result = json.getJSONObject("result");
        } else if (json.opt("result") instanceof JSONArray) {
            JSONArray arr = json.getJSONArray("result");
            if (arr.length() > 0 && arr.opt(0) instanceof JSONObject) {
                result = arr.getJSONObject(0);
            }
        } else if (json.has("text")) {
            result = json;
        }
        if (result == null) {
            return "";
        }
        JSONArray utterances = result.optJSONArray("utterances");
        if (utterances != null && utterances.length() > 0) {
            StringBuilder builder = new StringBuilder();
            for (int i = 0; i < utterances.length(); i++) {
                builder.append(utterances.getJSONObject(i).optString("text", ""));
            }
            return builder.toString();
        }
        return result.optString("text", "");
    }
}
