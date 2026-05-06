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
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

import java.util.List;

public final class DictionaryActivity extends Activity {
    private static final int OL_CANVAS = Color.rgb(247, 247, 248);
    private static final int OL_SURFACE = Color.rgb(255, 255, 255);
    private static final int OL_INK = Color.rgb(10, 10, 11);
    private static final int OL_INK_2 = Color.rgb(42, 42, 45);
    private static final int OL_INK_3 = Color.rgb(160, 160, 163);
    private static final int OL_INK_4 = Color.rgb(108, 108, 112);
    private static final int OL_BLUE = Color.rgb(37, 99, 235);
    private static final int OL_BLUE_SOFT = Color.rgb(239, 244, 255);
    private static final int OL_LINE = Color.argb(20, 0, 0, 0);
    private static final int OL_LINE_STRONG = Color.argb(36, 0, 0, 0);
    private static final int OL_OK = Color.rgb(22, 163, 74);
    private static final int OL_WARN = Color.rgb(217, 119, 6);
    private static final int OL_ERR = Color.rgb(220, 38, 38);

    private DictionaryStore dictionaryStore;
    private TextView summaryCountView;
    private TextView summaryEnabledView;
    private TextView summaryHitsView;
    private TextView statusView;
    private LinearLayout listContainer;
    private EditText phraseInput;
    private EditText noteInput;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        dictionaryStore = new DictionaryStore(this);
        setContentView(buildContent());
        refreshList();
    }

    @Override
    protected void onResume() {
        super.onResume();
        refreshList();
    }

    private View buildContent() {
        ScrollView scroll = new ScrollView(this);
        scroll.setFillViewport(true);
        scroll.setBackgroundColor(OL_CANVAS);

        LinearLayout root = column();
        root.setPadding(dp(16), dp(16), dp(16), dp(24));
        scroll.addView(root);

        header(root);
        overviewSection(root);
        editorSection(root);
        listSection(root);
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
        TextView title = text("词典", 24, Typeface.BOLD);
        titleCol.addView(title);
        TextView subtitle = text("维护热词、术语和产品名，直接影响识别与润色。", 12, Typeface.NORMAL);
        subtitle.setTextColor(OL_INK_3);
        titleCol.addView(subtitle);
        top.addView(titleCol, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));

        Button export = ghostButton("复制导出", OL_BLUE);
        export.setOnClickListener(v -> {
            String text = dictionaryStore.exportPlainText();
            if (text.trim().isEmpty()) {
                setStatus("当前没有可导出的词条", OL_WARN);
                return;
            }
            android.content.ClipboardManager clipboard =
                    (android.content.ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
            if (clipboard != null) {
                clipboard.setPrimaryClip(android.content.ClipData.newPlainText("OpenLess 字典", text));
                setStatus("词典内容已复制到剪贴板", OL_OK);
            }
        });
        top.addView(export);
        top.addView(spacer(dp(8)));

        Button importButton = ghostButton("粘贴导入", OL_INK_2);
        importButton.setOnClickListener(v -> importFromClipboard());
        top.addView(importButton);

        root.addView(top);
        root.addView(divider());

        statusView = text("就绪", 11, Typeface.BOLD);
        statusView.setTextColor(OL_BLUE);
        statusView.setPadding(0, dp(8), 0, 0);
        root.addView(statusView);
    }

    private void overviewSection(LinearLayout root) {
        card(root, card -> {
            LinearLayout head = row();
            head.setGravity(Gravity.CENTER_VERTICAL);
            TextView title = text("词典概览", 14, Typeface.BOLD);
            title.setTextColor(OL_INK_2);
            head.addView(title, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));
            TextView badge = text("热词", 10, Typeface.BOLD);
            badge.setTextColor(OL_BLUE);
            badge.setPadding(dp(8), dp(4), dp(8), dp(4));
            badge.setBackgroundDrawable(roundedBg(OL_BLUE_SOFT, 999));
            head.addView(badge);
            card.addView(head);

            TextView desc = text("先看当前词条规模，再增删和启停。", 11, Typeface.NORMAL);
            desc.setTextColor(OL_INK_4);
            desc.setPadding(0, dp(4), 0, dp(10));
            card.addView(desc);

            LinearLayout row = row();
            row.addView(summaryCard("词条数"));
            row.addView(spacer(dp(8)));
            row.addView(summaryCard("启用中"));
            row.addView(spacer(dp(8)));
            row.addView(summaryCard("总命中"));
            card.addView(row);
        });
    }

    private void editorSection(LinearLayout root) {
        card(root, card -> {
            TextView title = text("新增词条", 13, Typeface.BOLD);
            title.setTextColor(OL_INK_2);
            card.addView(title);
            TextView desc = text("适合录入产品名、人名、术语和缩写。", 11, Typeface.NORMAL);
            desc.setTextColor(OL_INK_4);
            desc.setPadding(0, dp(4), 0, dp(8));
            card.addView(desc);

            phraseInput = input("", "词条");
            noteInput = input("", "备注（可选）");
            card.addView(phraseInput);
            card.addView(spacer(dp(6)));
            card.addView(noteInput);
            card.addView(spacer(dp(10)));

            LinearLayout actions = row();
            Button add = pillButton("添加词条", OL_BLUE);
            add.setOnClickListener(v -> {
                String phrase = value(phraseInput);
                if (phrase.isEmpty()) {
                    setStatus("请先输入词条", OL_WARN);
                    return;
                }
                dictionaryStore.add(phrase, value(noteInput));
                phraseInput.setText("");
                noteInput.setText("");
                refreshList();
                setStatus("词条已添加", OL_OK);
            });
            actions.addView(add, new LinearLayout.LayoutParams(0, dp(44), 1));
            actions.addView(spacer(dp(8)));

            Button clear = ghostButton("清空输入", OL_INK_3);
            clear.setOnClickListener(v -> {
                phraseInput.setText("");
                noteInput.setText("");
                setStatus("输入已清空", OL_INK_3);
            });
            actions.addView(clear, new LinearLayout.LayoutParams(0, dp(44), 1));
            actions.addView(spacer(dp(8)));

            Button wipe = ghostButton("清空词典", OL_ERR);
            wipe.setOnClickListener(v -> {
                dictionaryStore.replacePlainText("");
                refreshList();
                setStatus("词典已清空", OL_WARN);
            });
            actions.addView(wipe, new LinearLayout.LayoutParams(0, dp(44), 1));
            card.addView(actions);
        });
    }

    private void listSection(LinearLayout root) {
        card(root, card -> {
            LinearLayout head = row();
            head.setGravity(Gravity.CENTER_VERTICAL);
            TextView title = text("词条列表", 13, Typeface.BOLD);
            title.setTextColor(OL_INK_2);
            head.addView(title, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));
            Button refresh = ghostButton("刷新", OL_BLUE);
            refresh.setOnClickListener(v -> {
                refreshList();
                setStatus("列表已刷新", OL_BLUE);
            });
            head.addView(refresh);
            card.addView(head);
            card.addView(spacer(dp(8)));
            listContainer = column();
            card.addView(listContainer);
        });
    }

    private LinearLayout summaryCard(String label) {
        LinearLayout box = column();
        box.setLayoutParams(new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));
        box.setPadding(dp(12), dp(10), dp(12), dp(10));
        box.setBackgroundDrawable(roundedBg(OL_CANVAS, 10));
        TextView title = text(label, 11, Typeface.BOLD);
        title.setTextColor(OL_INK_4);
        box.addView(title);
        TextView value = text("", 14, Typeface.BOLD);
        value.setTextColor(OL_INK);
        value.setPadding(0, dp(3), 0, 0);
        box.addView(value);
        if ("词条数".equals(label)) {
            summaryCountView = value;
        } else if ("启用中".equals(label)) {
            summaryEnabledView = value;
        } else if ("总命中".equals(label)) {
            summaryHitsView = value;
        }
        return box;
    }

    private void refreshList() {
        if (listContainer == null) return;
        listContainer.removeAllViews();
        List<DictionaryStore.Entry> entries = dictionaryStore.list();

        int enabled = 0;
        long hits = 0;
        for (DictionaryStore.Entry entry : entries) {
            if (entry.enabled) enabled++;
            hits += entry.hits;
        }
        if (summaryCountView != null) summaryCountView.setText(String.valueOf(entries.size()));
        if (summaryEnabledView != null) summaryEnabledView.setText(String.valueOf(enabled));
        if (summaryHitsView != null) summaryHitsView.setText(String.valueOf(hits));

        if (entries.isEmpty()) {
            TextView empty = text("还没有词条。可添加产品名、专有名词或术语来提升识别准确率。", 13, Typeface.NORMAL);
            empty.setTextColor(OL_INK_3);
            listContainer.addView(empty);
            return;
        }

        for (DictionaryStore.Entry entry : entries) {
            LinearLayout box = column();
            box.setPadding(dp(12), dp(10), dp(12), dp(10));
            box.setBackgroundDrawable(roundedBg(OL_CANVAS, 10));

            LinearLayout top = row();
            top.setGravity(Gravity.CENTER_VERTICAL);
            TextView phrase = text(entry.phrase, 13, Typeface.BOLD);
            phrase.setTextColor(OL_INK);
            top.addView(phrase, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));
            TextView state = text(entry.enabled ? "启用中" : "已停用", 10, Typeface.BOLD);
            state.setTextColor(entry.enabled ? OL_OK : OL_WARN);
            state.setPadding(dp(8), dp(4), dp(8), dp(4));
            state.setBackgroundDrawable(roundedBg(entry.enabled
                    ? Color.argb(20, 22, 163, 74)
                    : Color.argb(20, 217, 119, 6), 999));
            top.addView(state);
            box.addView(top);

            StringBuilder meta = new StringBuilder();
            meta.append("命中 ").append(entry.hits);
            if (!entry.note.isEmpty()) {
                meta.append("  ·  ").append(entry.note);
            }
            TextView note = text(meta.toString(), 11, Typeface.NORMAL);
            note.setTextColor(OL_INK_4);
            note.setPadding(0, dp(4), 0, 0);
            box.addView(note);

            LinearLayout actions = row();
            actions.setPadding(0, dp(8), 0, 0);
            Button toggle = ghostButton(entry.enabled ? "停用" : "启用", entry.enabled ? OL_WARN : OL_OK);
            toggle.setOnClickListener(v -> {
                dictionaryStore.setEnabled(entry.id, !entry.enabled);
                refreshList();
                setStatus(entry.enabled ? "词条已停用" : "词条已启用", entry.enabled ? OL_WARN : OL_OK);
            });
            actions.addView(toggle, new LinearLayout.LayoutParams(0, dp(40), 1));
            actions.addView(spacer(dp(8)));

            Button copy = ghostButton("复制", OL_BLUE);
            copy.setOnClickListener(v -> {
                android.content.ClipboardManager clipboard =
                        (android.content.ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
                if (clipboard != null) {
                    clipboard.setPrimaryClip(android.content.ClipData.newPlainText("OpenLess 词条", entry.phrase));
                    setStatus("词条已复制", OL_OK);
                }
            });
            actions.addView(copy, new LinearLayout.LayoutParams(0, dp(40), 1));
            actions.addView(spacer(dp(8)));

            Button delete = ghostButton("删除", OL_ERR);
            delete.setOnClickListener(v -> {
                dictionaryStore.remove(entry.id);
                refreshList();
                setStatus("词条已删除", OL_ERR);
            });
            actions.addView(delete, new LinearLayout.LayoutParams(0, dp(40), 1));
            box.addView(actions);

            listContainer.addView(box);
            listContainer.addView(spacer(dp(8)));
        }
    }

    private void importFromClipboard() {
        android.content.ClipboardManager clipboard =
                (android.content.ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
        if (clipboard == null || !clipboard.hasPrimaryClip()) {
            setStatus("剪贴板为空", OL_WARN);
            return;
        }
        android.content.ClipData clip = clipboard.getPrimaryClip();
        if (clip == null || clip.getItemCount() == 0) {
            setStatus("剪贴板为空", OL_WARN);
            return;
        }
        String raw = clip.getItemAt(0).coerceToText(this).toString().trim();
        if (raw.isEmpty()) {
            setStatus("剪贴板为空", OL_WARN);
            return;
        }
        dictionaryStore.replacePlainText(raw);
        refreshList();
        setStatus("已按剪贴板内容覆盖导入词典", OL_OK);
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

    private Button pillButton(String label, int color) {
        Button button = new Button(this);
        button.setText(label);
        button.setAllCaps(false);
        button.setTextColor(Color.WHITE);
        button.setTextSize(12);
        button.setBackgroundDrawable(roundedBg(color, 999));
        button.setPadding(dp(12), dp(8), dp(12), dp(8));
        button.setMinHeight(0);
        button.setMinimumHeight(0);
        return button;
    }

    private EditText input(String value, String hint) {
        EditText edit = new EditText(this);
        edit.setText(value);
        edit.setHint(hint);
        edit.setTextColor(OL_INK);
        edit.setHintTextColor(OL_INK_3);
        edit.setBackgroundDrawable(roundedBg(OL_CANVAS, 10));
        edit.setPadding(dp(12), dp(10), dp(12), dp(10));
        return edit;
    }

    private String value(EditText edit) {
        return edit == null || edit.getText() == null ? "" : edit.getText().toString().trim();
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
