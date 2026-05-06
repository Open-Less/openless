package com.openless.android;

import org.json.JSONArray;
import org.json.JSONObject;

import java.io.BufferedReader;
import java.io.InputStream;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;

final class OpenAiPolishProvider implements PolishProvider {
    interface QaDeltaListener {
        void onDelta(String partialText);
    }

    private final SettingsStore.Settings settings;

    OpenAiPolishProvider(SettingsStore.Settings settings) {
        this.settings = settings;
    }

    @Override
    public String polish(String raw, PolishMode mode, List<String> hotwords) throws Exception {
        if (mode == PolishMode.RAW) {
            return raw.trim();
        }
        return request(OpenLessPrompts.systemPrompt(mode, hotwords, settings.workingLanguages), raw);
    }

    @Override
    public String translate(String raw, String targetLanguage, List<String> hotwords,
                            List<String> workingLanguages) throws Exception {
        String target = targetLanguage == null ? "" : targetLanguage.trim();
        if (target.isEmpty()) {
            throw new IllegalStateException("缺少翻译目标语言。");
        }
        return request(OpenLessPrompts.translationSystemPrompt(target, hotwords, workingLanguages), raw);
    }

    String answerChat(List<QaChatMessage> history, List<String> workingLanguages) throws Exception {
        return answerChatStreaming(history, workingLanguages, null);
    }

    String answerChatStreaming(List<QaChatMessage> history, List<String> workingLanguages,
                               QaDeltaListener listener) throws Exception {
        if (settings.llmApiKey.trim().isEmpty()) {
            throw new IllegalStateException("缺少 LLM API Key。");
        }
        if (history == null || history.isEmpty()) {
            throw new IllegalStateException("问答历史为空。");
        }

        JSONObject body = new JSONObject();
        body.put("model", settings.llmModel);
        body.put("stream", listener != null);
        body.put("temperature", 0.3);
        JSONArray messages = new JSONArray();
        messages.put(new JSONObject()
                .put("role", "system")
                .put("content", OpenLessPrompts.qaSystemPrompt(workingLanguages)));
        for (QaChatMessage message : new ArrayList<>(history)) {
            if (message == null) {
                continue;
            }
            String content = message.content == null ? "" : message.content.trim();
            if (content.isEmpty()) {
                continue;
            }
            messages.put(new JSONObject()
                    .put("role", message.role == null ? "user" : message.role)
                    .put("content", content));
        }
        body.put("messages", messages);

        HttpURLConnection conn = (HttpURLConnection) new URL(chatCompletionsUrl(settings.llmBaseUrl)).openConnection();
        conn.setConnectTimeout(20000);
        conn.setReadTimeout(60000);
        conn.setRequestMethod("POST");
        conn.setDoOutput(true);
        conn.setRequestProperty("Authorization", "Bearer " + settings.llmApiKey);
        conn.setRequestProperty("Content-Type", "application/json; charset=utf-8");
        if (listener != null) {
            conn.setRequestProperty("Accept", "text/event-stream");
        }
        try (OutputStream out = conn.getOutputStream()) {
            out.write(body.toString().getBytes(StandardCharsets.UTF_8));
        }

        if (listener != null) {
            return parseStreamingTextResponse(conn, listener);
        }
        return parseTextResponse(conn);
    }

    private String request(String systemPrompt, String raw) throws Exception {
        if (settings.llmApiKey.trim().isEmpty()) {
            throw new IllegalStateException("缺少 LLM API Key。");
        }

        JSONObject body = new JSONObject();
        body.put("model", settings.llmModel);
        body.put("stream", false);
        body.put("temperature", 0.3);
        JSONArray messages = new JSONArray();
        messages.put(new JSONObject()
                .put("role", "system")
                .put("content", systemPrompt));
        messages.put(new JSONObject()
                .put("role", "user")
                .put("content", OpenLessPrompts.userPrompt(raw)));
        body.put("messages", messages);

        HttpURLConnection conn = (HttpURLConnection) new URL(chatCompletionsUrl(settings.llmBaseUrl)).openConnection();
        conn.setConnectTimeout(20000);
        conn.setReadTimeout(60000);
        conn.setRequestMethod("POST");
        conn.setDoOutput(true);
        conn.setRequestProperty("Authorization", "Bearer " + settings.llmApiKey);
        conn.setRequestProperty("Content-Type", "application/json; charset=utf-8");
        try (OutputStream out = conn.getOutputStream()) {
            out.write(body.toString().getBytes(StandardCharsets.UTF_8));
        }

        return parseTextResponse(conn);
    }

    private String parseTextResponse(HttpURLConnection conn) throws Exception {
        String response = OpenLessHttp.readResponse(conn);
        if (conn.getResponseCode() < 200 || conn.getResponseCode() >= 300) {
            throw new IllegalStateException("LLM 请求失败 HTTP " + conn.getResponseCode() + "：" + OpenLessHttp.preview(response));
        }
        JSONObject json = new JSONObject(response);
        JSONArray choices = json.optJSONArray("choices");
        if (choices == null || choices.length() == 0) {
            throw new IllegalStateException("LLM 返回结果中没有 choices。");
        }
        JSONObject message = choices.getJSONObject(0).optJSONObject("message");
        if (message == null) {
            throw new IllegalStateException("LLM 返回结果中没有 message。");
        }
        return message.optString("content", "").trim();
    }

    private String parseStreamingTextResponse(HttpURLConnection conn, QaDeltaListener listener) throws Exception {
        if (conn.getResponseCode() < 200 || conn.getResponseCode() >= 300) {
            String response = OpenLessHttp.readResponse(conn);
            throw new IllegalStateException("LLM 请求失败 HTTP " + conn.getResponseCode() + "：" + OpenLessHttp.preview(response));
        }

        InputStream stream = conn.getInputStream();
        if (stream == null) {
            return "";
        }

        StringBuilder full = new StringBuilder();
        try (BufferedReader reader = new BufferedReader(new InputStreamReader(stream, StandardCharsets.UTF_8))) {
            String line;
            while ((line = reader.readLine()) != null) {
                String trimmed = line.trim();
                if (!trimmed.startsWith("data:")) {
                    continue;
                }
                String payload = trimmed.substring(5).trim();
                if (payload.isEmpty()) {
                    continue;
                }
                if ("[DONE]".equals(payload)) {
                    break;
                }
                JSONObject json = new JSONObject(payload);
                JSONArray choices = json.optJSONArray("choices");
                if (choices == null || choices.length() == 0) {
                    continue;
                }
                JSONObject choice = choices.optJSONObject(0);
                if (choice == null) {
                    continue;
                }
                JSONObject delta = choice.optJSONObject("delta");
                if (delta == null) {
                    continue;
                }
                String piece = delta.optString("content", "");
                if (piece.isEmpty()) {
                    continue;
                }
                full.append(piece);
                listener.onDelta(full.toString());
            }
        }
        return full.toString().trim();
    }

    private static String chatCompletionsUrl(String base) {
        String trimmed = OpenLessHttp.trimSlash(base);
        String lower = trimmed.toLowerCase(Locale.ROOT);
        if (lower.endsWith("/chat/completions")) {
            return trimmed;
        }
        return trimmed + "/chat/completions";
    }
}
