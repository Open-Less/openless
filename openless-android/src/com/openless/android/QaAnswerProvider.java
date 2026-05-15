package com.openless.android;

import java.util.List;

final class QaAnswerProvider {
    interface StreamingListener {
        void onDelta(String partialText);
    }

    private final SettingsStore settingsStore;

    QaAnswerProvider(SettingsStore settingsStore) {
        this.settingsStore = settingsStore;
    }

    String answer(List<QaChatMessage> messages) throws Exception {
        SettingsStore.Settings settings = settingsStore.get();
        return new OpenAiPolishProvider(settings).answerChat(messages, settings.workingLanguages);
    }

    String answerStreaming(List<QaChatMessage> messages, StreamingListener listener) throws Exception {
        SettingsStore.Settings settings = settingsStore.get();
        return new OpenAiPolishProvider(settings).answerChatStreaming(messages, settings.workingLanguages,
                listener == null ? null : listener::onDelta);
    }
}
