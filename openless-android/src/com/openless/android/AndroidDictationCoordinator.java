package com.openless.android;

import android.Manifest;
import android.content.Context;
import android.content.pm.PackageManager;
import android.os.Handler;
import android.os.Looper;

import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

final class AndroidDictationCoordinator {
    interface Listener {
        void onCapsuleState(CapsuleState state, String message);

        void onRecordingLevel(float level);

        void onToast(String message);
    }

    private final Context context;
    private final SettingsStore settingsStore;
    private final HistoryStore historyStore;
    private final DictionaryStore dictionaryStore;
    private final AudioRecorder recorder = new AudioRecorder();
    private final TextInserter inserter;
    private final ExecutorService executor = Executors.newSingleThreadExecutor();
    private final Handler main = new Handler(Looper.getMainLooper());
    private final Listener listener;
    private int phase = DictationPhase.IDLE;
    private long sessionId;
    private VolcengineStreamingSession volcengineSession;
    private boolean cancelled;
    private boolean translateNext;

    AndroidDictationCoordinator(Context context, SettingsStore settingsStore, HistoryStore historyStore, Listener listener) {
        this.context = context.getApplicationContext();
        this.settingsStore = settingsStore;
        this.historyStore = historyStore;
        this.dictionaryStore = new DictionaryStore(context);
        this.listener = listener;
        this.inserter = new TextInserter(context);
    }

    synchronized void toggle() {
        if (phase == DictationPhase.IDLE) {
            beginSession(false);
        } else if (phase == DictationPhase.LISTENING) {
            endSession();
        } else {
            emitToast("OpenLess 正忙，请稍候。");
        }
    }

    synchronized void startTranslation() {
        if (phase != DictationPhase.IDLE) {
            emitToast("OpenLess 正忙，请稍候。");
            return;
        }
        String target = settingsStore.get().translationTargetLanguage == null
                ? ""
                : settingsStore.get().translationTargetLanguage.trim();
        if (target.isEmpty()) {
            emitToast("请先在设置里填写翻译目标语言。");
            return;
        }
        beginSession(true);
    }

    synchronized void cancel() {
        if (phase == DictationPhase.LISTENING) {
            recorder.stop();
        }
        if (volcengineSession != null) {
            volcengineSession.close();
            volcengineSession = null;
        }
        cancelled = true;
        translateNext = false;
        phase = DictationPhase.IDLE;
        sessionId++;
        emit(CapsuleState.CANCELLED, null);
    }

    void shutdown() {
        cancel();
        executor.shutdownNow();
    }

    private void beginSession(boolean translate) {
        if (context.checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            emitToast("请先在 OpenLess 中授予麦克风权限。");
            return;
        }
        phase = DictationPhase.STARTING;
        cancelled = false;
        translateNext = translate;
        long currentSession = ++sessionId;
        emit(CapsuleState.STARTING, null);
        try {
            SettingsStore.Settings settings = settingsStore.get();
            if (!"whisper".equals(settings.activeAsrProvider)) {
                java.util.List<String> hotwords = dictionaryStore.enabledPhrases();
                volcengineSession = new VolcengineStreamingSession(settings, hotwords);
                volcengineSession.open();
                recorder.start((pcm, length) -> {
                    VolcengineStreamingSession session = volcengineSession;
                    if (session != null) {
                        session.consume(pcm, length);
                    }
                }, listener::onRecordingLevel);
            } else {
                recorder.start(null, listener::onRecordingLevel);
            }
            synchronized (this) {
                if (currentSession != sessionId) {
                    return;
                }
                phase = DictationPhase.LISTENING;
            }
            emit(CapsuleState.RECORDING, null);
        } catch (Exception e) {
            fail(currentSession, "录音失败：" + e.getMessage());
        }
    }

    private void endSession() {
        phase = DictationPhase.PROCESSING;
        long currentSession = sessionId;
        AudioRecorder.Recording recording = recorder.stop();
        if (recording.pcm.length < 1000) {
            phase = DictationPhase.IDLE;
            emit(CapsuleState.ERROR, "录音过短");
            return;
        }
        emit(CapsuleState.TRANSCRIBING, null);
        VolcengineStreamingSession session = volcengineSession;
        volcengineSession = null;
        if (session != null) {
            executor.execute(() -> finishVolcengine(currentSession, session, recording.durationMs));
        } else {
            executor.execute(() -> processRecording(currentSession, recording));
        }
    }

    private void finishVolcengine(long currentSession, VolcengineStreamingSession session, long fallbackDurationMs) {
        try {
            RawTranscript raw = session.finish(fallbackDurationMs);
            session.close();
            processTranscript(currentSession, raw);
        } catch (Exception e) {
            session.close();
            main.post(() -> fail(currentSession, e.getMessage()));
        }
    }

    private void processRecording(long currentSession, AudioRecorder.Recording recording) {
        try {
            SettingsStore.Settings settings = settingsStore.get();
            java.util.List<String> hotwords = dictionaryStore.enabledPhrases();
            AsrProvider asr = new WhisperAsrProvider(settings);

            RawTranscript raw = asr.transcribe(recording);
            synchronized (this) {
                if (cancelled || currentSession != sessionId) {
                    return;
                }
            }
            processTranscript(currentSession, raw);
        } catch (Exception e) {
            main.post(() -> fail(currentSession, e.getMessage()));
        }
    }

    private void processTranscript(long currentSession, RawTranscript raw) throws Exception {
        if (raw.text.trim().isEmpty()) {
            throw new IllegalStateException("ASR 返回了空白转写结果。");
        }
        synchronized (this) {
            if (cancelled || currentSession != sessionId) {
                return;
            }
        }
        SettingsStore.Settings settings = settingsStore.get();
        java.util.List<String> hotwords = dictionaryStore.enabledPhrases();
        PolishProvider polish = new OpenAiPolishProvider(settings);
        boolean translating;
        synchronized (this) {
            translating = translateNext;
        }
        postState(translating ? CapsuleState.TRANSLATING : CapsuleState.POLISHING, null);
        String computedText;
        String computedErrorCode = null;
        if (translating) {
            try {
                computedText = polish.translate(raw.text, settings.translationTargetLanguage, hotwords, settings.workingLanguages);
            } catch (Exception e) {
                computedText = raw.text;
                computedErrorCode = "translation_failed";
            }
        } else {
            computedText = polish.polish(raw.text, settings.mode, hotwords);
        }
        final String finalText = computedText;
        final String errorCode = computedErrorCode;
        final String translationTarget = settings.translationTargetLanguage == null
                ? ""
                : settings.translationTargetLanguage.trim();
        synchronized (this) {
            if (cancelled || currentSession != sessionId) {
                return;
            }
        }
        main.post(() -> {
            synchronized (AndroidDictationCoordinator.this) {
                if (cancelled || currentSession != sessionId) {
                    return;
                }
                TextInserter.Result insertion = inserter.insertOrCopy(finalText, settings.allowClipboardFallback);
                InsertStatus insertStatus = insertion.status;
                int dictionaryHits = dictionaryStore.recordHits(finalText);
                String historyError = translating
                        ? (errorCode == null
                        ? "translation:" + translationTarget
                        : errorCode + ":" + translationTarget)
                        : errorCode;
                historyStore.add(raw.text, finalText, settings.mode, insertion.appBundleId, insertion.appName,
                        insertStatus, historyError, raw.durationMs, dictionaryHits);
                phase = DictationPhase.IDLE;
                translateNext = false;
                String doneMessage;
                if (insertStatus == InsertStatus.INSERTED) {
                    doneMessage = "已插入 " + finalText.length() + " 个字";
                } else if (insertStatus == InsertStatus.COPIED_FALLBACK) {
                    doneMessage = "已复制 " + finalText.length() + " 个字";
                } else {
                    doneMessage = "插入失败";
                }
                emit(CapsuleState.DONE, doneMessage);
                if (insertStatus == InsertStatus.INSERTED) {
                    emitToast("已插入当前输入框。");
                } else if (insertStatus == InsertStatus.COPIED_FALLBACK) {
                    emitToast("已复制到剪贴板。");
                } else if (settings.allowClipboardFallback) {
                    emitToast("OpenLess 无法复制结果。");
                } else {
                    emitToast("OpenLess 键盘未激活，且剪贴板兜底已关闭。");
                }
            }
        });
    }

    private synchronized void fail(long currentSession, String message) {
        if (currentSession != sessionId) {
            return;
        }
        phase = DictationPhase.IDLE;
        translateNext = false;
        try {
            historyStore.addFailure("", settingsStore.get().mode, "android_error", 0, dictionaryStore.enabledPhrases().size());
        } catch (Exception ignored) {
        }
        emit(CapsuleState.ERROR, message);
        emitToast(message);
    }

    private void postState(CapsuleState state, String message) {
        main.post(() -> emit(state, message));
    }

    private void emit(CapsuleState state, String message) {
        listener.onCapsuleState(state, message);
    }

    private void emitToast(String message) {
        listener.onToast(message == null ? "OpenLess 出错。" : message);
    }
}
