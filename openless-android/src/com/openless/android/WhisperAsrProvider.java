package com.openless.android;

import org.json.JSONObject;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.util.UUID;

final class WhisperAsrProvider implements AsrProvider {
    private final SettingsStore.Settings settings;

    WhisperAsrProvider(SettingsStore.Settings settings) {
        this.settings = settings;
    }

    @Override
    public RawTranscript transcribe(AudioRecorder.Recording recording) throws Exception {
        if (settings.asrApiKey.trim().isEmpty()) {
            throw new IllegalStateException("缺少 ASR API Key。");
        }
        byte[] wav = WavEncoder.encode16kMono(recording.pcm);
        String boundary = "openless-" + UUID.randomUUID();
        String endpoint = OpenLessHttp.trimSlash(settings.asrBaseUrl) + "/audio/transcriptions";

        HttpURLConnection conn = (HttpURLConnection) new URL(endpoint).openConnection();
        conn.setConnectTimeout(20000);
        conn.setReadTimeout(60000);
        conn.setRequestMethod("POST");
        conn.setDoOutput(true);
        conn.setRequestProperty("Authorization", "Bearer " + settings.asrApiKey);
        conn.setRequestProperty("Content-Type", "multipart/form-data; boundary=" + boundary);

        try (OutputStream out = conn.getOutputStream()) {
            OpenLessHttp.writeUtf8(out, "--" + boundary + "\r\n");
            OpenLessHttp.writeUtf8(out, "Content-Disposition: form-data; name=\"model\"\r\n\r\n");
            OpenLessHttp.writeUtf8(out, settings.asrModel + "\r\n");
            OpenLessHttp.writeUtf8(out, "--" + boundary + "\r\n");
            OpenLessHttp.writeUtf8(out, "Content-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\n");
            OpenLessHttp.writeUtf8(out, "Content-Type: audio/wav\r\n\r\n");
            out.write(wav);
            OpenLessHttp.writeUtf8(out, "\r\n--" + boundary + "--\r\n");
        }

        String body = OpenLessHttp.readResponse(conn);
        if (conn.getResponseCode() < 200 || conn.getResponseCode() >= 300) {
            throw new IllegalStateException("ASR 请求失败 HTTP " + conn.getResponseCode() + "：" + OpenLessHttp.preview(body));
        }
        return new RawTranscript(new JSONObject(body).optString("text", ""), recording.durationMs);
    }
}
