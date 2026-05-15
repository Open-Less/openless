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
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

import java.util.List;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

// UI-only changes: refined bubble styling, card borders, typography, spacing
// All functionality preserved: voice QA, streaming answers, placeholder messages, clipboard QA
// All Chinese text preserved
public final class QaPanelActivity extends Activity {
    static final String EXTRA_CONTEXT = "openless.extra.QA_CONTEXT";
    private static final int REQ_AUDIO = 71;

    // OpenLess design tokens
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

    private final ExecutorService executor = Executors.newSingleThreadExecutor();
    private final AudioRecorder recorder = new AudioRecorder();

    private SettingsStore settingsStore;
    private HistoryStore historyStore;
    private DictionaryStore dictionaryStore;
    private QaSessionStore sessionStore;
    private TextView status;
    private TextView qaContextStateView;
    private TextView qaMessageCountView;
    private TextView qaHistoryStateView;
    private LinearLayout messagesList;
    private EditText contextInput;
    private EditText questionInput;
    private Button askButton;
    private Button voiceButton;
    private VolcengineStreamingSession qaVolcengineSession;
    private ScrollView scrollView;

    @Override
    protected void onCreate(Bundle bundle) {
        super.onCreate(bundle);
        settingsStore = new SettingsStore(this);
        historyStore = new HistoryStore(this);
        dictionaryStore = new DictionaryStore(this);
        sessionStore = QaSessionStore.get();
        String initialContext = getIntent() == null ? null : getIntent().getStringExtra(EXTRA_CONTEXT);
        if (initialContext != null && !initialContext.trim().isEmpty()) {
            sessionStore.startNewContext(initialContext);
        }
        setContentView(buildContent());
        refreshMessages();
    }

    @Override
    protected void onDestroy() {
        recorder.stop();
        if (qaVolcengineSession != null) {
            qaVolcengineSession.close();
            qaVolcengineSession = null;
        }
        executor.shutdownNow();
        super.onDestroy();
    }

    @Override
    protected void onResume() {
        super.onResume();
        refreshQaOverview();
    }

    private View buildContent() {
        scrollView = new ScrollView(this);
        scrollView.setFillViewport(true);
        scrollView.setBackgroundColor(OL_CANVAS);

        LinearLayout root = column();
        root.setPadding(dp(16), dp(16), dp(16), dp(24));
        scrollView.addView(root);

        headerSection(root);
        overviewSection(root);
        inputSection(root);
        conversationSection(root);
        return scrollView;
    }

    private void headerSection(LinearLayout root) {
        LinearLayout top = row();
        top.setGravity(Gravity.CENTER_VERTICAL);
        top.setPadding(0, dp(8), 0, dp(8));
        Button backButton = ghostButton("返回", OL_INK_2);
        backButton.setOnClickListener(v -> finish());
        top.addView(backButton);
        top.addView(spacer(dp(8)));

        LinearLayout titleCol = column();
        TextView title = text("问答面板", 24, Typeface.BOLD);
        titleCol.addView(title);
        TextView subtitle = text("可粘贴上下文，也可直接语音追问。", 12, Typeface.NORMAL);
        subtitle.setTextColor(OL_INK_3);
        titleCol.addView(subtitle);

        top.addView(titleCol, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));

        status = text("就绪", 12, Typeface.BOLD);
        status.setGravity(Gravity.END);
        status.setTextColor(OL_BLUE);
        top.addView(status);
        top.addView(spacer(dp(8)));
        Button settingsButton = ghostButton("设置", OL_BLUE);
        settingsButton.setOnClickListener(v -> {
            android.content.Intent intent = new android.content.Intent(this, SettingsActivity.class);
            intent.putExtra(SettingsActivity.EXTRA_INITIAL_SECTION, "qa");
            startActivity(intent);
        });
        top.addView(settingsButton);

        root.addView(top);
        root.addView(divider());
        root.addView(spacer(dp(8)));
    }

    private void overviewSection(LinearLayout root) {
        card(root, card -> {
            LinearLayout head = row();
            head.setGravity(Gravity.CENTER_VERTICAL);
            TextView title = text("会话概览", 14, Typeface.BOLD);
            title.setTextColor(OL_INK_2);
            head.addView(title, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));
            TextView badge = text("问答", 10, Typeface.BOLD);
            badge.setTextColor(OL_BLUE);
            badge.setPadding(dp(8), dp(4), dp(8), dp(4));
            badge.setBackgroundDrawable(roundedBg(OL_BLUE_SOFT, 999));
            head.addView(badge);
            card.addView(head);

            TextView desc = text("集中展示上下文、消息数量和历史写入状态。", 11, Typeface.NORMAL);
            desc.setTextColor(OL_INK_4);
            desc.setPadding(0, dp(4), 0, dp(10));
            card.addView(desc);

            LinearLayout topRow = row();
            topRow.addView(qaMetricCard("上下文"));
            topRow.addView(spacer(dp(8)));
            topRow.addView(qaMetricCard("消息"));
            card.addView(topRow);

            card.addView(spacer(dp(8)));

            LinearLayout bottomRow = row();
            bottomRow.addView(qaMetricCard("历史写入"));
            bottomRow.addView(spacer(dp(8)));
            Button newSessionButton = ghostButton("新会话", OL_BLUE);
            newSessionButton.setOnClickListener(v -> {
                sessionStore.clear();
                if (contextInput != null) contextInput.setText("");
                if (questionInput != null) questionInput.setText("");
                refreshMessages();
                setStatus("已开始新会话", OL_BLUE);
            });
            bottomRow.addView(newSessionButton, new LinearLayout.LayoutParams(0, dp(44), 1));
            card.addView(bottomRow);
            card.addView(spacer(dp(8)));

            LinearLayout toolsRow = row();
            Button dictionaryButton = ghostButton("词典", OL_INK_2);
            dictionaryButton.setOnClickListener(v -> startActivity(new android.content.Intent(this, DictionaryActivity.class)));
            toolsRow.addView(dictionaryButton, new LinearLayout.LayoutParams(0, dp(40), 1));
            toolsRow.addView(spacer(dp(8)));
            Button copyButton = ghostButton("复制对话", OL_BLUE);
            copyButton.setOnClickListener(v -> copyConversation());
            toolsRow.addView(copyButton, new LinearLayout.LayoutParams(0, dp(40), 1));
            card.addView(toolsRow);
        });
        refreshQaOverview();
    }

    private void inputSection(LinearLayout root) {
        card(root, card -> {
            TextView contextLabel = text("上下文", 12, Typeface.BOLD);
            contextLabel.setTextColor(OL_INK_2);
            card.addView(contextLabel);

            contextInput = input(sessionStore.contextText(), "可选：粘贴上下文或选中的文本");
            contextInput.setMinLines(3);
            contextInput.setMaxLines(6);
            contextInput.setGravity(Gravity.TOP | Gravity.START);
            contextInput.setSingleLine(false);
            contextInput.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_MULTI_LINE);
            card.addView(contextInput);

            card.addView(spacer(dp(10)));

            TextView questionLabel = text("问题", 12, Typeface.BOLD);
            questionLabel.setTextColor(OL_INK_2);
            card.addView(questionLabel);

            questionInput = input("", "围绕上面的内容提出问题");
            questionInput.setMinLines(2);
            questionInput.setMaxLines(5);
            questionInput.setGravity(Gravity.TOP | Gravity.START);
            questionInput.setSingleLine(false);
            questionInput.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_MULTI_LINE);
            card.addView(questionInput);

            card.addView(spacer(dp(10)));

            LinearLayout actions = row();
            askButton = pillButton("提问");
            askButton.setOnClickListener(v -> submitQuestion());
            actions.addView(askButton, new LinearLayout.LayoutParams(0, dp(44), 1));
            actions.addView(spacer(dp(8)));

            voiceButton = ghostButton("按住提问", OL_BLUE);
            voiceButton.setOnTouchListener((view, event) -> {
                if (event.getAction() == android.view.MotionEvent.ACTION_DOWN) {
                    beginVoiceQuestion();
                    return true;
                }
                if (event.getAction() == android.view.MotionEvent.ACTION_UP
                        || event.getAction() == android.view.MotionEvent.ACTION_CANCEL) {
                    endVoiceQuestion();
                    return true;
                }
                return true;
            });
            actions.addView(voiceButton, new LinearLayout.LayoutParams(0, dp(44), 1));
            actions.addView(spacer(dp(8)));

            Button clearButton = ghostButton("清空", OL_INK_3);
            clearButton.setOnClickListener(v -> {
                sessionStore.clear();
                contextInput.setText("");
                questionInput.setText("");
                refreshMessages();
                setStatus("已清空", OL_BLUE);
            });
            actions.addView(clearButton, new LinearLayout.LayoutParams(0, dp(44), 1));
            card.addView(actions);

            card.addView(spacer(dp(8)));

            Button copyAllButton = ghostButton("复制全部", OL_BLUE);
            copyAllButton.setOnClickListener(v -> copyConversation());
            card.addView(copyAllButton);
        });
    }

    // conversationSection with visual bubble styling
    private void conversationSection(LinearLayout root) {
        card(root, card -> {
            LinearLayout headerRow = row();
            headerRow.setGravity(Gravity.CENTER_VERTICAL);

            TextView label = text("对话", 13, Typeface.BOLD);
            label.setTextColor(OL_INK_2);
            headerRow.addView(label, new LinearLayout.LayoutParams(0, LinearLayout.LayoutParams.WRAP_CONTENT, 1));

            Button scrollBottom = ghostButton("滚动到底部", OL_BLUE);
            scrollBottom.setOnClickListener(v -> {
                if (scrollView != null) {
                    scrollView.fullScroll(ScrollView.FOCUS_DOWN);
                }
            });
            headerRow.addView(scrollBottom);
            card.addView(headerRow);
            card.addView(spacer(dp(6)));

            messagesList = column();
            card.addView(messagesList);
        });
    }

    // ─── All functional logic preserved below ───────────────────────

    private void submitQuestion() {
        String question = value(questionInput);
        submitQuestionText(question);
    }

    private void submitQuestionText(String question) {
        String context = value(contextInput);
        if (question.isEmpty()) {
            setStatus("请先输入问题", OL_WARN);
            return;
        }

        sessionStore.setContextText(context);
        String userContent = question;
        if (!context.isEmpty() && sessionStore.messages().isEmpty()) {
            userContent = "上下文：\n" + context + "\n\n问题：\n" + question;
        }
        final String finalUserContent = userContent;
        sessionStore.addUser(finalUserContent);
        sessionStore.addAssistantPlaceholder();
        questionInput.setText("");
        refreshMessages();
        setBusy(true, "回答生成中...");

        executor.execute(() -> {
            try {
                List<QaChatMessage> messages = sessionStore.messages();
                String answer = new QaAnswerProvider(settingsStore).answerStreaming(messages, partial -> {
                    sessionStore.replaceLastAssistant(partial);
                    runOnUiThread(() -> {
                        refreshMessages();
                        if (scrollView != null) {
                            scrollView.post(() -> scrollView.fullScroll(ScrollView.FOCUS_DOWN));
                        }
                    });
                });
                sessionStore.replaceLastAssistant(answer);
                maybeSaveQaHistory(finalUserContent, answer);
                runOnUiThread(() -> {
                    refreshMessages();
                    if (scrollView != null) {
                        scrollView.postDelayed(() -> scrollView.fullScroll(ScrollView.FOCUS_DOWN), 80);
                    }
                    setBusy(false, "已回答");
                });
            } catch (Exception e) {
                runOnUiThread(() -> {
                    sessionStore.removeLastAssistantIfEmpty();
                    refreshMessages();
                    setBusy(false, "出错");
                    showError(e);
                });
            }
        });
    }

    // ─── Voice QA (fully preserved) ──────────────────────────────────

    private void beginVoiceQuestion() {
        if (!ensureAudioPermission() || recorder.isRecording()) {
            return;
        }
        try {
            SettingsStore.Settings settings = settingsStore.get();
            if (!"whisper".equals(settings.activeAsrProvider)) {
                qaVolcengineSession = new VolcengineStreamingSession(settings, dictionaryStore.enabledPhrases());
                qaVolcengineSession.open();
                recorder.start((pcm, length) -> {
                    VolcengineStreamingSession session = qaVolcengineSession;
                    if (session != null) {
                        session.consume(pcm, length);
                    }
                });
            } else {
                recorder.start();
            }
            setStatus("正在聆听问题...", OL_BLUE);
            if (voiceButton != null) {
                voiceButton.setText("松开结束");
            }
        } catch (Exception e) {
            showError(e);
        }
    }

    private void endVoiceQuestion() {
        if (!recorder.isRecording()) {
            return;
        }
        AudioRecorder.Recording recording = recorder.stop();
        if (voiceButton != null) {
            voiceButton.setText("按住提问");
        }
        if (recording.pcm.length < 1000) {
            setStatus("问题过短", OL_ERR);
            return;
        }
        setBusy(true, "正在转写问题...");
        VolcengineStreamingSession session = qaVolcengineSession;
        qaVolcengineSession = null;
        if (session != null) {
            executor.execute(() -> finishVoiceQuestion(session, recording.durationMs));
        } else {
            executor.execute(() -> transcribeWhisperQuestion(recording));
        }
    }

    private void finishVoiceQuestion(VolcengineStreamingSession session, long durationMs) {
        try {
            RawTranscript raw = session.finish(durationMs);
            session.close();
            handleVoiceTranscript(raw);
        } catch (Exception e) {
            session.close();
            runOnUiThread(() -> {
                setBusy(false, "出错");
                showError(e);
            });
        }
    }

    private void transcribeWhisperQuestion(AudioRecorder.Recording recording) {
        try {
            RawTranscript raw = new WhisperAsrProvider(settingsStore.get()).transcribe(recording);
            handleVoiceTranscript(raw);
        } catch (Exception e) {
            runOnUiThread(() -> {
                setBusy(false, "出错");
                showError(e);
            });
        }
    }

    private void handleVoiceTranscript(RawTranscript raw) {
        final String question = raw == null || raw.text == null ? "" : raw.text.trim();
        runOnUiThread(() -> {
            if (question.isEmpty()) {
                setBusy(false, "出错");
                setStatus("语音识别没有返回有效问题", OL_ERR);
                return;
            }
            questionInput.setText(question);
            setBusy(false, "已捕获问题");
            submitQuestionText(question);
        });
    }

    private void maybeSaveQaHistory(String question, String answer) {
        SettingsStore.Settings settings = settingsStore.get();
        if (!settings.qaSaveHistory) {
            return;
        }
        historyStore.add(question, answer, settings.mode, null, null,
                InsertStatus.COPIED_FALLBACK, "qaSession", 0, null);
    }

    // ─── Message rendering with visual refinement ───────────────────

    private LinearLayout qaMetricCard(String label) {
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
        if ("上下文".equals(label)) {
            qaContextStateView = value;
        } else if ("消息".equals(label)) {
            qaMessageCountView = value;
        } else if ("历史写入".equals(label)) {
            qaHistoryStateView = value;
        }
        return box;
    }

    private void refreshQaOverview() {
        List<QaChatMessage> messages = sessionStore.messages();
        String context = sessionStore.contextText();
        if (qaContextStateView != null) {
            boolean hasContext = context != null && !context.trim().isEmpty();
            qaContextStateView.setText(hasContext ? "已附带" : "未附带");
            qaContextStateView.setTextColor(hasContext ? OL_BLUE : OL_WARN);
        }
        if (qaMessageCountView != null) {
            qaMessageCountView.setText(String.valueOf(messages.size()) + " 条");
        }
        if (qaHistoryStateView != null) {
            boolean enabled = settingsStore.get().qaSaveHistory;
            qaHistoryStateView.setText(enabled ? "开启" : "关闭");
            qaHistoryStateView.setTextColor(enabled ? OL_OK : OL_INK_3);
        }
    }

    private void refreshMessages() {
        messagesList.removeAllViews();
        List<QaChatMessage> messages = sessionStore.messages();
        refreshQaOverview();
        if (messages.isEmpty()) {
            TextView empty = text("还没有对话内容。", 13, Typeface.NORMAL);
            empty.setTextColor(OL_INK_3);
            messagesList.addView(empty);
            return;
        }
        for (QaChatMessage message : messages) {
            boolean isUser = "user".equals(message.role);

            // Bubble container
            LinearLayout bubble = column();
            bubble.setPadding(dp(12), dp(10), dp(12), dp(10));
            bubble.setBackgroundDrawable(roundedBg(isUser ? OL_BLUE_SOFT : OL_SURFACE, 10));

            // Role label
            TextView roleLabel = new TextView(this);
            roleLabel.setText(isUser ? "你" : "助手");
            roleLabel.setTextSize(10);
            roleLabel.setTypeface(Typeface.DEFAULT_BOLD);
            roleLabel.setTextColor(isUser ? OL_BLUE : OL_OK);
            roleLabel.setPadding(0, 0, 0, dp(4));
            bubble.addView(roleLabel);

            // Message content
            TextView content = new TextView(this);
            content.setText(message.content);
            content.setTextSize(13);
            content.setTextColor(OL_INK_2);
            content.setPadding(0, 0, 0, dp(2));
            content.setLineSpacing(0, 1.3f);
            content.setTextIsSelectable(true);
            bubble.addView(content);

            // Copy button for assistant messages
            if (!isUser && message.content != null && !message.content.trim().isEmpty()) {
                LinearLayout meta = row();
                meta.setPadding(0, dp(4), 0, 0);
                Button copyAnswer = ghostButton("复制", OL_BLUE);
                copyAnswer.setOnClickListener(v -> {
                    android.content.ClipboardManager clipboard =
                            (android.content.ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
                    if (clipboard != null) {
                        clipboard.setPrimaryClip(
                                android.content.ClipData.newPlainText("OpenLess QA", message.content));
                        setStatus("已复制回答", OL_OK);
                    }
                });
                meta.addView(copyAnswer);
                bubble.addView(meta);
            }

            LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(
                    LinearLayout.LayoutParams.MATCH_PARENT,
                    LinearLayout.LayoutParams.WRAP_CONTENT);
            params.setMargins(0, 0, 0, dp(8));
            messagesList.addView(bubble, params);
        }
    }

    private void copyConversation() {
        List<QaChatMessage> messages = sessionStore.messages();
        if (messages.isEmpty()) {
            setStatus("当前没有可复制的对话", OL_WARN);
            return;
        }
        StringBuilder builder = new StringBuilder();
        for (QaChatMessage message : messages) {
            builder.append("user".equals(message.role) ? "你：\n" : "助手：\n");
            builder.append(message.content == null ? "" : message.content);
            builder.append("\n\n");
        }
        android.content.ClipboardManager clipboard =
                (android.content.ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
        if (clipboard != null) {
            clipboard.setPrimaryClip(
                    android.content.ClipData.newPlainText("OpenLess 问答对话", builder.toString().trim()));
            setStatus("对话已复制到剪贴板", OL_OK);
        }
    }

    private void setBusy(boolean busy, String message) {
        askButton.setEnabled(!busy);
        askButton.setAlpha(busy ? 0.6f : 1f);
        if (voiceButton != null) {
            voiceButton.setEnabled(!busy);
            voiceButton.setAlpha(busy ? 0.6f : 1f);
        }
        setStatus(message, busy ? OL_WARN : OL_OK);
    }

    private void showError(Exception e) {
        setStatus("出错", OL_ERR);
        android.content.Intent intent = new android.content.Intent(this, ErrorDetailActivity.class);
        intent.putExtra(ErrorDetailActivity.EXTRA_TITLE, "问答失败");
        intent.putExtra(ErrorDetailActivity.EXTRA_SOURCE, "问答面板");
        intent.putExtra(
                ErrorDetailActivity.EXTRA_MESSAGE,
                e == null ? "未知错误" : (e.getMessage() == null ? e.toString() : e.getMessage()));
        startActivity(intent);
    }

    private void setStatus(String message, int color) {
        status.setText(message);
        status.setTextColor(color);
    }

    private boolean ensureAudioPermission() {
        if (checkSelfPermission(android.Manifest.permission.RECORD_AUDIO)
                == android.content.pm.PackageManager.PERMISSION_GRANTED) {
            return true;
        }
        requestPermissions(new String[]{android.Manifest.permission.RECORD_AUDIO}, REQ_AUDIO);
        return false;
    }

    // ─── UI helpers ──────────────────────────────────────────────────

    private void card(LinearLayout root, CardBuilder builder) {
        LinearLayout card = new LinearLayout(this);
        card.setOrientation(LinearLayout.VERTICAL);
        card.setPadding(dp(14), dp(14), dp(14), dp(14));
        card.setBackgroundDrawable(cardBg());
        LinearLayout.LayoutParams params = new LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.MATCH_PARENT,
                LinearLayout.LayoutParams.WRAP_CONTENT);
        params.setMargins(0, 0, 0, dp(10));
        card.setLayoutParams(params);
        builder.build(card);
        root.addView(card);
    }

    private Drawable cardBg() {
        // OpenLess card: white bg, 12px radius
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

    private Drawable outlineBg() {
        float r = dp(999);
        float[] radii = new float[]{r, r, r, r, r, r, r, r};
        RoundRectShape shape = new RoundRectShape(radii, null, null);
        ShapeDrawable bg = new ShapeDrawable(shape);
        bg.getPaint().setStyle(Paint.Style.STROKE);
        bg.getPaint().setStrokeWidth(Math.max(1, dp(0.5f)));
        bg.getPaint().setColor(OL_LINE_STRONG);
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

    private EditText input(String value, String hint) {
        EditText editText = new EditText(this);
        editText.setText(value);
        editText.setHint(hint);
        editText.setTextColor(OL_INK);
        editText.setHintTextColor(OL_INK_4);
        editText.setTextSize(13);
        editText.setBackgroundDrawable(roundedBg(OL_CANVAS, 8));
        editText.setPadding(dp(10), dp(10), dp(10), dp(10));
        return editText;
    }

    private Button pillButton(String label) {
        Button button = new Button(this);
        button.setText(label);
        button.setAllCaps(false);
        button.setTextColor(Color.WHITE);
        button.setTextSize(13);
        button.setTypeface(Typeface.DEFAULT_BOLD);
        button.setBackgroundDrawable(roundedBg(OL_BLUE, 999));
        button.setPadding(dp(14), dp(8), dp(14), dp(8));
        button.setMinHeight(0);
        button.setMinimumHeight(0);
        return button;
    }

    private Button ghostButton(String label, int color) {
        Button button = new Button(this);
        button.setText(label);
        button.setAllCaps(false);
        button.setTextColor(color);
        button.setTextSize(11);
        button.setBackgroundDrawable(outlineBg());
        button.setPadding(dp(8), dp(4), dp(8), dp(4));
        button.setMinHeight(0);
        button.setMinimumHeight(0);
        return button;
    }

    private String value(EditText editText) {
        return editText.getText().toString().trim();
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
