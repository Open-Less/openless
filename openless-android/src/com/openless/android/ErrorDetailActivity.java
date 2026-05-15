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

public final class ErrorDetailActivity extends Activity {
    static final String EXTRA_TITLE = "openless.extra.ERROR_TITLE";
    static final String EXTRA_SOURCE = "openless.extra.ERROR_SOURCE";
    static final String EXTRA_MESSAGE = "openless.extra.ERROR_MESSAGE";

    private static final int OL_CANVAS = Color.rgb(247, 247, 248);
    private static final int OL_SURFACE = Color.rgb(255, 255, 255);
    private static final int OL_INK = Color.rgb(10, 10, 11);
    private static final int OL_INK_2 = Color.rgb(42, 42, 45);
    private static final int OL_INK_3 = Color.rgb(160, 160, 163);
    private static final int OL_INK_4 = Color.rgb(108, 108, 112);
    private static final int OL_BLUE = Color.rgb(37, 99, 235);
    private static final int OL_OK = Color.rgb(22, 163, 74);
    private static final int OL_RED = Color.rgb(220, 38, 38);
    private static final int OL_RED_SOFT = Color.rgb(254, 242, 242);
    private static final int OL_LINE = Color.argb(20, 0, 0, 0);
    private static final int OL_LINE_STRONG = Color.argb(36, 0, 0, 0);

    private TextView statusView;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
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
        bodySection(root);
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
        TextView title = text(fallback(extra(EXTRA_TITLE), "错误"), 24, Typeface.BOLD);
        titleCol.addView(title);
        TextView subtitle = text(fallback(extra(EXTRA_SOURCE), "OpenLess"), 12, Typeface.NORMAL);
        subtitle.setTextColor(OL_INK_3);
        titleCol.addView(subtitle);
        top.addView(titleCol, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));

        Button copy = ghostButton("复制", OL_BLUE);
        copy.setOnClickListener(v -> copyMessage());
        top.addView(copy);

        root.addView(top);
        root.addView(divider());

        statusView = text("就绪", 11, Typeface.BOLD);
        statusView.setTextColor(OL_BLUE);
        statusView.setPadding(0, dp(8), 0, 0);
        root.addView(statusView);
    }

    private void bodySection(LinearLayout root) {
        card(root, card -> {
            LinearLayout badgeRow = row();
            badgeRow.setGravity(Gravity.CENTER_VERTICAL);
            TextView badge = text("错误详情", 10, Typeface.BOLD);
            badge.setTextColor(OL_RED);
            badge.setPadding(dp(8), dp(4), dp(8), dp(4));
            badge.setBackgroundDrawable(roundedBg(OL_RED_SOFT, 999));
            badgeRow.addView(badge);
            card.addView(badgeRow);

            TextView desc = text("这里保留原始错误信息，便于排查和转发。", 11, Typeface.NORMAL);
            desc.setTextColor(OL_INK_4);
            desc.setPadding(0, dp(8), 0, dp(10));
            card.addView(desc);

            card.addView(detailBlock("来源", fallback(extra(EXTRA_SOURCE), "OpenLess"), false));
            card.addView(spacer(dp(8)));
            card.addView(detailBlock("消息", fallback(extra(EXTRA_MESSAGE), "未知错误"), true));
        });
    }

    private void copyMessage() {
        String message = "标题："
                + fallback(extra(EXTRA_TITLE), "错误")
                + "\n来源："
                + fallback(extra(EXTRA_SOURCE), "OpenLess")
                + "\n消息：\n"
                + fallback(extra(EXTRA_MESSAGE), "未知错误");
        android.content.ClipboardManager clipboard =
                (android.content.ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
        if (clipboard != null) {
            clipboard.setPrimaryClip(android.content.ClipData.newPlainText("OpenLess Error", message));
            setStatus("错误信息已复制", OL_OK);
        }
    }

    private void setStatus(String message, int color) {
        if (statusView == null) return;
        statusView.setText(message);
        statusView.setTextColor(color);
    }

    private String extra(String key) {
        String value = getIntent() == null ? null : getIntent().getStringExtra(key);
        return value == null ? "" : value;
    }

    private String fallback(String value, String fallback) {
        return value == null || value.trim().isEmpty() ? fallback : value;
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
