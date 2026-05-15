package com.openless.android;

import android.app.Activity;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.Typeface;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.ShapeDrawable;
import android.graphics.drawable.shapes.RoundRectShape;
import android.os.Bundle;
import android.view.Gravity;
import android.view.View;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

public final class HistoryDetailActivity extends Activity {
    static final String EXTRA_ITEM_ID = "openless.extra.HISTORY_ID";
    static final String EXTRA_CREATED_AT = "openless.extra.HISTORY_CREATED_AT";
    static final String EXTRA_DURATION = "openless.extra.HISTORY_DURATION";
    static final String EXTRA_MODE = "openless.extra.HISTORY_MODE";
    static final String EXTRA_INSERT_STATUS = "openless.extra.HISTORY_INSERT_STATUS";
    static final String EXTRA_APP_NAME = "openless.extra.HISTORY_APP_NAME";
    static final String EXTRA_DICT_HITS = "openless.extra.HISTORY_DICT_HITS";
    static final String EXTRA_ERROR = "openless.extra.HISTORY_ERROR";
    static final String EXTRA_RAW = "openless.extra.HISTORY_RAW";
    static final String EXTRA_TEXT = "openless.extra.HISTORY_TEXT";

    private static final int OL_CANVAS = Color.rgb(247, 247, 248);
    private static final int OL_SURFACE = Color.rgb(255, 255, 255);
    private static final int OL_INK = Color.rgb(10, 10, 11);
    private static final int OL_INK_2 = Color.rgb(42, 42, 45);
    private static final int OL_INK_3 = Color.rgb(160, 160, 163);
    private static final int OL_INK_4 = Color.rgb(108, 108, 112);
    private static final int OL_BLUE = Color.rgb(37, 99, 235);
    private static final int OL_LINE = Color.argb(20, 0, 0, 0);
    private static final int OL_LINE_STRONG = Color.argb(36, 0, 0, 0);
    private static final int OL_OK = Color.rgb(22, 163, 74);
    private static final int OL_WARN = Color.rgb(217, 119, 6);
    private static final int OL_ERR = Color.rgb(220, 38, 38);

    private HistoryStore historyStore;
    private TextView statusView;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        historyStore = new HistoryStore(this);
        setContentView(buildContent());
    }

    private View buildContent() {
        ScrollView scroll = new ScrollView(this);
        scroll.setFillViewport(true);
        scroll.setBackgroundColor(OL_CANVAS);

        LinearLayout root = column();
        root.setPadding(dp(16), dp(16), dp(16), dp(24));
        scroll.addView(root);

        header(root);
        detailSection(root);
        return scroll;
    }

    private void header(LinearLayout root) {
        LinearLayout top = row();
        top.setGravity(Gravity.CENTER_VERTICAL);
        top.setPadding(0, dp(8), 0, dp(8));

        Button back = ghostButton("返回", OL_INK_2);
        back.setOnClickListener(v -> finish());
        top.addView(back);
        top.addView(spacer(dp(8)));

        LinearLayout titleCol = column();
        TextView title = text("历史详情", 24, Typeface.BOLD);
        titleCol.addView(title);
        TextView subtitle = text(
                getStringExtra(EXTRA_CREATED_AT) + "  ·  "
                        + formatDuration(getIntent().getLongExtra(EXTRA_DURATION, 0)),
                12,
                Typeface.NORMAL);
        subtitle.setTextColor(OL_INK_3);
        titleCol.addView(subtitle);
        top.addView(titleCol, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));

        Button copy = ghostButton("复制结果", OL_BLUE);
        copy.setOnClickListener(v -> copyText(primaryBody()));
        top.addView(copy);

        root.addView(top);
        root.addView(divider());

        statusView = text("就绪", 11, Typeface.BOLD);
        statusView.setTextColor(OL_BLUE);
        statusView.setPadding(0, dp(8), 0, 0);
        root.addView(statusView);
    }

    private void detailSection(LinearLayout root) {
        card(root, card -> {
            card.addView(detailBlock("模式", fallback(getStringExtra(EXTRA_MODE), "轻润色"), false));
            card.addView(spacer(dp(8)));
            card.addView(detailBlock("插入状态", fallback(getStringExtra(EXTRA_INSERT_STATUS), "未记录"), false));

            String appName = getStringExtra(EXTRA_APP_NAME);
            if (!appName.isEmpty()) {
                card.addView(spacer(dp(8)));
                card.addView(detailBlock("目标应用", appName, false));
            }

            String dictHits = getStringExtra(EXTRA_DICT_HITS);
            if (!dictHits.isEmpty()) {
                card.addView(spacer(dp(8)));
                card.addView(detailBlock("热词命中", dictHits, false));
            }

            String error = getStringExtra(EXTRA_ERROR);
            if (!error.isEmpty()) {
                card.addView(spacer(dp(8)));
                card.addView(detailBlock("错误", error, false));
            }

            card.addView(spacer(dp(12)));
            card.addView(detailBlock("原文", fallback(getStringExtra(EXTRA_RAW), "（空）"), true));
            card.addView(spacer(dp(10)));
            card.addView(detailBlock("处理结果", fallback(getStringExtra(EXTRA_TEXT), "（空）"), true));

            card.addView(spacer(dp(10)));
            LinearLayout actions = row();
            Button qa = ghostButton("打开问答", OL_BLUE);
            qa.setOnClickListener(v -> {
                android.content.Intent intent = new android.content.Intent(this, QaPanelActivity.class);
                intent.putExtra(QaPanelActivity.EXTRA_CONTEXT, primaryBody());
                startActivity(intent);
                setStatus("已把历史内容送入问答", OL_BLUE);
            });
            actions.addView(qa, new LinearLayout.LayoutParams(0, dp(40), 1));
            actions.addView(spacer(dp(8)));
            Button copy = ghostButton("复制", OL_BLUE);
            copy.setOnClickListener(v -> copyText(primaryBody()));
            actions.addView(copy, new LinearLayout.LayoutParams(0, dp(40), 1));
            card.addView(actions);

            card.addView(spacer(dp(8)));
            Button delete = ghostButton("删除记录", OL_ERR);
            delete.setOnClickListener(v -> deleteItem());
            card.addView(delete, new LinearLayout.LayoutParams(
                    LinearLayout.LayoutParams.MATCH_PARENT,
                    dp(40)));
        });
    }

    private String primaryBody() {
        String text = getStringExtra(EXTRA_TEXT);
        return text.isEmpty() ? getStringExtra(EXTRA_RAW) : text;
    }

    private void copyText(String text) {
        android.content.ClipboardManager clipboard =
                (android.content.ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
        if (clipboard != null) {
            clipboard.setPrimaryClip(android.content.ClipData.newPlainText("OpenLess History", text == null ? "" : text));
            setStatus("已复制", OL_OK);
        }
    }

    private void deleteItem() {
        String id = getStringExtra(EXTRA_ITEM_ID);
        if (id.isEmpty()) {
            setStatus("当前记录缺少标识，无法删除", OL_ERR);
            return;
        }
        historyStore.delete(id);
        setStatus("记录已删除", OL_ERR);
        finish();
    }

    private void setStatus(String message, int color) {
        if (statusView == null) return;
        statusView.setText(message);
        statusView.setTextColor(color);
    }

    private String getStringExtra(String key) {
        String value = getIntent() == null ? null : getIntent().getStringExtra(key);
        return value == null ? "" : value;
    }

    private String fallback(String value, String fallback) {
        return value == null || value.trim().isEmpty() ? fallback : value;
    }

    private String formatDuration(long durationMs) {
        if (durationMs <= 0) return "未记录";
        float seconds = durationMs / 1000f;
        if (seconds < 60f) {
            return String.format(java.util.Locale.US, "%.1f 秒", seconds);
        }
        return String.format(java.util.Locale.US, "%.1f 分钟", seconds / 60f);
    }

    private View detailBlock(String title, String body, boolean selectable) {
        LinearLayout box = column();
        box.setPadding(dp(12), dp(10), dp(12), dp(10));
        box.setBackgroundDrawable(roundedBg(selectable ? OL_CANVAS : OL_SURFACE, 8));

        TextView label = text(title, 11, Typeface.BOLD);
        label.setTextColor(OL_INK_4);
        box.addView(label);

        TextView content = text(body, 13, Typeface.NORMAL);
        content.setTextColor(OL_INK_2);
        content.setPadding(0, dp(6), 0, 0);
        content.setLineSpacing(0, 1.3f);
        content.setTextIsSelectable(selectable);
        box.addView(content);
        return box;
    }

    private void card(LinearLayout root, CardBuilder builder) {
        LinearLayout card = column();
        card.setPadding(dp(14), dp(14), dp(14), dp(14));
        card.setBackgroundDrawable(cardBg());
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT
        );
        params.setMargins(0, 0, 0, dp(10));
        card.setLayoutParams(params);
        builder.build(card);
        root.addView(card);
    }

    private Drawable cardBg() {
        float r = dp(12);
        float[] radii = new float[]{r, r, r, r, r, r, r, r};
        ShapeDrawable bg = new ShapeDrawable(new RoundRectShape(radii, null, null));
        bg.getPaint().setColor(OL_SURFACE);
        return bg;
    }

    private Drawable roundedBg(int color, float radiusDip) {
        float r = dp(radiusDip);
        float[] radii = new float[]{r, r, r, r, r, r, r, r};
        ShapeDrawable bg = new ShapeDrawable(new RoundRectShape(radii, null, null));
        bg.getPaint().setColor(color);
        return bg;
    }

    private Drawable outlineBg(int borderColor) {
        float r = dp(999);
        float[] radii = new float[]{r, r, r, r, r, r, r, r};
        ShapeDrawable bg = new ShapeDrawable(new RoundRectShape(radii, null, null));
        bg.getPaint().setColor(Color.TRANSPARENT);
        bg.getPaint().setStyle(Paint.Style.STROKE);
        bg.getPaint().setStrokeWidth(dp(0.5f));
        bg.getPaint().setColor(borderColor);
        return bg;
    }

    private LinearLayout column() {
        LinearLayout layout = new LinearLayout(this);
        layout.setOrientation(LinearLayout.VERTICAL);
        return layout;
    }

    private LinearLayout row() {
        LinearLayout layout = new LinearLayout(this);
        layout.setOrientation(LinearLayout.HORIZONTAL);
        return layout;
    }

    private View divider() {
        View v = new View(this);
        v.setBackgroundColor(OL_LINE);
        v.setLayoutParams(new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, 1));
        return v;
    }

    private View spacer(int px) {
        View v = new View(this);
        v.setLayoutParams(new LinearLayout.LayoutParams(Math.max(1, px), px));
        return v;
    }

    private TextView text(String value, int sp, int style) {
        TextView view = new TextView(this);
        view.setText(value);
        view.setTextColor(OL_INK);
        view.setTextSize(sp);
        view.setTypeface(Typeface.DEFAULT, style);
        view.setLineSpacing(0, 1.2f);
        return view;
    }

    private Button ghostButton(String label, int color) {
        Button button = new Button(this);
        button.setText(label);
        button.setAllCaps(false);
        button.setTextColor(color);
        button.setTextSize(11);
        button.setBackgroundDrawable(outlineBg(OL_LINE_STRONG));
        button.setPadding(dp(8), dp(4), dp(8), dp(4));
        button.setMinHeight(0);
        button.setMinimumHeight(0);
        return button;
    }

    private int dp(int value) {
        return (int) (value * getResources().getDisplayMetrics().density + 0.5f);
    }

    private float dp(float value) {
        return value * getResources().getDisplayMetrics().density + 0.5f;
    }

    private interface CardBuilder {
        void build(LinearLayout card);
    }
}
