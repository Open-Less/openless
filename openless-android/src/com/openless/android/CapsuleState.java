package com.openless.android;

final class CapsuleState {
    static final CapsuleState IDLE = new CapsuleState("就绪", 0xff2563eb);
    static final CapsuleState STARTING = new CapsuleState("启动中", 0xffdc2626);
    static final CapsuleState RECORDING = new CapsuleState("听写中", 0xffdc2626);
    static final CapsuleState TRANSCRIBING = new CapsuleState("转写中", 0xffd97706);
    static final CapsuleState POLISHING = new CapsuleState("润色中", 0xffd97706);
    static final CapsuleState TRANSLATING = new CapsuleState("翻译中", 0xffd97706);
    static final CapsuleState DONE = new CapsuleState("完成", 0xff16a34a);
    static final CapsuleState CANCELLED = new CapsuleState("已取消", 0xffa0a0a3);
    static final CapsuleState ERROR = new CapsuleState("错误", 0xffdc2626);

    final String label;
    final int color;

    private CapsuleState(String label, int color) {
        this.label = label;
        this.color = color;
    }
}
