package com.openless.android;

import android.Manifest;
import android.app.Activity;
import android.content.ClipData;
import android.content.ClipboardManager;
import android.content.Context;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.Typeface;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.ShapeDrawable;
import android.graphics.drawable.shapes.RoundRectShape;
import android.net.Uri;
import android.os.Bundle;
import android.text.InputType;
import android.view.Gravity;
import android.view.MotionEvent;
import android.view.View;
import android.view.inputmethod.InputMethodManager;
import android.widget.Button;
import android.widget.CheckBox;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

public final class MainActivity extends Activity {
    private static final int REQ_AUDIO = 42;
    private static final int REQ_NOTIFICATIONS = 43;
    private static final int SECTION_DICTATION = 1;
    private static final int SECTION_HISTORY = 2;
    private static final int SECTION_TOOLS = 3;

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

    private final AudioRecorder recorder = new AudioRecorder();
    private final OpenLessClient client = new OpenLessClient();
    private final ExecutorService executor = Executors.newSingleThreadExecutor();
    private SettingsStore settingsStore;
    private HistoryStore historyStore;
    private DictionaryStore dictionaryStore;
    private SettingsStore.Settings settings;
    private TextInserter inserter;
    private TextView status;
    private TextView imeStatus;
    private TextView rawText;
    private TextView finalText;
    private TextView historyCountView;
    private TextView overviewAsrValue;
    private TextView overviewLlmValue;
    private TextView overviewModeValue;
    private TextView overviewHistoryValue;
    private TextView overviewTranslationValue;
    private LinearLayout historyList;
    private LinearLayout permissionStatusList;
    private LinearLayout modeRow;
    private Button micButton;
    private Button floatingButton;
    private Button translateButton;
    private Button llmCheckButton;
    private Button asrCheckButton;
    private Button listModelsButton;
    private VolcengineStreamingSession inlineVolcengineSession;
    private boolean translateNext;
    private static final String[] LLM_PROVIDER_IDS = new String[]{"ark", "deepseek", "siliconflow", "openai", "custom"};
    private LinearLayout dictationSectionView;
    private LinearLayout historySectionView;
    private LinearLayout toolsSectionView;
    private final ArrayList<Button> sectionButtons = new ArrayList<>();
    private final ArrayList<Button> historyFilterButtons = new ArrayList<>();
    private int currentSection = SECTION_DICTATION;
    private String currentHistoryFilter = "all";

    @Override
    protected void onCreate(Bundle bundle) {
        super.onCreate(bundle);
        settingsStore = new SettingsStore(this);
        historyStore = new HistoryStore(this);
        dictionaryStore = new DictionaryStore(this);
        inserter = new TextInserter(this);
        settings = settingsStore.get();
        setContentView(buildContent());
        refreshModeButtons();
        refreshHistory();
        ensureAudioPermission();
    }

    @Override
    protected void onDestroy() {
        recorder.stop();
        executor.shutdownNow();
        super.onDestroy();
    }

    @Override
    protected void onResume() {
        super.onResume();
        settings = settingsStore.get();
        refreshModeButtons();
        refreshTranslationButton();
        refreshImeStatus();
        refreshPermissionDiagnostics();
        refreshHistory();
        refreshOverview();
    }

    private View buildContent() {
        ScrollView scroll = new ScrollView(this);
        scroll.setFillViewport(true);
        scroll.setBackgroundColor(OL_CANVAS);
        LinearLayout root = column();
        root.setPadding(dp(16), dp(16), dp(16), dp(24));
        scroll.addView(root);
        header(root);
        sectionNav(root);

        dictationSectionView = column();
        historySectionView = column();
        toolsSectionView = column();
        root.addView(dictationSectionView);
        root.addView(historySectionView);
        root.addView(toolsSectionView);

        overviewSection(dictationSectionView);
        floatingSection(dictationSectionView);
        modeSection(dictationSectionView);
        recordingSection(dictationSectionView);
        transcriptSection(dictationSectionView);

        historySection(historySectionView);

        permissionDiagnosticsSection(toolsSectionView);
        diagnosticsSection(toolsSectionView);
        utilitySection(toolsSectionView);

        applySection(SECTION_DICTATION);
        return scroll;
    }

    private void header(LinearLayout root) {
        LinearLayout top = row();
        top.setGravity(Gravity.CENTER_VERTICAL);
        top.setPadding(0, dp(8), 0, dp(8));
        LinearLayout titleCol = column();
        TextView title = text("OpenLess", 24, Typeface.BOLD);
        titleCol.addView(title);
        TextView subtitle = text("安卓版语音输入", 12, Typeface.NORMAL);
        subtitle.setTextColor(OL_INK_3);
        titleCol.addView(subtitle);
        top.addView(titleCol, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));
        Button settingsButton = ghostButton("设置", OL_BLUE);
        settingsButton.setOnClickListener(v -> openSettingsActivity());
        top.addView(settingsButton);
        root.addView(top);
        root.addView(divider());
    }

    private void sectionNav(LinearLayout root) {
        LinearLayout navWrap = row();
        navWrap.setPadding(0, dp(14), 0, dp(12));

        LinearLayout nav = row();
        nav.setPadding(dp(2), dp(2), dp(2), dp(2));
        nav.setBackgroundDrawable(roundedBg(Color.argb(10, 0, 0, 0), 10));

        Button dictation = sectionButton("听写", SECTION_DICTATION);
        Button history = sectionButton("历史", SECTION_HISTORY);
        Button tools = sectionButton("工具", SECTION_TOOLS);
        nav.addView(dictation);
        nav.addView(spacer(dp(4)));
        nav.addView(history);
        nav.addView(spacer(dp(4)));
        nav.addView(tools);
        navWrap.addView(nav);
        root.addView(navWrap);
    }

    private Button sectionButton(String label, int section) {
        Button button = new Button(this);
        button.setText(label);
        button.setAllCaps(false);
        button.setTextSize(12);
        button.setMinHeight(0);
        button.setMinimumHeight(0);
        button.setPadding(dp(14), dp(7), dp(14), dp(7));
        button.setTag(section);
        button.setOnClickListener(v -> applySection((Integer) v.getTag()));
        sectionButtons.add(button);
        return button;
    }

    private void applySection(int section) {
        currentSection = section;
        if (dictationSectionView != null) {
            dictationSectionView.setVisibility(section == SECTION_DICTATION ? View.VISIBLE : View.GONE);
        }
        if (historySectionView != null) {
            historySectionView.setVisibility(section == SECTION_HISTORY ? View.VISIBLE : View.GONE);
        }
        if (toolsSectionView != null) {
            toolsSectionView.setVisibility(section == SECTION_TOOLS ? View.VISIBLE : View.GONE);
        }
        for (Button button : sectionButtons) {
            boolean active = ((Integer) button.getTag()) == section;
            button.setTextColor(active ? Color.WHITE : OL_INK_3);
            button.setBackgroundDrawable(active ? roundedBg(OL_BLUE, 8) : roundedBg(Color.TRANSPARENT, 8));
        }
    }

    private void overviewSection(LinearLayout root) {
        card(root, card -> {
            LinearLayout head = row();
            head.setGravity(Gravity.CENTER_VERTICAL);
            TextView title = text("概览", 15, Typeface.BOLD);
            head.addView(title, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));
            TextView badge = text("安卓", 10, Typeface.BOLD);
            badge.setTextColor(OL_BLUE);
            badge.setPadding(dp(8), dp(4), dp(8), dp(4));
            badge.setBackgroundDrawable(roundedBg(OL_BLUE_SOFT, 999));
            head.addView(badge);
            card.addView(head);

            TextView desc = text("把当前 provider、模式、翻译和历史状态集中到一张卡里。", 12, Typeface.NORMAL);
            desc.setTextColor(OL_INK_3);
            desc.setPadding(0, dp(4), 0, dp(12));
            card.addView(desc);

            LinearLayout topRow = row();
            topRow.addView(metricCard("ASR", true));
            topRow.addView(spacer(dp(8)));
            topRow.addView(metricCard("LLM", true));
            card.addView(topRow);

            card.addView(spacer(dp(8)));

            LinearLayout middleRow = row();
            middleRow.addView(metricCard("模式", false));
            middleRow.addView(spacer(dp(8)));
            middleRow.addView(metricCard("历史", false));
            card.addView(middleRow);

            card.addView(spacer(dp(8)));

            LinearLayout translationBox = column();
            translationBox.setPadding(dp(12), dp(10), dp(12), dp(10));
            translationBox.setBackgroundDrawable(roundedBg(OL_CANVAS, 10));
            TextView label = text("翻译目标", 11, Typeface.BOLD);
            label.setTextColor(OL_INK_4);
            translationBox.addView(label);
            overviewTranslationValue = text("", 14, Typeface.BOLD);
            overviewTranslationValue.setTextColor(OL_INK);
            overviewTranslationValue.setPadding(0, dp(3), 0, 0);
            translationBox.addView(overviewTranslationValue);
            card.addView(translationBox);
        });
        refreshOverview();
    }

    private LinearLayout metricCard(String label, boolean provider) {
        LinearLayout box = column();
        box.setLayoutParams(new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));
        box.setPadding(dp(12), dp(10), dp(12), dp(10));
        box.setBackgroundDrawable(roundedBg(OL_CANVAS, 10));
        TextView title = text(label, 11, Typeface.BOLD);
        title.setTextColor(OL_INK_4);
        box.addView(title);
        TextView value = text("", provider ? 13 : 16, Typeface.BOLD);
        value.setTextColor(OL_INK);
        value.setPadding(0, dp(3), 0, 0);
        box.addView(value);
        if ("ASR".equals(label)) {
            overviewAsrValue = value;
        } else if ("LLM".equals(label)) {
            overviewLlmValue = value;
        } else if ("模式".equals(label)) {
            overviewModeValue = value;
        } else if ("历史".equals(label)) {
            overviewHistoryValue = value;
        }
        return box;
    }

    private void refreshOverview() {
        if (overviewAsrValue == null || settings == null) return;
        overviewAsrValue.setText("whisper".equalsIgnoreCase(settings.activeAsrProvider) ? "Whisper 兼容" : "火山流式");
        overviewLlmValue.setText(providerDisplayName(settings.activeLlmProvider));
        overviewModeValue.setText(settings.mode == null ? "轻润色" : settings.mode.label);
        overviewHistoryValue.setText(String.valueOf(historyStore.list().size()) + " 条");
        String target = settings.translationTargetLanguage == null ? "" : settings.translationTargetLanguage.trim();
        overviewTranslationValue.setText(target.isEmpty() ? "未配置" : target);
        overviewTranslationValue.setTextColor(target.isEmpty() ? OL_WARN : OL_BLUE);
    }

    private void floatingSection(LinearLayout root) {
        card(root, card -> {
            LinearLayout line = row();
            line.setGravity(Gravity.CENTER_VERTICAL);
            LinearLayout info = column();
            TextView label = text("悬浮触发器", 15, Typeface.BOLD);
            info.addView(label);
            TextView desc = text("点击悬浮气泡，可在任意应用里开始听写。", 12, Typeface.NORMAL);
            desc.setTextColor(OL_INK_3);
            info.addView(desc);
            line.addView(info, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));
            floatingButton = pillButton("启动", OL_BLUE);
            floatingButton.setOnClickListener(v -> toggleFloatingTrigger());
            line.addView(floatingButton);
            card.addView(line);
        });

        LinearLayout actions = row();
        actions.setPadding(0, dp(8), 0, 0);
        Button overlayButton = ghostButton("悬浮窗权限", OL_WARN);
        overlayButton.setOnClickListener(v -> requestOverlayPermission());
        actions.addView(overlayButton, new LinearLayout.LayoutParams(0, dp(40), 1));

        Button imeButton = ghostButton("启用键盘", OL_BLUE);
        imeButton.setOnClickListener(v -> openImeSettings());
        actions.addView(imeButton, new LinearLayout.LayoutParams(0, dp(40), 1));

        root.addView(actions);

        imeStatus = text("", 12, Typeface.NORMAL);
        imeStatus.setPadding(0, dp(8), 0, 0);
        root.addView(imeStatus);
        refreshImeStatus();
    }

    private void refreshImeStatus() {
        if (imeStatus == null) return;
        boolean active = OpenLessInputMethodService.isActive();
        String pkg = OpenLessInputMethodService.currentTargetPackage();
        if (active) {
            String appInfo = pkg != null ? "（" + pkg + "）" : "";
            imeStatus.setText("OpenLess 键盘已激活" + appInfo + "，可直接插入文字");
            imeStatus.setTextColor(OL_OK);
        } else {
            boolean enabled = isImeEnabled();
            if (enabled) {
                imeStatus.setText("OpenLess 键盘已启用，但当前未激活；请在任意输入框切换到它");
                imeStatus.setTextColor(OL_WARN);
            } else {
                imeStatus.setText("OpenLess 键盘尚未启用；请打开系统输入法设置启用");
                imeStatus.setTextColor(OL_ERR);
            }
        }
    }

    private boolean isImeEnabled() {
        android.view.inputmethod.InputMethodManager imm =
                (android.view.inputmethod.InputMethodManager) getSystemService(INPUT_METHOD_SERVICE);
        if (imm == null) return false;
        String expectedId = getPackageName() + "/" + OpenLessInputMethodService.class.getName();
        for (android.view.inputmethod.InputMethodInfo info : imm.getEnabledInputMethodList()) {
            if (expectedId.equals(info.getId())) return true;
        }
        return false;
    }

    private void diagnosticsSection(LinearLayout root) {
        card(root, card -> {
            TextView label = text("提供商诊断", 13, Typeface.BOLD);
            label.setTextColor(OL_INK_2);
            card.addView(label);
            TextView desc = text("快速检查 LLM 与 ASR 的配置是否完整。", 12, Typeface.NORMAL);
            desc.setTextColor(OL_INK_3);
            desc.setPadding(0, dp(4), 0, dp(10));
            card.addView(desc);
            LinearLayout topRow = row();
            llmCheckButton = pillButton("检测 LLM", OL_BLUE);
            llmCheckButton.setOnClickListener(v -> runLlmCheck());
            topRow.addView(llmCheckButton, new LinearLayout.LayoutParams(0, dp(40), 1));
            topRow.addView(spacer(dp(8)));
            asrCheckButton = pillButton("检测 ASR", OL_WARN);
            asrCheckButton.setOnClickListener(v -> runAsrCheck());
            topRow.addView(asrCheckButton, new LinearLayout.LayoutParams(0, dp(40), 1));
            card.addView(topRow);
            card.addView(spacer(dp(8)));
            listModelsButton = ghostButton("列出 LLM 模型", OL_BLUE);
            listModelsButton.setOnClickListener(v -> runListModels());
            card.addView(listModelsButton, new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, dp(40)));
            card.addView(spacer(dp(8)));
            Button providerSettingsButton = ghostButton("打开服务商设置", OL_INK_2);
            providerSettingsButton.setOnClickListener(v -> openSettingsActivity("providers"));
            card.addView(providerSettingsButton, new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, dp(40)));
        });
    }

    private void utilitySection(LinearLayout root) {
        card(root, card -> {
            TextView label = text("快捷工具", 13, Typeface.BOLD);
            label.setTextColor(OL_INK_2);
            card.addView(label);
            TextView desc = text("把词典、问答和输入法入口集中到一起。", 12, Typeface.NORMAL);
            desc.setTextColor(OL_INK_3);
            desc.setPadding(0, dp(4), 0, dp(10));
            card.addView(desc);
            card.addView(actionTile(
                    "词典",
                    "维护热词、产品名和专有名词，直接影响识别与润色。",
                    "管理",
                    OL_INK_2,
                    v -> openDictionaryActivity()
            ));
            card.addView(spacer(dp(8)));
            card.addView(actionTile(
                    "问答面板",
                    "进入多轮问答和语音提问面板，适合长上下文追问。",
                    "打开",
                    OL_BLUE,
                    v -> openQaPanel()
            ));
            card.addView(spacer(dp(8)));
            card.addView(actionTile(
                    "剪贴板问答",
                    "把当前剪贴板文本作为上下文直接送进问答链路。",
                    "发送",
                    OL_INK_2,
                    v -> openQaFromClipboard()
            ));
            card.addView(spacer(dp(8)));
            card.addView(actionTile(
                    "OpenLess 键盘",
                    "切到 OpenLess 输入法后，可优先走 IME 直接插入路径。",
                    "启用",
                    OL_BLUE,
                    v -> openImeSettings()
            ));
            card.addView(spacer(dp(8)));
            card.addView(actionTile(
                    "听写偏好",
                    "跳到设置页的听写分区，管理模式、胶囊和剪贴板兜底。",
                    "前往",
                    OL_INK_2,
                    v -> openSettingsActivity("dictation")
            ));
            card.addView(spacer(dp(8)));
            card.addView(actionTile(
                    "语言与翻译",
                    "管理工作语言和翻译目标，直接影响翻译按钮状态。",
                    "前往",
                    OL_BLUE,
                    v -> openSettingsActivity("language")
            ));
        });
    }

    private void permissionDiagnosticsSection(LinearLayout root) {
        card(root, card -> {
            TextView label = text("Android 诊断", 13, Typeface.BOLD);
            label.setTextColor(OL_INK_2);
            card.addView(label);
            TextView desc = text("检查听写、悬浮窗与文本插入的系统就绪状态。", 12, Typeface.NORMAL);
            desc.setTextColor(OL_INK_3);
            desc.setPadding(0, dp(4), 0, dp(10));
            card.addView(desc);
            permissionStatusList = column();
            card.addView(permissionStatusList);
        });
        refreshPermissionDiagnostics();
    }

    private void modeSection(LinearLayout root) {
        card(root, card -> {
            TextView label = text("润色模式", 13, Typeface.BOLD);
            label.setTextColor(OL_INK_2);
            card.addView(label);
            TextView desc = text("选择转写文本后处理的方式。", 12, Typeface.NORMAL);
            desc.setTextColor(OL_INK_3);
            desc.setPadding(0, dp(4), 0, dp(10));
            card.addView(desc);
            modeRow = row();
            for (PolishMode mode : PolishMode.values()) {
                Button button = modePill(mode.label);
                button.setTag(mode);
                button.setOnClickListener(v -> {
                    PolishMode selected = (PolishMode) v.getTag();
                    if (!isModeEnabled(selected)) return;
                    settings.mode = selected;
                    settingsStore.save(settings);
                    refreshModeButtons();
                });
                modeRow.addView(button);
                if (mode != PolishMode.FORMAL) modeRow.addView(spacer(dp(8)));
            }
            card.addView(modeRow);
            card.addView(spacer(dp(10)));
            translateButton = ghostButton("翻译一次", OL_BLUE);
            translateButton.setOnClickListener(v -> armTranslation());
            card.addView(translateButton);
        });
        refreshTranslationButton();
    }

    private void recordingSection(LinearLayout root) {
        card(root, card -> {
            status = text("就绪", 14, Typeface.BOLD);
            status.setTextColor(OL_BLUE);
            status.setGravity(Gravity.CENTER);
            status.setPadding(0, 0, 0, dp(12));
            card.addView(status);
            micButton = new Button(this);
            micButton.setText("按住说话");
            micButton.setTextColor(Color.WHITE);
            micButton.setTextSize(18);
            micButton.setTypeface(Typeface.DEFAULT_BOLD);
            micButton.setBackgroundDrawable(roundedBg(OL_BLUE, 12));
            micButton.setAllCaps(false);
            micButton.setOnTouchListener((view, event) -> {
                if (event.getAction() == MotionEvent.ACTION_DOWN) { beginRecording(); return true; }
                if (event.getAction() == MotionEvent.ACTION_UP || event.getAction() == MotionEvent.ACTION_CANCEL) { endRecording(); return true; }
                return true;
            });
            card.addView(micButton, new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, dp(64)));
        });
    }

    private void transcriptSection(LinearLayout root) {
        card(root, card -> {
            TextView rawLabel = text("原文转写", 12, Typeface.BOLD);
            rawLabel.setTextColor(OL_INK_2);
            card.addView(rawLabel);
            rawText = text("", 14, Typeface.NORMAL);
            rawText.setTextColor(OL_INK_2);
            rawText.setTextIsSelectable(true);
            rawText.setMinHeight(dp(56));
            rawText.setPadding(dp(12), dp(10), dp(12), dp(10));
            rawText.setBackgroundDrawable(roundedBg(OL_CANVAS, 8));
            rawText.setLineSpacing(0, 1.3f);
            card.addView(rawText);
            card.addView(spacer(dp(12)));
            TextView polishLabel = text("处理结果", 12, Typeface.BOLD);
            polishLabel.setTextColor(OL_INK_2);
            card.addView(polishLabel);
            finalText = text("", 14, Typeface.NORMAL);
            finalText.setTextColor(OL_INK);
            finalText.setTextIsSelectable(true);
            finalText.setMinHeight(dp(56));
            finalText.setPadding(dp(12), dp(10), dp(12), dp(10));
            finalText.setBackgroundDrawable(roundedBg(OL_BLUE_SOFT, 8));
            finalText.setLineSpacing(0, 1.3f);
            card.addView(finalText);
            card.addView(spacer(dp(10)));
            LinearLayout actions = row();
            Button copyButton = pillButton("复制", OL_BLUE);
            copyButton.setOnClickListener(v -> copyFinalText());
            actions.addView(copyButton);
            actions.addView(spacer(dp(8)));
            Button clearButton = ghostButton("清空", OL_INK_3);
            clearButton.setOnClickListener(v -> { rawText.setText(""); finalText.setText(""); setStatus("已清空", OL_BLUE); });
            actions.addView(clearButton);
            card.addView(actions);
        });
    }

    // ─── History section with visual badges ─────────────────────────

    private void historySection(LinearLayout root) {
        card(root, card -> {
            LinearLayout line = row();
            line.setGravity(Gravity.CENTER_VERTICAL);
            TextView label = text("历史记录", 15, Typeface.BOLD);
            line.addView(label, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));

            historyCountView = text(String.valueOf(historyStore.list().size()) + " 条", 10, Typeface.BOLD);
            historyCountView.setTextColor(OL_BLUE);
            historyCountView.setPadding(dp(8), dp(4), dp(8), dp(4));
            historyCountView.setBackgroundDrawable(roundedBg(OL_BLUE_SOFT, 999));
            line.addView(historyCountView);
            line.addView(spacer(dp(8)));

            Button copyAllButton = ghostButton("全部复制", OL_BLUE);
            copyAllButton.setOnClickListener(v -> copyAllHistory());
            line.addView(copyAllButton);
            line.addView(spacer(dp(8)));

            Button clearButton = ghostButton("清空", OL_ERR);
            clearButton.setOnClickListener(v -> { historyStore.clear(); refreshHistory(); setStatus("历史记录已清空", OL_BLUE); });
            line.addView(clearButton);
            card.addView(line);
            card.addView(spacer(dp(6)));
            TextView hint = text("点击或长按进入详情页，复制、转问答和删除已收口到详情页。", 11, Typeface.NORMAL);
            hint.setTextColor(OL_INK_4);
            hint.setPadding(0, 0, 0, dp(8));
            card.addView(hint);

            LinearLayout filters = row();
            filters.addView(historyFilterButton("全部", "all"));
            filters.addView(spacer(dp(6)));
            filters.addView(historyFilterButton("原文", PolishMode.RAW.id));
            filters.addView(spacer(dp(6)));
            filters.addView(historyFilterButton("轻润色", PolishMode.LIGHT.id));
            filters.addView(spacer(dp(6)));
            filters.addView(historyFilterButton("结构化", PolishMode.STRUCTURED.id));
            filters.addView(spacer(dp(6)));
            filters.addView(historyFilterButton("正式", PolishMode.FORMAL.id));
            card.addView(filters);
            card.addView(spacer(dp(8)));
            historyList = column();
            card.addView(historyList);
        });
        refreshHistoryFilters();
    }

    private void refreshHistory() {
        if (historyList == null) return;
        historyList.removeAllViews();
        List<HistoryStore.Item> allItems = historyStore.list();
        List<HistoryStore.Item> items = new ArrayList<>();
        for (HistoryStore.Item item : allItems) {
            if ("all".equals(currentHistoryFilter) || (item.mode != null && currentHistoryFilter.equals(item.mode.id))) {
                items.add(item);
            }
        }
        if (historyCountView != null) {
            historyCountView.setText("all".equals(currentHistoryFilter)
                    ? String.valueOf(allItems.size()) + " 条"
                    : String.valueOf(items.size()) + " / " + allItems.size() + " 条");
        }
        if (items.isEmpty()) {
            LinearLayout emptyBox = column();
            emptyBox.setPadding(0, dp(24), 0, dp(24));
            emptyBox.setGravity(Gravity.CENTER);
            TextView empty = text("all".equals(currentHistoryFilter) ? "还没有听写记录。" : "当前筛选下没有记录。", 13, Typeface.NORMAL);
            empty.setTextColor(OL_INK_3);
            empty.setGravity(Gravity.CENTER);
            emptyBox.addView(empty);
            TextView hint = text("all".equals(currentHistoryFilter) ? "点击麦克风按钮或悬浮触发器开始。" : "切换筛选或继续听写后会出现在这里。", 11, Typeface.NORMAL);
            hint.setTextColor(OL_INK_4);
            hint.setGravity(Gravity.CENTER);
            hint.setPadding(0, dp(4), 0, 0);
            emptyBox.addView(hint);
            historyList.addView(emptyBox);
            refreshOverview();
            return;
        }

        for (HistoryStore.Item item : items) {
            LinearLayout row = new LinearLayout(this);
            row.setOrientation(LinearLayout.VERTICAL);
            row.setPadding(0, dp(10), 0, dp(10));

            LinearLayout topLine = new LinearLayout(this);
            topLine.setOrientation(LinearLayout.HORIZONTAL);
            topLine.setGravity(Gravity.CENTER_VERTICAL);

            String timeStr = formatTime(item.createdAt);
            TextView timeView = text(timeStr, 11, Typeface.NORMAL);
            timeView.setTextColor(OL_INK_4);
            timeView.setTypeface(Typeface.MONOSPACE);
            topLine.addView(timeView);

            topLine.addView(spacer(dp(8)));

            // Mode pill
            View modePill = buildModePill(item.mode);
            topLine.addView(modePill);

            topLine.addView(spacer(dp(6)));

            // Status badge
            View statusBadge = buildStatusBadge(item.insertStatus);
            topLine.addView(statusBadge);

            // App name if available
            String appName = item.appName;
            if (appName != null && !appName.isEmpty()) {
                topLine.addView(spacer(dp(6)));
                TextView appView = text(appName, 9, Typeface.NORMAL);
                appView.setTextColor(OL_INK_4);
                topLine.addView(appView);
            }

            // Error if present
            if (item.errorCode != null && !item.errorCode.isEmpty()) {
                topLine.addView(spacer(dp(6)));
                TextView errView = text(humanizeHistoryError(item.errorCode), 9, Typeface.NORMAL);
                errView.setTextColor(OL_ERR);
                topLine.addView(errView);
            }

            row.addView(topLine);
            row.addView(spacer(dp(4)));

            // Text preview
            String body = item.text != null && !item.text.isEmpty() ? item.text : item.raw;
            if (body == null) body = "";
            String preview = body.length() > 140 ? body.substring(0, 140) + "..." : body;
            boolean hasMore = body.length() > 140;
            TextView bodyView = text(preview, 13, Typeface.NORMAL);
            bodyView.setTextColor(hasMore ? OL_INK_2 : OL_INK);
            bodyView.setLineSpacing(0, 1.25f);
            bodyView.setTextIsSelectable(false);
            row.addView(bodyView);

            final HistoryStore.Item itemRef = item;
            final String copyBody = body;

            // Tap actions
            row.setOnClickListener(v -> openHistoryDetailActivity(itemRef));
            row.setOnLongClickListener(v -> {
                openHistoryDetailActivity(itemRef);
                setStatus("已打开历史详情", OL_BLUE);
                return true;
            });

            historyList.addView(row);
            View divider = new View(this);
            divider.setBackgroundColor(OL_LINE);
            divider.setLayoutParams(new LinearLayout.LayoutParams(
                    LinearLayout.LayoutParams.MATCH_PARENT, 1));
            historyList.addView(divider);
        }
        refreshOverview();
    }

    private Button historyFilterButton(String label, String filterId) {
        Button button = new Button(this);
        button.setText(label);
        button.setAllCaps(false);
        button.setTextSize(11);
        button.setMinHeight(0);
        button.setMinimumHeight(0);
        button.setPadding(dp(10), dp(5), dp(10), dp(5));
        button.setTag(filterId);
        button.setOnClickListener(v -> {
            currentHistoryFilter = (String) v.getTag();
            refreshHistoryFilters();
            refreshHistory();
        });
        historyFilterButtons.add(button);
        return button;
    }

    private void refreshHistoryFilters() {
        for (Button button : historyFilterButtons) {
            boolean active = currentHistoryFilter.equals(button.getTag());
            button.setTextColor(active ? Color.WHITE : OL_INK_3);
            button.setBackgroundDrawable(active ? roundedBg(OL_INK, 999) : roundedBg(Color.TRANSPARENT, 999));
        }
    }

    private String formatTime(String iso) {
        if (iso == null || iso.isEmpty()) return "";
        String cleaned = iso.replace("T", " ");
        if (cleaned.length() > 16) cleaned = cleaned.substring(0, 16);
        return cleaned;
    }

    private String formatDuration(long durationMs) {
        if (durationMs <= 0) return "未记录";
        float seconds = durationMs / 1000f;
        if (seconds < 60f) {
            return String.format(java.util.Locale.US, "%.1f 秒", seconds);
        }
        return String.format(java.util.Locale.US, "%.1f 分钟", seconds / 60f);
    }

    private View buildModePill(PolishMode mode) {
        TextView pill = new TextView(this);
        pill.setText(mode == null ? "轻润色" : mode.label);
        pill.setTextSize(9);
        pill.setTypeface(Typeface.DEFAULT_BOLD);
        pill.setTextColor(OL_INK_2);
        pill.setPadding(dp(6), dp(2), dp(6), dp(2));
        pill.setBackgroundDrawable(roundedBg(OL_CANVAS, 999));
        return pill;
    }

    private View buildStatusBadge(InsertStatus status) {
        LinearLayout badge = new LinearLayout(this);
        badge.setOrientation(LinearLayout.HORIZONTAL);
        badge.setGravity(Gravity.CENTER_VERTICAL);
        badge.setPadding(dp(6), dp(2), dp(6), dp(2));
        badge.setBackgroundDrawable(pillSmallBg(OL_LINE_STRONG));

        int dotColor;
        String label;
        if (status == InsertStatus.INSERTED) {
            dotColor = OL_OK;
            label = "已插入";
        } else if (status == InsertStatus.COPIED_FALLBACK) {
            dotColor = OL_BLUE;
            label = "已复制";
        } else {
            dotColor = OL_ERR;
            label = "失败";
        }

        View dot = new View(this);
        dot.setLayoutParams(new LinearLayout.LayoutParams(dp(5), dp(5)));
        dot.setBackgroundDrawable(circleBg(dotColor));
        badge.addView(dot);

        badge.addView(spacer(dp(4)));

        TextView tv = new TextView(this);
        tv.setText(label);
        tv.setTextSize(9);
        tv.setTypeface(Typeface.DEFAULT_BOLD);
        tv.setTextColor(dotColor);
        badge.addView(tv);

        return badge;
    }

    private void copyAllHistory() {
        List<HistoryStore.Item> items = historyStore.list();
        if (items.isEmpty()) {
            setStatus("没有可复制的历史记录", OL_WARN);
            return;
        }
        StringBuilder sb = new StringBuilder();
        for (HistoryStore.Item item : items) {
            String time = formatTime(item.createdAt);
            String mode = item.mode == null ? "" : item.mode.label;
            String text = item.text != null && !item.text.isEmpty() ? item.text : item.raw;
            sb.append("[").append(time).append("] [").append(mode).append("]\n").append(text).append("\n\n");
        }
        copyText(sb.toString().trim());
        setStatus("已复制全部历史记录", OL_OK);
    }

    private void beginRecording() {
        if (!ensureAudioPermission() || recorder.isRecording()) return;
        try {
            rawText.setText("");
            finalText.setText("");
            SettingsStore.Settings current = settingsStore.get();
            if (!"whisper".equals(current.activeAsrProvider)) {
                inlineVolcengineSession = new VolcengineStreamingSession(current, dictionaryStore.enabledPhrases());
                inlineVolcengineSession.open();
                recorder.start((pcm, length) -> {
                    VolcengineStreamingSession session = inlineVolcengineSession;
                    if (session != null) session.consume(pcm, length);
                });
            } else {
                recorder.start();
            }
            setStatus("正在听写...", OL_ERR);
            micButton.setText("松开结束");
        } catch (Exception e) { showError(e); }
    }

    private void endRecording() {
        if (!recorder.isRecording()) return;
        AudioRecorder.Recording recording = recorder.stop();
        micButton.setText("按住说话");
        if (recording.pcm.length < 1000) { setStatus("录音过短", OL_WARN); return; }
        setStatus("正在转写...", OL_WARN);
        VolcengineStreamingSession session = inlineVolcengineSession;
        inlineVolcengineSession = null;
        if (session != null) {
            executor.execute(() -> processVolcengineRecording(session, recording.durationMs));
        } else {
            executor.execute(() -> processRecording(recording));
        }
    }

    private void processVolcengineRecording(VolcengineStreamingSession session, long durationMs) {
        try { RawTranscript raw = session.finish(durationMs); session.close(); processTranscript(raw); }
        catch (Exception e) { session.close(); runOnUiThread(() -> showError(e)); }
    }

    private void processRecording(AudioRecorder.Recording recording) {
        try { String raw = client.transcribe(settingsStore.get(), recording.pcm); processTranscript(new RawTranscript(raw, recording.durationMs)); }
        catch (Exception e) { runOnUiThread(() -> showError(e)); }
    }

    private void processTranscript(RawTranscript raw) throws Exception {
        SettingsStore.Settings current = settingsStore.get();
        boolean translating = translateNext;
        runOnUiThread(() -> {
            rawText.setText(raw.text);
            setStatus(translating ? "正在翻译..." : (current.mode == PolishMode.RAW ? "正在复制..." : "正在润色..."), OL_WARN);
        });
        OpenAiPolishProvider provider = new OpenAiPolishProvider(current);
        List<String> hotwords = dictionaryStore.enabledPhrases();
        String computedText;
        String computedErrorCode = null;
        if (translating) {
            try { computedText = provider.translate(raw.text, current.translationTargetLanguage, hotwords, current.workingLanguages); }
            catch (Exception e) { computedText = raw.text; computedErrorCode = "translation_failed"; }
        } else {
            computedText = provider.polish(raw.text, current.mode, hotwords);
        }
        final String polished = computedText;
        final String errorCode = computedErrorCode;
        final String translationTarget = current.translationTargetLanguage == null ? "" : current.translationTargetLanguage.trim();
        runOnUiThread(() -> {
            finalText.setText(polished);
            TextInserter.Result insertion = inserter.insertOrCopy(polished, current.allowClipboardFallback);
            InsertStatus insertStatus = insertion.status;
            int dictionaryHits = dictionaryStore.recordHits(polished);
            String historyError = translating ? (errorCode == null ? "translation:" + translationTarget : errorCode + ":" + translationTarget) : errorCode;
            historyStore.add(raw.text, polished, current.mode, insertion.appBundleId, insertion.appName, insertStatus, historyError, raw.durationMs, dictionaryHits);
            refreshHistory();
            translateNext = false;
            refreshTranslationButton();
            if (insertStatus == InsertStatus.INSERTED) { setStatus("已插入", OL_OK); }
            else if (insertStatus == InsertStatus.COPIED_FALLBACK) { setStatus("已复制到剪贴板", OL_BLUE); }
            else {
                setStatus(
                        !current.allowClipboardFallback
                                ? "插入失败；剪贴板兜底已关闭"
                                : "插入失败",
                        OL_ERR);
            }
        });
    }

    private void openHistoryDetailActivity(HistoryStore.Item item) {
        if (item == null) return;
        Intent intent = new Intent(this, HistoryDetailActivity.class);
        intent.putExtra(HistoryDetailActivity.EXTRA_ITEM_ID, item.id == null ? "" : item.id);
        intent.putExtra(HistoryDetailActivity.EXTRA_CREATED_AT, formatTime(item.createdAt));
        intent.putExtra(HistoryDetailActivity.EXTRA_DURATION, item.durationMs);
        intent.putExtra(HistoryDetailActivity.EXTRA_MODE, item.mode == null ? "轻润色" : item.mode.label);
        intent.putExtra(HistoryDetailActivity.EXTRA_INSERT_STATUS, item.insertStatus == null ? "未记录" : item.insertStatus.label);
        intent.putExtra(HistoryDetailActivity.EXTRA_APP_NAME, item.appName == null ? "" : item.appName);
        intent.putExtra(HistoryDetailActivity.EXTRA_DICT_HITS, item.dictionaryEntryCount == null || item.dictionaryEntryCount <= 0 ? "" : String.valueOf(item.dictionaryEntryCount));
        intent.putExtra(HistoryDetailActivity.EXTRA_ERROR, humanizeHistoryError(item.errorCode));
        intent.putExtra(HistoryDetailActivity.EXTRA_RAW, item.raw == null ? "" : item.raw);
        intent.putExtra(HistoryDetailActivity.EXTRA_TEXT, item.text == null ? "" : item.text);
        startActivity(intent);
    }

    private void toggleFloatingTrigger() {
        if (!android.provider.Settings.canDrawOverlays(this)) { requestOverlayPermission(); return; }
        if (!ensureAudioPermission()) return;
        Intent intent = new Intent(this, FloatingTriggerService.class);
        if ("停止".contentEquals(floatingButton.getText())) {
            stopService(intent); floatingButton.setText("启动"); floatingButton.setBackgroundDrawable(pillBg(OL_BLUE)); setStatus("悬浮触发器已停止", OL_BLUE);
        } else {
            if (android.os.Build.VERSION.SDK_INT >= 26) startForegroundService(intent); else startService(intent);
            floatingButton.setText("停止"); floatingButton.setBackgroundDrawable(pillBg(OL_ERR)); setStatus("悬浮触发器运行中", OL_WARN);
        }
    }

    private void openQaPanel() {
        startActivity(new Intent(this, QaPanelActivity.class));
    }

    private void openDictionaryActivity() {
        startActivity(new Intent(this, DictionaryActivity.class));
    }

    private void openSettingsActivity() {
        openSettingsActivity(null);
    }

    private void openSettingsActivity(String section) {
        Intent intent = new Intent(this, SettingsActivity.class);
        if (section != null && !section.trim().isEmpty()) {
            intent.putExtra(SettingsActivity.EXTRA_INITIAL_SECTION, section);
        }
        startActivity(intent);
    }

    private void refreshFloatingServiceSettings() {
        if (floatingButton == null || !"停止".contentEquals(floatingButton.getText())) {
            return;
        }
        Intent intent = new Intent(this, FloatingTriggerService.class);
        intent.setAction(FloatingTriggerService.ACTION_REFRESH_SETTINGS);
        if (android.os.Build.VERSION.SDK_INT >= 26) {
            startForegroundService(intent);
        } else {
            startService(intent);
        }
    }

    private void openQaFromClipboard() {
        ClipboardManager clipboard = (ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
        if (clipboard == null || !clipboard.hasPrimaryClip()) { setStatus("剪贴板为空", OL_WARN); return; }
        ClipData clip = clipboard.getPrimaryClip();
        if (clip == null || clip.getItemCount() == 0) { setStatus("剪贴板为空", OL_WARN); return; }
        String context = clip.getItemAt(0).coerceToText(this).toString().trim();
        if (context.isEmpty()) { setStatus("剪贴板为空", OL_WARN); return; }
        Intent qaIntent = new Intent(this, QaPanelActivity.class);
        if (!context.isEmpty()) qaIntent.putExtra(QaPanelActivity.EXTRA_CONTEXT, context);
        startActivity(qaIntent);
        setStatus("已把剪贴板内容送入问答", OL_BLUE);
    }

    private void requestOverlayPermission() {
        startActivity(new Intent(android.provider.Settings.ACTION_MANAGE_OVERLAY_PERMISSION, Uri.parse("package:" + getPackageName())));
    }

    private void openImeSettings() {
        startActivity(new Intent(android.provider.Settings.ACTION_INPUT_METHOD_SETTINGS));
        setStatus("先启用 OpenLess 键盘，再到任意输入框切换到它", OL_WARN);
    }

    private void armTranslation() {
        SettingsStore.Settings current = settingsStore.get();
        String target = current.translationTargetLanguage == null ? "" : current.translationTargetLanguage.trim();
        if (translateNext) {
            translateNext = false;
            refreshTranslationButton();
            setStatus("已取消翻译", OL_BLUE);
            return;
        }
        if (target.isEmpty()) {
            setStatus("请先在设置中填写翻译目标语言", OL_WARN);
            return;
        }
        translateNext = true;
        refreshTranslationButton();
        setStatus("已准备翻译到 " + target, OL_WARN);
    }

    private void refreshTranslationButton() {
        if (translateButton == null) return;
        String target = settings == null || settings.translationTargetLanguage == null
                ? ""
                : settings.translationTargetLanguage.trim();
        if (translateNext) {
            translateButton.setText("取消翻译");
            translateButton.setTextColor(OL_ERR);
        } else if (target.isEmpty()) {
            translateButton.setText("翻译一次（未配置）");
            translateButton.setTextColor(OL_WARN);
        } else {
            translateButton.setText("翻译到 " + target);
            translateButton.setTextColor(OL_BLUE);
        }
    }

    private void runLlmCheck() {
        setDiagnosticsBusy(true, "正在检测 LLM...");
        executor.execute(() -> {
            try {
                String result = ProviderDiagnostics.validateLlm(settingsStore.get());
                runOnUiThread(() -> { setStatus(result, OL_OK); setDiagnosticsBusy(false, "就绪"); });
            } catch (Exception e) {
                runOnUiThread(() -> { setStatus(e.getMessage() == null ? "LLM 检测失败" : e.getMessage(), OL_ERR); setDiagnosticsBusy(false, "就绪"); });
            }
        });
    }

    private void runAsrCheck() {
        setDiagnosticsBusy(true, "正在检测 ASR 配置...");
        executor.execute(() -> {
            try {
                String result = ProviderDiagnostics.validateAsr(settingsStore.get());
                runOnUiThread(() -> { setStatus(result, OL_OK); setDiagnosticsBusy(false, "就绪"); });
            } catch (Exception e) {
                runOnUiThread(() -> { setStatus(e.getMessage() == null ? "ASR 检测失败" : e.getMessage(), OL_ERR); setDiagnosticsBusy(false, "就绪"); });
            }
        });
    }

    private void runListModels() {
        SettingsStore.Settings current = settingsStore.get();
        setDiagnosticsBusy(true, "正在加载模型...");
        executor.execute(() -> {
            try {
                List<String> models = ProviderDiagnostics.listModels(current.llmBaseUrl, current.llmApiKey);
                runOnUiThread(() -> {
                    setDiagnosticsBusy(false, "就绪");
                    Intent intent = new Intent(this, ModelListActivity.class);
                    intent.putStringArrayListExtra(ModelListActivity.EXTRA_MODELS, new ArrayList<>(models));
                    startActivity(intent);
                });
            } catch (Exception e) {
                runOnUiThread(() -> { setStatus(e.getMessage() == null ? "模型加载失败" : e.getMessage(), OL_ERR); setDiagnosticsBusy(false, "就绪"); });
            }
        });
    }

    private void setDiagnosticsBusy(boolean busy, String message) {
        llmCheckButton.setEnabled(!busy);
        asrCheckButton.setEnabled(!busy);
        listModelsButton.setEnabled(!busy);
        float alpha = busy ? 0.6f : 1f;
        llmCheckButton.setAlpha(alpha); asrCheckButton.setAlpha(alpha); listModelsButton.setAlpha(alpha);
        setStatus(message, busy ? OL_WARN : OL_BLUE);
    }

    private LinearLayout actionTile(String title,
                                    String description,
                                    String actionLabel,
                                    int actionColor,
                                    View.OnClickListener listener) {
        LinearLayout tile = column();
        tile.setPadding(dp(12), dp(12), dp(12), dp(12));
        tile.setBackgroundDrawable(roundedBg(OL_CANVAS, 10));

        LinearLayout top = row();
        top.setGravity(Gravity.CENTER_VERTICAL);
        TextView titleView = text(title, 13, Typeface.BOLD);
        titleView.setTextColor(OL_INK);
        top.addView(titleView, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));
        Button action = ghostButton(actionLabel, actionColor);
        action.setOnClickListener(listener);
        top.addView(action);
        tile.addView(top);

        TextView descView = text(description, 11, Typeface.NORMAL);
        descView.setTextColor(OL_INK_4);
        descView.setPadding(0, dp(6), 0, 0);
        descView.setLineSpacing(0, 1.25f);
        tile.addView(descView);
        return tile;
    }

    private void refreshModeButtons() {
        if (modeRow == null) return;
        for (int i = 0; i < modeRow.getChildCount(); i++) {
            View child = modeRow.getChildAt(i);
            if (child instanceof Button && child.getTag() instanceof PolishMode) {
                Button b = (Button) child;
                PolishMode mode = (PolishMode) b.getTag();
                boolean enabled = isModeEnabled(mode);
                boolean active = mode == settings.mode;
                b.setEnabled(enabled);
                b.setTextColor(enabled && active ? Color.WHITE : OL_INK_3);
                if (enabled && active) b.setBackgroundDrawable(pillBg(OL_BLUE));
                else if (enabled) b.setBackgroundDrawable(outlineBg(OL_LINE_STRONG));
                else { b.setBackgroundDrawable(outlineBg(OL_LINE)); b.setAlpha(0.4f); }
            }
        }
    }

    private void refreshPermissionDiagnostics() {
        if (permissionStatusList == null) return;
        permissionStatusList.removeAllViews();
        List<PermissionStatus> items = AndroidPermissionDiagnostics.collect(this);
        for (int i = 0; i < items.size(); i++) {
            permissionStatusList.addView(buildPermissionRow(items.get(i)));
            if (i < items.size() - 1) {
                View divider = new View(this);
                divider.setBackgroundColor(OL_LINE);
                divider.setLayoutParams(new LinearLayout.LayoutParams(
                        LinearLayout.LayoutParams.MATCH_PARENT, 1));
                permissionStatusList.addView(divider);
            }
        }
    }

    private View buildPermissionRow(PermissionStatus item) {
        LinearLayout row = new LinearLayout(this);
        row.setOrientation(LinearLayout.HORIZONTAL);
        row.setGravity(Gravity.CENTER_VERTICAL);
        row.setPadding(dp(8), dp(8), dp(8), dp(8));
        row.setBackgroundDrawable(roundedBg(item.ok ? OL_SURFACE : Color.argb(8, 220, 38, 38), 8));

        View dot = new View(this);
        dot.setLayoutParams(new LinearLayout.LayoutParams(dp(7), dp(7)));
        dot.setBackgroundDrawable(circleBg(item.ok ? OL_OK : OL_ERR));
        row.addView(dot);
        row.addView(spacer(dp(10)));

        LinearLayout col = column();
        col.setLayoutParams(new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));
        TextView title = text(item.title, 13, Typeface.BOLD);
        title.setTextColor(item.ok ? OL_INK_2 : OL_ERR);
        col.addView(title);
        TextView detail = text(item.detail, 10, Typeface.NORMAL);
        detail.setTextColor(item.ok ? OL_INK_3 : OL_ERR);
        detail.setPadding(0, dp(2), 0, 0);
        col.addView(detail);
        row.addView(col);

        if (!PermissionStatus.ACTION_NONE.equals(item.action)) {
            Button action = ghostButton(item.actionLabel, item.ok ? OL_BLUE : OL_ERR);
            action.setOnClickListener(v -> runPermissionAction(item.action));
            row.addView(action);
        }
        return row;
    }

    private void runPermissionAction(String action) {
        if (PermissionStatus.ACTION_OVERLAY.equals(action)) requestOverlayPermission();
        else if (PermissionStatus.ACTION_NOTIFICATIONS.equals(action)) startActivity(new Intent(android.provider.Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(android.provider.Settings.EXTRA_APP_PACKAGE, getPackageName()));
        else if (PermissionStatus.ACTION_IME.equals(action)) openImeSettings();
        else if (PermissionStatus.ACTION_APP_PERMISSIONS.equals(action)) openAppPermissionSettings();
    }

    private void openAppPermissionSettings() {
        Intent intent = new Intent(android.provider.Settings.ACTION_APPLICATION_DETAILS_SETTINGS);
        intent.setData(Uri.parse("package:" + getPackageName()));
        startActivity(intent);
    }

    private void copyFinalText() {
        String text = finalText.getText().toString();
        if (text.trim().isEmpty()) return;
        copyText(text);
        setStatus("已复制", OL_OK);
    }

    private void copyText(String text) {
        ClipboardManager clipboard = (ClipboardManager) getSystemService(Context.CLIPBOARD_SERVICE);
        if (clipboard != null) clipboard.setPrimaryClip(ClipData.newPlainText("OpenLess", text));
    }

    private void showError(Exception e) {
        setStatus("出错", OL_ERR);
        Intent intent = new Intent(this, ErrorDetailActivity.class);
        intent.putExtra(ErrorDetailActivity.EXTRA_TITLE, "OpenLess 出错");
        intent.putExtra(ErrorDetailActivity.EXTRA_SOURCE, "主界面");
        intent.putExtra(ErrorDetailActivity.EXTRA_MESSAGE, e == null ? "未知错误" : (e.getMessage() == null ? e.toString() : e.getMessage()));
        startActivity(intent);
    }

    private void setStatus(String message, int color) {
        if (status == null) return;
        status.setText(message);
        status.setTextColor(color);
    }

    private android.widget.RadioButton providerOption(String label, String value, String selected) {
        android.widget.RadioButton button = new android.widget.RadioButton(this);
        button.setText(label);
        button.setTag(value);
        button.setTextColor(OL_INK_2);
        button.setChecked(value.equals(normalizeLlmProvider(selected)));
        return button;
    }

    private String normalizeLlmProvider(String value) {
        if (value == null) return "ark";
        for (String id : LLM_PROVIDER_IDS) {
            if (id.equalsIgnoreCase(value)) return id;
        }
        return "custom";
    }

    private String providerDisplayName(String value) {
        String provider = normalizeLlmProvider(value);
        if ("ark".equals(provider)) return "Ark";
        if ("deepseek".equals(provider)) return "DeepSeek";
        if ("siliconflow".equals(provider)) return "SiliconFlow";
        if ("openai".equals(provider)) return "OpenAI";
        return "自定义";
    }

    private String selectedLlmProvider(android.widget.RadioButton ark,
                                       android.widget.RadioButton deepseek,
                                       android.widget.RadioButton siliconflow,
                                       android.widget.RadioButton openai,
                                       android.widget.RadioButton custom) {
        if (deepseek.isChecked()) return "deepseek";
        if (siliconflow.isChecked()) return "siliconflow";
        if (openai.isChecked()) return "openai";
        if (custom.isChecked()) return "custom";
        return "ark";
    }

    private String llmBasePreset(String provider) {
        if ("ark".equals(provider)) return "https://ark.cn-beijing.volces.com/api/v3";
        if ("deepseek".equals(provider)) return "https://api.deepseek.com/v1";
        if ("siliconflow".equals(provider)) return "https://api.siliconflow.cn/v1";
        if ("openai".equals(provider)) return "https://api.openai.com/v1";
        return "";
    }

    private String llmBaseHint(String provider) {
        String preset = llmBasePreset(provider);
        return preset.isEmpty() ? "https://your-endpoint/v1" : preset;
    }

    private String llmModelHint(String provider) {
        if ("ark".equals(provider)) return "deepseek-v3-2";
        if ("deepseek".equals(provider)) return "deepseek-v4-flash";
        if ("siliconflow".equals(provider)) return "Qwen/Qwen2.5-7B-Instruct";
        if ("openai".equals(provider)) return "gpt-4o";
        return "model-name";
    }

    private boolean isKnownLlmBase(String value) {
        if (value == null || value.trim().isEmpty()) return false;
        String normalized = value.trim();
        for (String id : LLM_PROVIDER_IDS) {
            String preset = llmBasePreset(id);
            if (!preset.isEmpty() && preset.equalsIgnoreCase(normalized)) return true;
        }
        return false;
    }

    private String humanizeHistoryError(String errorCode) {
        if (errorCode == null || errorCode.trim().isEmpty()) return "";
        if ("translation_failed".equals(errorCode)) return "翻译失败";
        if ("android_error".equals(errorCode)) return "Android 错误";
        if (errorCode.startsWith("translation_failed:")) {
            String target = errorCode.substring("translation_failed:".length()).trim();
            return target.isEmpty() ? "翻译失败" : "翻译失败：" + target;
        }
        if (errorCode.startsWith("translation:")) {
            String target = errorCode.substring("translation:".length()).trim();
            return target.isEmpty() ? "翻译" : "翻译：" + target;
        }
        return errorCode;
    }

    private boolean isModeEnabled(PolishMode mode) {
        return settings.enabledModes == null || settings.enabledModes.contains(mode);
    }

    private List<PolishMode> parseModes(String raw) {
        ArrayList<PolishMode> out = new ArrayList<>();
        if (raw == null) return out;
        String[] parts = raw.split("[,，\\n]");
        for (String part : parts) {
            String value = part == null ? "" : part.trim();
            if (value.isEmpty()) continue;
            PolishMode mode = PolishMode.fromId(value);
            if (mode == PolishMode.LIGHT && !"light".equalsIgnoreCase(value) && !"轻润色".equals(value)) {
                if ("raw".equalsIgnoreCase(value) || "原文".equals(value)) mode = PolishMode.RAW;
                else if ("structured".equalsIgnoreCase(value) || "结构化".equals(value)) mode = PolishMode.STRUCTURED;
                else if ("formal".equalsIgnoreCase(value) || "正式".equals(value)) mode = PolishMode.FORMAL;
            }
            if (!out.contains(mode)) out.add(mode);
        }
        return out;
    }

    private List<String> parseStrings(String raw) {
        ArrayList<String> out = new ArrayList<>();
        if (raw == null) return out;
        String[] parts = raw.split("[,，\\n]");
        for (String part : parts) {
            String value = part == null ? "" : part.trim();
            if (!value.isEmpty() && !out.contains(value)) out.add(value);
        }
        if (out.isEmpty()) out.add("简体中文");
        return out;
    }

    private String modesText(List<PolishMode> modes) {
        StringBuilder sb = new StringBuilder();
        if (modes != null) for (PolishMode m : modes) { if (sb.length() > 0) sb.append(','); sb.append(m.id); }
        return sb.toString();
    }

    private String stringsText(List<String> values) {
        StringBuilder sb = new StringBuilder();
        if (values != null) for (String v : values) { if (sb.length() > 0) sb.append(','); sb.append(v); }
        return sb.toString();
    }

    private boolean ensureAudioPermission() {
        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) {
            ensureNotificationPermission(); return true;
        }
        requestPermissions(new String[]{Manifest.permission.RECORD_AUDIO}, REQ_AUDIO);
        return false;
    }

    private void ensureNotificationPermission() {
        if (android.os.Build.VERSION.SDK_INT >= 33 && checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED)
            requestPermissions(new String[]{Manifest.permission.POST_NOTIFICATIONS}, REQ_NOTIFICATIONS);
    }

    // ─── View helpers ───────────────────────────────────────────────

    private void card(LinearLayout root, CardBuilder builder) {
        LinearLayout card = column();
        card.setPadding(dp(14), dp(14), dp(14), dp(14));
        card.setBackgroundDrawable(cardBg());
        card.setLayoutParams(new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, LinearLayout.LayoutParams.WRAP_CONTENT));
        ((LinearLayout.LayoutParams) card.getLayoutParams()).setMargins(0, 0, 0, dp(10));
        builder.build(card);
        root.addView(card);
    }

    private Drawable cardBg() {
        float r = dp(12);
        float[] radii = new float[]{r, r, r, r, r, r, r, r};
        RoundRectShape shape = new RoundRectShape(radii, null, null);
        ShapeDrawable bg = new ShapeDrawable(shape);
        bg.getPaint().setColor(OL_SURFACE);
        return bg;
    }

    private Drawable roundedBg(int color, float radiusDip) {
        float r = dp(radiusDip);
        float[] radii = new float[]{r, r, r, r, r, r, r, r};
        RoundRectShape shape = new RoundRectShape(radii, null, null);
        ShapeDrawable bg = new ShapeDrawable(shape);
        bg.getPaint().setColor(color);
        return bg;
    }

    private Drawable pillBg(int color) {
        float r = dp(999);
        float[] radii = new float[]{r, r, r, r, r, r, r, r};
        RoundRectShape shape = new RoundRectShape(radii, null, null);
        ShapeDrawable bg = new ShapeDrawable(shape);
        bg.getPaint().setColor(color);
        return bg;
    }

    private Drawable pillSmallBg(int borderColor) {
        float r = dp(999);
        float[] radii = new float[]{r, r, r, r, r, r, r, r};
        RoundRectShape shape = new RoundRectShape(radii, null, null);
        ShapeDrawable bg = new ShapeDrawable(shape);
        bg.getPaint().setColor(Color.TRANSPARENT);
        bg.getPaint().setStyle(Paint.Style.STROKE);
        bg.getPaint().setStrokeWidth(dp(0.5f));
        bg.getPaint().setColor(borderColor);
        return bg;
    }

    private Drawable outlineBg(int borderColor) {
        float r = dp(999);
        float[] radii = new float[]{r, r, r, r, r, r, r, r};
        RoundRectShape shape = new RoundRectShape(radii, null, null);
        ShapeDrawable bg = new ShapeDrawable(shape);
        bg.getPaint().setColor(Color.TRANSPARENT);
        bg.getPaint().setStyle(Paint.Style.STROKE);
        bg.getPaint().setStrokeWidth(dp(0.5f));
        bg.getPaint().setColor(borderColor);
        return bg;
    }

    private Drawable circleBg(int color) {
        float r = dp(999);
        float[] radii = new float[]{r, r, r, r, r, r, r, r};
        RoundRectShape shape = new RoundRectShape(radii, null, null);
        ShapeDrawable bg = new ShapeDrawable(shape);
        bg.getPaint().setColor(color);
        return bg;
    }

    private LinearLayout column() { LinearLayout l = new LinearLayout(this); l.setOrientation(LinearLayout.VERTICAL); return l; }
    private LinearLayout row() { LinearLayout l = new LinearLayout(this); l.setOrientation(LinearLayout.HORIZONTAL); return l; }

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
        TextView v = new TextView(this);
        v.setText(value);
        v.setTextColor(OL_INK);
        v.setTextSize(sp);
        v.setTypeface(Typeface.DEFAULT, style);
        v.setLineSpacing(0, 1.2f);
        return v;
    }

    private TextView sectionTitle(String value) {
        TextView v = text(value, 12, Typeface.BOLD);
        v.setTextColor(OL_INK_2);
        return v;
    }

    private Button pillButton(String label, int color) {
        Button b = new Button(this);
        b.setText(label);
        b.setTextColor(Color.WHITE);
        b.setTextSize(12);
        b.setTypeface(Typeface.DEFAULT_BOLD);
        b.setBackgroundDrawable(pillBg(color));
        b.setAllCaps(false);
        b.setMinHeight(0); b.setMinimumHeight(0);
        b.setPadding(dp(14), dp(6), dp(14), dp(6));
        return b;
    }

    private Button ghostButton(String label, int color) {
        Button b = new Button(this);
        b.setText(label);
        b.setTextColor(color);
        b.setTextSize(12);
        b.setBackgroundDrawable(outlineBg(OL_LINE_STRONG));
        b.setAllCaps(false);
        b.setMinHeight(0); b.setMinimumHeight(0);
        b.setPadding(dp(12), dp(5), dp(12), dp(5));
        return b;
    }

    private Button modePill(String label) {
        Button b = new Button(this);
        b.setText(label);
        b.setTextSize(12);
        b.setTypeface(Typeface.DEFAULT_BOLD);
        b.setAllCaps(false);
        b.setMinHeight(0); b.setMinimumHeight(0);
        b.setPadding(dp(10), dp(6), dp(10), dp(6));
        return b;
    }

    private CheckBox checkbox(String label, boolean checked) {
        CheckBox b = new CheckBox(this);
        b.setText(label);
        b.setTextColor(OL_INK_2);
        b.setTextSize(13);
        b.setChecked(checked);
        return b;
    }

    private EditText input(String value, String hint) {
        EditText e = new EditText(this);
        e.setText(value);
        e.setHint(hint);
        e.setHintTextColor(OL_INK_4);
        e.setTextColor(OL_INK);
        e.setSingleLine(true);
        e.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_URI);
        e.setPadding(dp(8), dp(10), dp(8), dp(10));
        e.setBackgroundDrawable(roundedBg(OL_CANVAS, 6));
        return e;
    }

    private String value(EditText editText) { return editText.getText().toString().trim(); }
    private int dp(int value) { return (int) (value * getResources().getDisplayMetrics().density + 0.5f); }
    private float dp(float value) { return value * getResources().getDisplayMetrics().density + 0.5f; }

    private interface CardBuilder { void build(LinearLayout card); }
}
