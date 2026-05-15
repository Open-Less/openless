package com.openless.android;

final class OpenLessClient {
    String transcribe(SettingsStore.Settings settings, byte[] pcm) throws Exception {
        AudioRecorder.Recording recording = new AudioRecorder.Recording(pcm, estimateDurationMs(pcm));
        return new WhisperAsrProvider(settings).transcribe(recording).text;
    }

    String polish(SettingsStore.Settings settings, PolishMode mode, String raw) throws Exception {
        return new OpenAiPolishProvider(settings).polish(raw, mode, java.util.Collections.emptyList());
    }

    private static long estimateDurationMs(byte[] pcm) {
        return (pcm.length / 2L) * 1000L / AudioRecorder.SAMPLE_RATE;
    }
}
