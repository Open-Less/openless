package com.openless.android;

import android.app.Activity;
import android.content.Intent;
import android.os.Bundle;

public final class ProcessTextActivity extends Activity {
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        launchQaFromSelection(getIntent());
        finish();
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        launchQaFromSelection(intent);
        finish();
    }

    private void launchQaFromSelection(Intent intent) {
        String selectedText = "";
        if (intent != null) {
            CharSequence text = intent.getCharSequenceExtra(Intent.EXTRA_PROCESS_TEXT);
            if (text != null) {
                selectedText = text.toString().trim();
            }
        }
        Intent qaIntent = new Intent(this, QaPanelActivity.class);
        if (!selectedText.isEmpty()) {
            qaIntent.putExtra(QaPanelActivity.EXTRA_CONTEXT, selectedText);
        }
        startActivity(qaIntent);
    }
}
