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

import java.util.ArrayList;

public final class ModelListActivity extends Activity {
    static final String EXTRA_MODELS = "openless.extra.MODELS";

    private static final int OL_CANVAS = Color.rgb(247, 247, 248);
    private static final int OL_SURFACE = Color.rgb(255, 255, 255);
    private static final int OL_INK = Color.rgb(10, 10, 11);
    private static final int OL_INK_2 = Color.rgb(42, 42, 45);
    private static final int OL_INK_3 = Color.rgb(160, 160, 163);
    private static final int OL_BLUE = Color.rgb(37, 99, 235);
    private static final int OL_LINE = Color.argb(20, 0, 0, 0);
    private static final int OL_LINE_STRONG = Color.argb(36, 0, 0, 0);
    private static final int OL_OK = Color.rgb(22, 163, 74);

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
        modelsSection(root);
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
        TextView title = text("可用模型", 24, Typeface.BOLD);
        titleCol.addView(title);
        ArrayList<String> models = getIntent() == null ? null : getIntent().getStringArrayListExtra(EXTRA_MODELS);
        int count = models == null ? 0 : models.size();
        TextView subtitle = text("当前接口返回 " + count + " 个模型。点击即可复制。", 12, Typeface.NORMAL);
        subtitle.setTextColor(OL_INK_3);
        titleCol.addView(subtitle);
        top.addView(titleCol, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));

        root.addView(top);
        root.addView(divider());

        statusView = text("就绪", 11, Typeface.BOLD);
        statusView.setTextColor(OL_BLUE);
        statusView.setPadding(0, dp(8), 0, 0);
        root.addView(statusView);
    }

    private void modelsSection(LinearLayout root) {
        card(root, card -> {
            ArrayList<String> models = getIntent() == null ? null : getIntent().getStringArrayListExtra(EXTRA_MODELS);
            if (models == null || models.isEmpty()) {
                TextView empty = text("没有返回任何模型。", 13, Typeface.NORMAL);
                empty.setTextColor(OL_INK_3);
                card.addView(empty);
                return;
            }
            for (String model : models) {
                LinearLayout row = new LinearLayout(this);
                row.setOrientation(LinearLayout.HORIZONTAL);
                row.setGravity(Gravity.CENTER_VERTICAL);
                row.setPadding(dp(12), dp(10), dp(12), dp(10));
                row.setBackgroundDrawable(roundedBg(OL_CANVAS, 10));

                TextView text = text(model, 13, Typeface.NORMAL);
                text.setTextColor(OL_INK_2);
                row.addView(text, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));

                Button copy = ghostButton("复制", OL_BLUE);
                copy.setOnClickListener(v -> copyModel(model));
                row.addView(copy);

                card.addView(row);
                card.addView(spacer(dp(8)));
            }
        });
    }

    private void copyModel(String model) {
        android.content.ClipboardManager clipboard =
                (android.content.ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
        if (clipboard != null) {
            clipboard.setPrimaryClip(android.content.ClipData.newPlainText("OpenLess Model", model));
            setStatus("模型名已复制", OL_OK);
        }
    }

    private void setStatus(String message, int color) {
        if (statusView == null) return;
        statusView.setText(message);
        statusView.setTextColor(color);
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
