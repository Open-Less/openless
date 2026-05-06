package com.openless.android;

import android.app.Activity;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.Typeface;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.ShapeDrawable;
import android.graphics.drawable.shapes.RoundRectShape;
import android.os.Bundle;
import android.text.InputType;
import android.view.Gravity;
import android.view.View;
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

public final class SettingsActivity extends Activity {
    static final String EXTRA_INITIAL_SECTION = "openless.extra.SETTINGS_SECTION";
    private static final int SECTION_PROVIDERS = 1;
    private static final int SECTION_LANGUAGE = 2;
    private static final int SECTION_DICTATION = 3;
    private static final int SECTION_QA = 4;

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

    private static final String[] LLM_PROVIDER_IDS = new String[]{"ark", "deepseek", "siliconflow", "openai", "custom"};

    private final ExecutorService executor = Executors.newSingleThreadExecutor();
    private SettingsStore settingsStore;
    private SettingsStore.Settings settings;

    private LinearLayout providersSectionView;
    private LinearLayout languageSectionView;
    private LinearLayout dictationSectionView;
    private LinearLayout qaSectionView;
    private final ArrayList<Button> sectionButtons = new ArrayList<>();

    private android.widget.RadioButton providerVolcengine;
    private android.widget.RadioButton providerWhisper;
    private android.widget.RadioButton providerArk;
    private android.widget.RadioButton providerDeepSeek;
    private android.widget.RadioButton providerSiliconFlow;
    private android.widget.RadioButton providerOpenAi;
    private android.widget.RadioButton providerCustom;

    private LinearLayout volcSection;
    private LinearLayout whisperSection;
    private EditText volcAppKey;
    private EditText volcAccessKey;
    private EditText volcResource;
    private EditText asrBase;
    private EditText asrKey;
    private EditText asrModel;
    private EditText llmBase;
    private EditText llmKey;
    private EditText llmModel;
    private EditText workingLanguages;
    private EditText translationTarget;

    private CheckBox modeRaw;
    private CheckBox modeLight;
    private CheckBox modeStructured;
    private CheckBox modeFormal;
    private CheckBox showCapsule;
    private CheckBox clipboardFallback;
    private CheckBox qaSaveHistory;
    private TextView clipboardWarning;
    private TextView pageStatusView;
    private TextView summaryAsrView;
    private TextView summaryLlmView;
    private TextView summaryModeView;
    private TextView summaryTranslationView;
    private Button llmCheckButton;
    private Button asrCheckButton;
    private Button listModelsButton;
    private TextView diagnosticsStatusView;

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        settingsStore = new SettingsStore(this);
        settings = settingsStore.get();
        setContentView(buildContent());
        applySection(resolveInitialSection());
        refreshProviderSections();
        refreshLlmHints();
        refreshClipboardWarning();
    }

    @Override
    protected void onDestroy() {
        executor.shutdownNow();
        super.onDestroy();
    }

    @Override
    protected void onResume() {
        super.onResume();
        refreshSummary();
    }

    private int resolveInitialSection() {
        android.content.Intent intent = getIntent();
        String value = intent == null ? null : intent.getStringExtra(EXTRA_INITIAL_SECTION);
        if ("language".equalsIgnoreCase(value)) return SECTION_LANGUAGE;
        if ("dictation".equalsIgnoreCase(value)) return SECTION_DICTATION;
        if ("qa".equalsIgnoreCase(value)) return SECTION_QA;
        return SECTION_PROVIDERS;
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
        sectionNav(root);

        providersSectionView = column();
        languageSectionView = column();
        dictationSectionView = column();
        qaSectionView = column();
        root.addView(providersSectionView);
        root.addView(languageSectionView);
        root.addView(dictationSectionView);
        root.addView(qaSectionView);

        buildProvidersSection(providersSectionView);
        buildLanguageSection(languageSectionView);
        buildDictationSection(dictationSectionView);
        buildQaSection(qaSectionView);
        buildToolsSection(qaSectionView);
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
        TextView title = text("设置", 24, Typeface.BOLD);
        titleCol.addView(title);
        TextView subtitle = text("按分区维护 provider、语言、听写与问答行为。", 12, Typeface.NORMAL);
        subtitle.setTextColor(OL_INK_3);
        titleCol.addView(subtitle);
        top.addView(titleCol, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));

        Button save = pillButton("保存", OL_BLUE);
        save.setOnClickListener(v -> saveSettings());
        top.addView(save);

        root.addView(top);
        root.addView(divider());

        pageStatusView = text("就绪", 11, Typeface.BOLD);
        pageStatusView.setTextColor(OL_BLUE);
        pageStatusView.setPadding(0, dp(8), 0, 0);
        root.addView(pageStatusView);
    }

    private void overviewSection(LinearLayout root) {
        card(root, card -> {
            LinearLayout head = row();
            head.setGravity(Gravity.CENTER_VERTICAL);
            TextView title = text("配置概览", 14, Typeface.BOLD);
            title.setTextColor(OL_INK_2);
            head.addView(title, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));
            TextView badge = text("设置", 10, Typeface.BOLD);
            badge.setTextColor(OL_BLUE);
            badge.setPadding(dp(8), dp(4), dp(8), dp(4));
            badge.setBackgroundDrawable(roundedBg(OL_BLUE_SOFT, 999));
            head.addView(badge);
            card.addView(head);

            TextView desc = text("先看当前激活配置，再进入分区编辑。", 11, Typeface.NORMAL);
            desc.setTextColor(OL_INK_4);
            desc.setPadding(0, dp(4), 0, dp(10));
            card.addView(desc);

            LinearLayout topRow = row();
            topRow.addView(summaryCard("ASR"));
            topRow.addView(spacer(dp(8)));
            topRow.addView(summaryCard("LLM"));
            card.addView(topRow);

            card.addView(spacer(dp(8)));

            LinearLayout bottomRow = row();
            bottomRow.addView(summaryCard("模式"));
            bottomRow.addView(spacer(dp(8)));
            bottomRow.addView(summaryCard("翻译"));
            card.addView(bottomRow);
        });
        refreshSummary();
    }

    private void sectionNav(LinearLayout root) {
        LinearLayout navWrap = row();
        navWrap.setPadding(0, dp(14), 0, dp(12));

        LinearLayout nav = row();
        nav.setPadding(dp(2), dp(2), dp(2), dp(2));
        nav.setBackgroundDrawable(roundedBg(Color.argb(10, 0, 0, 0), 10));
        nav.addView(sectionButton("服务商", SECTION_PROVIDERS));
        nav.addView(spacer(dp(4)));
        nav.addView(sectionButton("语言", SECTION_LANGUAGE));
        nav.addView(spacer(dp(4)));
        nav.addView(sectionButton("听写", SECTION_DICTATION));
        nav.addView(spacer(dp(4)));
        nav.addView(sectionButton("问答", SECTION_QA));
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
        providersSectionView.setVisibility(section == SECTION_PROVIDERS ? View.VISIBLE : View.GONE);
        languageSectionView.setVisibility(section == SECTION_LANGUAGE ? View.VISIBLE : View.GONE);
        dictationSectionView.setVisibility(section == SECTION_DICTATION ? View.VISIBLE : View.GONE);
        qaSectionView.setVisibility(section == SECTION_QA ? View.VISIBLE : View.GONE);
        for (Button button : sectionButtons) {
            boolean active = ((Integer) button.getTag()) == section;
            button.setTextColor(active ? Color.WHITE : OL_INK_3);
            button.setBackgroundDrawable(active ? roundedBg(OL_BLUE, 8) : roundedBg(Color.TRANSPARENT, 8));
        }
    }

    private void buildProvidersSection(LinearLayout root) {
        card(root, card -> {
            TextView title = text("ASR 服务", 13, Typeface.BOLD);
            title.setTextColor(OL_INK_2);
            card.addView(title);
            TextView desc = text("切换火山流式听写与 Whisper 兼容转写。", 11, Typeface.NORMAL);
            desc.setTextColor(OL_INK_4);
            desc.setPadding(0, dp(4), 0, dp(8));
            card.addView(desc);

            android.widget.RadioGroup asrGroup = new android.widget.RadioGroup(this);
            asrGroup.setOrientation(android.widget.RadioGroup.HORIZONTAL);
            providerVolcengine = radio("火山 ASR", !"whisper".equalsIgnoreCase(settings.activeAsrProvider));
            providerWhisper = radio("Whisper 兼容", "whisper".equalsIgnoreCase(settings.activeAsrProvider));
            asrGroup.addView(providerVolcengine);
            asrGroup.addView(providerWhisper);
            card.addView(asrGroup);

            volcSection = column();
            volcSection.setPadding(0, dp(10), 0, 0);
            volcAppKey = input(settings.volcengineAppKey, "火山应用 Key");
            volcAccessKey = input(settings.volcengineAccessKey, "火山访问 Key");
            volcAccessKey.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_PASSWORD);
            volcResource = input(settings.volcengineResourceId, "火山资源 ID");
            addLabeledField(volcSection, "火山 ASR 配置", volcAppKey, volcAccessKey, volcResource);
            card.addView(volcSection);

            whisperSection = column();
            whisperSection.setPadding(0, dp(10), 0, 0);
            asrBase = input(settings.asrBaseUrl, "ASR 服务地址");
            asrKey = input(settings.asrApiKey, "ASR API Key");
            asrKey.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_PASSWORD);
            asrModel = input(settings.asrModel, "ASR 模型");
            addLabeledField(whisperSection, "Whisper 兼容 ASR 配置", asrBase, asrKey, asrModel);
            card.addView(whisperSection);

            providerVolcengine.setOnCheckedChangeListener((buttonView, isChecked) -> refreshProviderSections());
            providerWhisper.setOnCheckedChangeListener((buttonView, isChecked) -> refreshProviderSections());
        });

        card(root, card -> {
            TextView title = text("LLM 服务", 13, Typeface.BOLD);
            title.setTextColor(OL_INK_2);
            card.addView(title);
            TextView desc = text("保留 ark、deepseek、siliconflow、openai、custom 的独立状态。", 11, Typeface.NORMAL);
            desc.setTextColor(OL_INK_4);
            desc.setPadding(0, dp(4), 0, dp(8));
            card.addView(desc);

            android.widget.RadioGroup llmGroup = new android.widget.RadioGroup(this);
            llmGroup.setOrientation(android.widget.RadioGroup.VERTICAL);
            providerArk = providerOption("火山方舟 / Ark", "ark", settings.activeLlmProvider);
            providerDeepSeek = providerOption("DeepSeek", "deepseek", settings.activeLlmProvider);
            providerSiliconFlow = providerOption("SiliconFlow", "siliconflow", settings.activeLlmProvider);
            providerOpenAi = providerOption("OpenAI 兼容", "openai", settings.activeLlmProvider);
            providerCustom = providerOption("自定义", "custom", settings.activeLlmProvider);
            llmGroup.addView(providerArk);
            llmGroup.addView(providerDeepSeek);
            llmGroup.addView(providerSiliconFlow);
            llmGroup.addView(providerOpenAi);
            llmGroup.addView(providerCustom);
            card.addView(llmGroup);

            llmBase = input(settings.llmBaseUrl, llmBaseHint(normalizeLlmProvider(settings.activeLlmProvider)));
            llmKey = input(settings.llmApiKey, "LLM API Key");
            llmKey.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_PASSWORD);
            llmModel = input(settings.llmModel, llmModelHint(normalizeLlmProvider(settings.activeLlmProvider)));
            addLabeledField(card, "LLM 配置", llmBase, llmKey, llmModel);

            llmGroup.setOnCheckedChangeListener((group, checkedId) -> {
                refreshLlmHints();
                String selected = selectedLlmProvider();
                String currentBase = value(llmBase);
                String presetBase = llmBasePreset(selected);
                if (!presetBase.isEmpty() && (currentBase.isEmpty() || isKnownLlmBase(currentBase))) {
                    llmBase.setText(presetBase);
                }
            });
        });

        card(root, card -> {
            TextView title = text("服务商诊断", 13, Typeface.BOLD);
            title.setTextColor(OL_INK_2);
            card.addView(title);
            TextView desc = text("保存前后都可以直接检查 LLM / ASR 配置有效性。", 11, Typeface.NORMAL);
            desc.setTextColor(OL_INK_4);
            desc.setPadding(0, dp(4), 0, dp(8));
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

            diagnosticsStatusView = text("就绪", 11, Typeface.NORMAL);
            diagnosticsStatusView.setTextColor(OL_INK_4);
            diagnosticsStatusView.setPadding(0, dp(8), 0, 0);
            card.addView(diagnosticsStatusView);
        });
    }

    private void buildLanguageSection(LinearLayout root) {
        card(root, card -> {
            TextView title = text("工作语言", 13, Typeface.BOLD);
            title.setTextColor(OL_INK_2);
            card.addView(title);
            TextView desc = text("工作语言参与润色和翻译判断；支持逗号或换行分隔。", 11, Typeface.NORMAL);
            desc.setTextColor(OL_INK_4);
            desc.setPadding(0, dp(4), 0, dp(8));
            card.addView(desc);

            workingLanguages = input(stringsText(settings.workingLanguages), "简体中文, English");
            workingLanguages.setSingleLine(false);
            workingLanguages.setMinLines(3);
            card.addView(workingLanguages);
        });

        card(root, card -> {
            TextView title = text("翻译目标", 13, Typeface.BOLD);
            title.setTextColor(OL_INK_2);
            card.addView(title);
            TextView desc = text("主界面的翻译按钮会直接读取这里的目标语言。", 11, Typeface.NORMAL);
            desc.setTextColor(OL_INK_4);
            desc.setPadding(0, dp(4), 0, dp(8));
            card.addView(desc);

            translationTarget = input(settings.translationTargetLanguage, "例如：日语 / English");
            card.addView(translationTarget);
        });
    }

    private void buildDictationSection(LinearLayout root) {
        card(root, card -> {
            TextView title = text("润色模式", 13, Typeface.BOLD);
            title.setTextColor(OL_INK_2);
            card.addView(title);
            TextView desc = text("禁用后，不再出现在主界面的模式切换里。", 11, Typeface.NORMAL);
            desc.setTextColor(OL_INK_4);
            desc.setPadding(0, dp(4), 0, dp(8));
            card.addView(desc);

            modeRaw = checkbox("原文", settings.enabledModes.contains(PolishMode.RAW));
            modeLight = checkbox("轻润色", settings.enabledModes.contains(PolishMode.LIGHT));
            modeStructured = checkbox("结构化", settings.enabledModes.contains(PolishMode.STRUCTURED));
            modeFormal = checkbox("正式", settings.enabledModes.contains(PolishMode.FORMAL));
            card.addView(modeRaw);
            card.addView(modeLight);
            card.addView(modeStructured);
            card.addView(modeFormal);
        });

        card(root, card -> {
            TextView title = text("插入与悬浮", 13, Typeface.BOLD);
            title.setTextColor(OL_INK_2);
            card.addView(title);
            TextView desc = text("控制悬浮胶囊显示与插入失败时的剪贴板兜底。", 11, Typeface.NORMAL);
            desc.setTextColor(OL_INK_4);
            desc.setPadding(0, dp(4), 0, dp(8));
            card.addView(desc);

            showCapsule = checkbox("显示悬浮胶囊", settings.showCapsule);
            clipboardFallback = checkbox("允许剪贴板兜底", settings.allowClipboardFallback);
            card.addView(showCapsule);
            card.addView(clipboardFallback);

            clipboardWarning = text("关闭剪贴板兜底后，非 OpenLess 键盘路径下的文本插入失败将不会自动补救。", 11, Typeface.NORMAL);
            clipboardWarning.setTextColor(OL_ERR);
            clipboardWarning.setPadding(dp(20), dp(6), dp(12), 0);
            card.addView(clipboardWarning);

            clipboardFallback.setOnCheckedChangeListener((buttonView, isChecked) -> refreshClipboardWarning());
        });
    }

    private void buildQaSection(LinearLayout root) {
        card(root, card -> {
            TextView title = text("问答历史", 13, Typeface.BOLD);
            title.setTextColor(OL_INK_2);
            card.addView(title);
            TextView desc = text("控制语音提问、多轮对话和历史转问答的数据保留。", 11, Typeface.NORMAL);
            desc.setTextColor(OL_INK_4);
            desc.setPadding(0, dp(4), 0, dp(8));
            card.addView(desc);

            qaSaveHistory = checkbox("保存问答到历史记录", settings.qaSaveHistory);
            card.addView(qaSaveHistory);
        });
    }

    private void buildToolsSection(LinearLayout root) {
        card(root, card -> {
            TextView title = text("相关工具", 13, Typeface.BOLD);
            title.setTextColor(OL_INK_2);
            card.addView(title);
            TextView desc = text("从设置页直接跳到词典、问答或系统输入法设置。", 11, Typeface.NORMAL);
            desc.setTextColor(OL_INK_4);
            desc.setPadding(0, dp(4), 0, dp(8));
            card.addView(desc);

            LinearLayout topRow = row();
            Button dictionary = ghostButton("词典", OL_INK_2);
            dictionary.setOnClickListener(v -> startActivity(new android.content.Intent(this, DictionaryActivity.class)));
            topRow.addView(dictionary, new LinearLayout.LayoutParams(0, dp(40), 1));
            topRow.addView(spacer(dp(8)));
            Button qa = ghostButton("问答面板", OL_BLUE);
            qa.setOnClickListener(v -> startActivity(new android.content.Intent(this, QaPanelActivity.class)));
            topRow.addView(qa, new LinearLayout.LayoutParams(0, dp(40), 1));
            card.addView(topRow);

            card.addView(spacer(dp(8)));

            Button ime = ghostButton("输入法设置", OL_BLUE);
            ime.setOnClickListener(v -> startActivity(new android.content.Intent(android.provider.Settings.ACTION_INPUT_METHOD_SETTINGS)));
            card.addView(ime, new LinearLayout.LayoutParams(LinearLayout.LayoutParams.MATCH_PARENT, dp(40)));
        });
    }

    private void refreshProviderSections() {
        boolean whisper = providerWhisper != null && providerWhisper.isChecked();
        if (whisperSection != null) {
            whisperSection.setVisibility(whisper ? View.VISIBLE : View.GONE);
        }
        if (volcSection != null) {
            volcSection.setVisibility(whisper ? View.GONE : View.VISIBLE);
        }
    }

    private void refreshLlmHints() {
        if (llmBase == null || llmModel == null) return;
        String selected = selectedLlmProvider();
        llmBase.setHint(llmBaseHint(selected));
        llmModel.setHint(llmModelHint(selected));
    }

    private void refreshClipboardWarning() {
        if (clipboardWarning == null || clipboardFallback == null) return;
        clipboardWarning.setVisibility(clipboardFallback.isChecked() ? View.GONE : View.VISIBLE);
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
        if ("ASR".equals(label)) {
            summaryAsrView = value;
        } else if ("LLM".equals(label)) {
            summaryLlmView = value;
        } else if ("模式".equals(label)) {
            summaryModeView = value;
        } else if ("翻译".equals(label)) {
            summaryTranslationView = value;
        }
        return box;
    }

    private void refreshSummary() {
        SettingsStore.Settings current = collectDraftSettings();
        if (summaryAsrView != null) {
            summaryAsrView.setText("whisper".equalsIgnoreCase(current.activeAsrProvider) ? "Whisper 兼容" : "火山流式");
        }
        if (summaryLlmView != null) {
            summaryLlmView.setText(providerDisplayName(current.activeLlmProvider));
        }
        if (summaryModeView != null) {
            summaryModeView.setText(enabledModesSummary(current.enabledModes));
        }
        if (summaryTranslationView != null) {
            String target = current.translationTargetLanguage == null ? "" : current.translationTargetLanguage.trim();
            summaryTranslationView.setText(target.isEmpty() ? "未配置" : target);
            summaryTranslationView.setTextColor(target.isEmpty() ? OL_WARN : OL_BLUE);
        }
    }

    private void saveSettings() {
        settings.activeAsrProvider = providerWhisper.isChecked() ? "whisper" : "volcengine";
        settings.activeLlmProvider = selectedLlmProvider();
        settings.volcengineAppKey = value(volcAppKey);
        settings.volcengineAccessKey = value(volcAccessKey);
        settings.volcengineResourceId = value(volcResource).isEmpty() ? VolcengineAsrProvider.DEFAULT_RESOURCE_ID : value(volcResource);
        settings.asrBaseUrl = value(asrBase);
        settings.asrApiKey = value(asrKey);
        settings.asrModel = value(asrModel);
        settings.llmBaseUrl = value(llmBase);
        settings.llmApiKey = value(llmKey);
        settings.llmModel = value(llmModel);

        ArrayList<PolishMode> selectedModes = new ArrayList<>();
        if (modeRaw.isChecked()) selectedModes.add(PolishMode.RAW);
        if (modeLight.isChecked()) selectedModes.add(PolishMode.LIGHT);
        if (modeStructured.isChecked()) selectedModes.add(PolishMode.STRUCTURED);
        if (modeFormal.isChecked()) selectedModes.add(PolishMode.FORMAL);
        if (selectedModes.isEmpty()) selectedModes.add(PolishMode.LIGHT);
        settings.enabledModes = selectedModes;
        if (!settings.enabledModes.contains(settings.mode)) {
            settings.mode = settings.enabledModes.get(0);
        }

        settings.workingLanguages = parseStrings(value(workingLanguages));
        settings.translationTargetLanguage = value(translationTarget);
        settings.showCapsule = showCapsule.isChecked();
        settings.allowClipboardFallback = clipboardFallback.isChecked();
        settings.qaSaveHistory = qaSaveHistory.isChecked();

        settingsStore.save(settings);
        refreshFloatingServiceSettings();
        setPageStatus("已保存并同步悬浮窗设置", OL_OK);
        setResult(RESULT_OK);
        finish();
    }

    private void runLlmCheck() {
        settings = collectDraftSettings();
        setDiagnosticsBusy(true, "正在检测 LLM...");
        executor.execute(() -> {
            try {
                String result = ProviderDiagnostics.validateLlm(settings);
                runOnUiThread(() -> {
                    setDiagnosticsBusy(false, result, OL_OK);
                });
            } catch (Exception e) {
                runOnUiThread(() -> {
                    setDiagnosticsBusy(false, fallbackMessage(e.getMessage(), "LLM 检测失败"), OL_ERR);
                });
            }
        });
    }

    private void runAsrCheck() {
        settings = collectDraftSettings();
        setDiagnosticsBusy(true, "正在检测 ASR...");
        executor.execute(() -> {
            try {
                String result = ProviderDiagnostics.validateAsr(settings);
                runOnUiThread(() -> {
                    setDiagnosticsBusy(false, result, OL_OK);
                });
            } catch (Exception e) {
                runOnUiThread(() -> {
                    setDiagnosticsBusy(false, fallbackMessage(e.getMessage(), "ASR 检测失败"), OL_ERR);
                });
            }
        });
    }

    private void runListModels() {
        settings = collectDraftSettings();
        setDiagnosticsBusy(true, "正在加载模型...");
        executor.execute(() -> {
            try {
                List<String> models = ProviderDiagnostics.listModels(settings.llmBaseUrl, settings.llmApiKey);
                runOnUiThread(() -> {
                    setDiagnosticsBusy(false, "模型列表已返回");
                    android.content.Intent intent = new android.content.Intent(this, ModelListActivity.class);
                    intent.putStringArrayListExtra(ModelListActivity.EXTRA_MODELS, new ArrayList<>(models));
                    startActivity(intent);
                });
            } catch (Exception e) {
                runOnUiThread(() -> {
                    setDiagnosticsBusy(false, fallbackMessage(e.getMessage(), "模型加载失败"), OL_ERR);
                });
            }
        });
    }

    private void setDiagnosticsBusy(boolean busy, String message) {
        setDiagnosticsBusy(busy, message, busy ? OL_WARN : OL_INK_4);
    }

    private void setDiagnosticsBusy(boolean busy, String message, int color) {
        if (llmCheckButton != null) {
            llmCheckButton.setEnabled(!busy);
            llmCheckButton.setAlpha(busy ? 0.6f : 1f);
        }
        if (asrCheckButton != null) {
            asrCheckButton.setEnabled(!busy);
            asrCheckButton.setAlpha(busy ? 0.6f : 1f);
        }
        if (listModelsButton != null) {
            listModelsButton.setEnabled(!busy);
            listModelsButton.setAlpha(busy ? 0.6f : 1f);
        }
        if (diagnosticsStatusView != null) {
            diagnosticsStatusView.setText(message);
            diagnosticsStatusView.setTextColor(color);
        }
    }

    private SettingsStore.Settings collectDraftSettings() {
        SettingsStore.Settings draft = settingsStore.get();
        if (providerWhisper == null
                || volcAppKey == null
                || volcAccessKey == null
                || volcResource == null
                || asrBase == null
                || asrKey == null
                || asrModel == null
                || llmBase == null
                || llmKey == null
                || llmModel == null
                || workingLanguages == null
                || translationTarget == null
                || showCapsule == null
                || clipboardFallback == null
                || qaSaveHistory == null) {
            return draft;
        }
        draft.activeAsrProvider = providerWhisper.isChecked() ? "whisper" : "volcengine";
        draft.activeLlmProvider = selectedLlmProvider();
        draft.volcengineAppKey = value(volcAppKey);
        draft.volcengineAccessKey = value(volcAccessKey);
        draft.volcengineResourceId = value(volcResource).isEmpty() ? VolcengineAsrProvider.DEFAULT_RESOURCE_ID : value(volcResource);
        draft.asrBaseUrl = value(asrBase);
        draft.asrApiKey = value(asrKey);
        draft.asrModel = value(asrModel);
        draft.llmBaseUrl = value(llmBase);
        draft.llmApiKey = value(llmKey);
        draft.llmModel = value(llmModel);
        draft.translationTargetLanguage = value(translationTarget);
        draft.workingLanguages = parseStrings(value(workingLanguages));
        draft.showCapsule = showCapsule.isChecked();
        draft.allowClipboardFallback = clipboardFallback.isChecked();
        draft.qaSaveHistory = qaSaveHistory.isChecked();
        if (modeRaw != null && modeLight != null && modeStructured != null && modeFormal != null) {
            ArrayList<PolishMode> selectedModes = new ArrayList<>();
            if (modeRaw.isChecked()) selectedModes.add(PolishMode.RAW);
            if (modeLight.isChecked()) selectedModes.add(PolishMode.LIGHT);
            if (modeStructured.isChecked()) selectedModes.add(PolishMode.STRUCTURED);
            if (modeFormal.isChecked()) selectedModes.add(PolishMode.FORMAL);
            if (!selectedModes.isEmpty()) {
                draft.enabledModes = selectedModes;
                if (!draft.enabledModes.contains(draft.mode)) {
                    draft.mode = draft.enabledModes.get(0);
                }
            }
        }
        return draft;
    }

    private void setPageStatus(String message, int color) {
        if (pageStatusView == null) return;
        pageStatusView.setText(message);
        pageStatusView.setTextColor(color);
    }

    private String fallbackMessage(String message, String fallback) {
        return message == null || message.trim().isEmpty() ? fallback : message;
    }

    private void refreshFloatingServiceSettings() {
        android.content.Intent intent = new android.content.Intent(this, FloatingTriggerService.class);
        intent.setAction(FloatingTriggerService.ACTION_REFRESH_SETTINGS);
        if (android.os.Build.VERSION.SDK_INT >= 26) {
            startForegroundService(intent);
        } else {
            startService(intent);
        }
    }

    private android.widget.RadioButton radio(String label, boolean checked) {
        android.widget.RadioButton button = new android.widget.RadioButton(this);
        button.setText(label);
        button.setTextColor(OL_INK_2);
        button.setChecked(checked);
        return button;
    }

    private android.widget.RadioButton providerOption(String label, String value, String selected) {
        android.widget.RadioButton button = radio(label, value.equals(normalizeLlmProvider(selected)));
        button.setTag(value);
        return button;
    }

    private void addLabeledField(LinearLayout root, String title, EditText... fields) {
        TextView label = text(title, 12, Typeface.BOLD);
        label.setTextColor(OL_INK_2);
        root.addView(label);
        root.addView(spacer(dp(4)));
        for (int i = 0; i < fields.length; i++) {
            root.addView(fields[i]);
            if (i < fields.length - 1) {
                root.addView(spacer(dp(6)));
            }
        }
    }

    private String selectedLlmProvider() {
        if (providerDeepSeek != null && providerDeepSeek.isChecked()) return "deepseek";
        if (providerSiliconFlow != null && providerSiliconFlow.isChecked()) return "siliconflow";
        if (providerOpenAi != null && providerOpenAi.isChecked()) return "openai";
        if (providerCustom != null && providerCustom.isChecked()) return "custom";
        return "ark";
    }

    private String providerDisplayName(String value) {
        String provider = normalizeLlmProvider(value);
        if ("ark".equals(provider)) return "Ark";
        if ("deepseek".equals(provider)) return "DeepSeek";
        if ("siliconflow".equals(provider)) return "SiliconFlow";
        if ("openai".equals(provider)) return "OpenAI";
        return "自定义";
    }

    private String enabledModesSummary(List<PolishMode> modes) {
        if (modes == null || modes.isEmpty()) return "轻润色";
        if (modes.size() == 1) return modes.get(0).label;
        return modes.size() + " 个已启用";
    }

    private String normalizeLlmProvider(String value) {
        if (value == null) return "ark";
        for (String id : LLM_PROVIDER_IDS) {
            if (id.equalsIgnoreCase(value)) return id;
        }
        return "custom";
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

    private List<String> parseStrings(String raw) {
        ArrayList<String> out = new ArrayList<>();
        if (raw == null) return out;
        String[] parts = raw.split("[,，\n]");
        for (String part : parts) {
            String value = part == null ? "" : part.trim();
            if (!value.isEmpty() && !out.contains(value)) out.add(value);
        }
        if (out.isEmpty()) out.add("简体中文");
        return out;
    }

    private String stringsText(List<String> values) {
        StringBuilder sb = new StringBuilder();
        if (values != null) {
            for (String value : values) {
                if (value == null || value.trim().isEmpty()) continue;
                if (sb.length() > 0) sb.append(',');
                sb.append(value.trim());
            }
        }
        return sb.toString();
    }

    private String value(EditText editText) {
        return editText == null || editText.getText() == null ? "" : editText.getText().toString().trim();
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
        TextView v = new TextView(this);
        v.setText(value);
        v.setTextColor(OL_INK);
        v.setTextSize(sp);
        v.setTypeface(Typeface.DEFAULT, style);
        v.setLineSpacing(0, 1.2f);
        return v;
    }

    private EditText input(String value, String hint) {
        EditText editText = new EditText(this);
        editText.setText(value == null ? "" : value);
        editText.setHint(hint);
        editText.setHintTextColor(OL_INK_4);
        editText.setTextColor(OL_INK);
        editText.setBackgroundDrawable(roundedBg(OL_CANVAS, 10));
        editText.setPadding(dp(12), dp(10), dp(12), dp(10));
        return editText;
    }

    private CheckBox checkbox(String label, boolean checked) {
        CheckBox box = new CheckBox(this);
        box.setText(label);
        box.setTextColor(OL_INK_2);
        box.setChecked(checked);
        return box;
    }

    private Button pillButton(String label, int bgColor) {
        Button button = new Button(this);
        button.setText(label);
        button.setAllCaps(false);
        button.setTextColor(Color.WHITE);
        button.setTextSize(12);
        button.setTypeface(Typeface.DEFAULT_BOLD);
        button.setBackgroundDrawable(pillBg(bgColor));
        button.setPadding(dp(16), dp(8), dp(16), dp(8));
        return button;
    }

    private Button ghostButton(String label, int inkColor) {
        Button button = new Button(this);
        button.setText(label);
        button.setAllCaps(false);
        button.setTextColor(inkColor);
        button.setTextSize(12);
        button.setTypeface(Typeface.DEFAULT_BOLD);
        button.setBackgroundDrawable(outlineBg(OL_LINE_STRONG));
        button.setPadding(dp(14), dp(8), dp(14), dp(8));
        return button;
    }

    private int dp(float value) {
        return Math.round(getResources().getDisplayMetrics().density * value);
    }

    private interface CardBuilder {
        void build(LinearLayout card);
    }
}
