package com.openless.android;

final class InsertStatus {
    static final InsertStatus INSERTED = new InsertStatus("inserted", "已插入");
    static final InsertStatus COPIED_FALLBACK = new InsertStatus("copiedFallback", "已复制");
    static final InsertStatus FAILED = new InsertStatus("failed", "失败");

    final String id;
    final String label;

    private InsertStatus(String id, String label) {
        this.id = id;
        this.label = label;
    }

    static InsertStatus fromId(String id) {
        if (INSERTED.id.equals(id)) {
            return INSERTED;
        }
        if (COPIED_FALLBACK.id.equals(id)) {
            return COPIED_FALLBACK;
        }
        return FAILED;
    }
}
