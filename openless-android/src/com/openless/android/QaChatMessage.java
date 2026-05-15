package com.openless.android;

final class QaChatMessage {
    final String role;
    final String content;

    QaChatMessage(String role, String content) {
        this.role = role == null ? "user" : role;
        this.content = content == null ? "" : content;
    }
}
