package com.openless.android;

import org.json.JSONArray;
import org.json.JSONObject;

import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Locale;

final class ProviderDiagnostics {
    private ProviderDiagnostics() {
    }

    static String validateLlm(SettingsStore.Settings settings) throws Exception {
        if (settings.llmBaseUrl.trim().isEmpty()) {
            throw new IllegalStateException("缺少 LLM Base URL。");
        }
        if (settings.llmApiKey.trim().isEmpty()) {
            throw new IllegalStateException("缺少 LLM API Key。");
        }
        if (settings.llmModel.trim().isEmpty()) {
            throw new IllegalStateException("缺少 LLM 模型。");
        }

        JSONObject body = new JSONObject();
        body.put("model", settings.llmModel);
        body.put("stream", false);
        body.put("temperature", 0);
        JSONArray messages = new JSONArray();
        messages.put(new JSONObject().put("role", "system").put("content", "Reply with exactly OK."));
        messages.put(new JSONObject().put("role", "user").put("content", "ping"));
        body.put("messages", messages);

        HttpURLConnection conn = openJsonPost(chatCompletionsUrl(settings.llmBaseUrl), settings.llmApiKey);
        try (OutputStream out = conn.getOutputStream()) {
            out.write(body.toString().getBytes(StandardCharsets.UTF_8));
        }
        String response = OpenLessHttp.readResponse(conn);
        if (conn.getResponseCode() < 200 || conn.getResponseCode() >= 300) {
            throw new IllegalStateException("LLM 请求失败 HTTP " + conn.getResponseCode() + "：" + OpenLessHttp.preview(response));
        }
        JSONObject json = new JSONObject(response);
        JSONArray choices = json.optJSONArray("choices");
        if (choices == null || choices.length() == 0) {
            throw new IllegalStateException("LLM 返回结果中没有 choices。");
        }
        String content = choices.getJSONObject(0).optJSONObject("message") == null
                ? ""
                : choices.getJSONObject(0).getJSONObject("message").optString("content", "").trim();
        return content.isEmpty() ? "LLM 检测通过。" : "LLM 检测通过：" + content;
    }

    static String validateAsr(SettingsStore.Settings settings) {
        if ("whisper".equals(settings.activeAsrProvider)) {
            if (settings.asrBaseUrl.trim().isEmpty()) {
                throw new IllegalStateException("缺少 ASR Base URL。");
            }
            if (settings.asrApiKey.trim().isEmpty()) {
                throw new IllegalStateException("缺少 ASR API Key。");
            }
            if (settings.asrModel.trim().isEmpty()) {
                throw new IllegalStateException("缺少 ASR 模型。");
            }
            return "Whisper 兼容 ASR 配置看起来完整，实际联调仍需要一段真实录音。";
        }
        if (settings.volcengineAppKey.trim().isEmpty()) {
            throw new IllegalStateException("缺少火山 ASR App Key。");
        }
        if (settings.volcengineAccessKey.trim().isEmpty()) {
            throw new IllegalStateException("缺少火山 ASR Access Key。");
        }
        if (settings.volcengineResourceId.trim().isEmpty()) {
            throw new IllegalStateException("缺少火山 ASR Resource ID。");
        }
        return "火山 ASR 配置看起来完整，实际联调仍需要一段真实录音。";
    }

    static List<String> listModels(String baseUrl, String apiKey) throws Exception {
        if (baseUrl.trim().isEmpty()) {
            throw new IllegalStateException("缺少 Base URL。");
        }
        if (apiKey.trim().isEmpty()) {
            throw new IllegalStateException("缺少 API Key。");
        }
        HttpURLConnection conn = openJsonGet(modelsUrl(baseUrl), apiKey);
        String response = OpenLessHttp.readResponse(conn);
        if (conn.getResponseCode() < 200 || conn.getResponseCode() >= 300) {
            int code = conn.getResponseCode();
            if (code == 404 || code == 405) {
                throw new IllegalStateException("当前提供商可能不支持模型列表接口。");
            }
            throw new IllegalStateException("模型列表请求失败 HTTP " + code + "：" + OpenLessHttp.preview(response));
        }
        JSONArray data = new JSONObject(response).optJSONArray("data");
        if (data == null) {
            throw new IllegalStateException("当前提供商没有返回可枚举的模型列表。");
        }
        ArrayList<String> models = new ArrayList<>();
        for (int i = 0; i < data.length(); i++) {
            JSONObject item = data.optJSONObject(i);
            if (item == null) {
                continue;
            }
            String id = item.optString("id", "").trim();
            if (!id.isEmpty() && !models.contains(id)) {
                models.add(id);
            }
        }
        Collections.sort(models);
        if (models.isEmpty()) {
            throw new IllegalStateException("当前提供商没有返回可用模型，或不支持模型列表接口。");
        }
        return models;
    }

    private static HttpURLConnection openJsonPost(String url, String apiKey) throws Exception {
        HttpURLConnection conn = (HttpURLConnection) new URL(url).openConnection();
        conn.setConnectTimeout(20000);
        conn.setReadTimeout(30000);
        conn.setRequestMethod("POST");
        conn.setDoOutput(true);
        conn.setRequestProperty("Authorization", "Bearer " + apiKey);
        conn.setRequestProperty("Content-Type", "application/json; charset=utf-8");
        return conn;
    }

    private static HttpURLConnection openJsonGet(String url, String apiKey) throws Exception {
        HttpURLConnection conn = (HttpURLConnection) new URL(url).openConnection();
        conn.setConnectTimeout(20000);
        conn.setReadTimeout(30000);
        conn.setRequestMethod("GET");
        conn.setRequestProperty("Authorization", "Bearer " + apiKey);
        conn.setRequestProperty("Accept", "application/json");
        return conn;
    }

    private static String chatCompletionsUrl(String base) {
        String trimmed = OpenLessHttp.trimSlash(base);
        String lower = trimmed.toLowerCase(Locale.ROOT);
        if (lower.endsWith("/chat/completions")) {
            return trimmed;
        }
        return trimmed + "/chat/completions";
    }

    private static String modelsUrl(String base) {
        String trimmed = OpenLessHttp.trimSlash(base);
        String lower = trimmed.toLowerCase(Locale.ROOT);
        if (lower.endsWith("/chat/completions")) {
            trimmed = trimmed.substring(0, trimmed.length() - "/chat/completions".length());
            lower = trimmed.toLowerCase(Locale.ROOT);
        }
        if (lower.endsWith("/models")) {
            return trimmed;
        }
        return trimmed + "/models";
    }
}
