package com.openless.android;

import java.util.ArrayList;
import java.util.List;

final class QaSessionStore {
    private static final QaSessionStore INSTANCE = new QaSessionStore();

    private final ArrayList<QaChatMessage> messages = new ArrayList<>();
    private String contextText = "";

    private QaSessionStore() {
    }

    static QaSessionStore get() {
        return INSTANCE;
    }

    synchronized List<QaChatMessage> messages() {
        return new ArrayList<>(messages);
    }

    synchronized void addUser(String content) {
        messages.add(new QaChatMessage("user", content));
    }

    synchronized void addAssistant(String content) {
        messages.add(new QaChatMessage("assistant", content));
    }

    synchronized void addAssistantPlaceholder() {
        messages.add(new QaChatMessage("assistant", ""));
    }

    synchronized void replaceLastAssistant(String content) {
        if (!messages.isEmpty()) {
            QaChatMessage last = messages.get(messages.size() - 1);
            if ("assistant".equals(last.role)) {
                messages.set(messages.size() - 1, new QaChatMessage("assistant", content));
                return;
            }
        }
        messages.add(new QaChatMessage("assistant", content));
    }

    synchronized void removeLastAssistantIfEmpty() {
        if (messages.isEmpty()) {
            return;
        }
        QaChatMessage last = messages.get(messages.size() - 1);
        if ("assistant".equals(last.role) && (last.content == null || last.content.trim().isEmpty())) {
            messages.remove(messages.size() - 1);
        }
    }

    synchronized void setContextText(String value) {
        contextText = value == null ? "" : value.trim();
    }

    synchronized void startNewContext(String value) {
        messages.clear();
        contextText = value == null ? "" : value.trim();
    }

    synchronized String contextText() {
        return contextText;
    }

    synchronized void clear() {
        messages.clear();
        contextText = "";
    }
}
