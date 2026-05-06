package com.openless.android;

final class PolishMode {
    static final PolishMode RAW = new PolishMode("raw", "原文");
    static final PolishMode LIGHT = new PolishMode("light", "轻润色");
    static final PolishMode STRUCTURED = new PolishMode("structured", "结构化");
    static final PolishMode FORMAL = new PolishMode("formal", "正式");

    final String id;
    final String label;

    private PolishMode(String id, String label) {
        this.id = id;
        this.label = label;
    }

    static PolishMode[] values() {
        return new PolishMode[]{RAW, LIGHT, STRUCTURED, FORMAL};
    }

    static PolishMode fromId(String id) {
        for (PolishMode mode : values()) {
            if (mode.id.equals(id)) {
                return mode;
            }
        }
        return LIGHT;
    }
}
