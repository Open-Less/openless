package com.openless.android;

final class RawTranscript {
    final String text;
    final long durationMs;

    RawTranscript(String text, long durationMs) {
        this.text = text == null ? "" : text.trim();
        this.durationMs = durationMs;
    }
}
