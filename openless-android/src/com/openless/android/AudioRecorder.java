package com.openless.android;

import android.media.AudioFormat;
import android.media.AudioRecord;
import android.media.MediaRecorder;

import java.io.ByteArrayOutputStream;
import java.util.concurrent.atomic.AtomicBoolean;

final class AudioRecorder {
    static final int SAMPLE_RATE = 16000;

    interface AudioConsumer {
        void consume(byte[] pcm, int length);
    }

    interface LevelListener {
        void onLevel(float level);
    }

    private final AtomicBoolean recording = new AtomicBoolean(false);
    private final Object pcmLock = new Object();
    private AudioRecord audioRecord;
    private Thread worker;
    private ByteArrayOutputStream pcm = new ByteArrayOutputStream();
    private long startedAtMs;

    boolean isRecording() {
        return recording.get();
    }

    void start() {
        start(null);
    }

    void start(AudioConsumer consumer) {
        start(consumer, null);
    }

    void start(AudioConsumer consumer, LevelListener levelListener) {
        if (recording.get()) {
            return;
        }
        int minBuffer = AudioRecord.getMinBufferSize(
                SAMPLE_RATE,
                AudioFormat.CHANNEL_IN_MONO,
                AudioFormat.ENCODING_PCM_16BIT);
        int bufferSize = Math.max(minBuffer, SAMPLE_RATE);
        audioRecord = new AudioRecord(
                MediaRecorder.AudioSource.MIC,
                SAMPLE_RATE,
                AudioFormat.CHANNEL_IN_MONO,
                AudioFormat.ENCODING_PCM_16BIT,
                bufferSize);
        synchronized (pcmLock) {
            pcm = new ByteArrayOutputStream();
        }
        startedAtMs = System.currentTimeMillis();
        recording.set(true);
        audioRecord.startRecording();
        worker = new Thread(() -> readLoop(bufferSize, consumer, levelListener), "openless-audio");
        worker.start();
    }

    Recording stop() {
        if (!recording.getAndSet(false)) {
            return new Recording(new byte[0], 0);
        }
        try {
            if (audioRecord != null) {
                audioRecord.stop();
            }
        } catch (IllegalStateException ignored) {
        }
        if (worker != null) {
            try {
                worker.join(900);
            } catch (InterruptedException e) {
                Thread.currentThread().interrupt();
            }
        }
        if (audioRecord != null) {
            audioRecord.release();
            audioRecord = null;
        }
        long duration = Math.max(0, System.currentTimeMillis() - startedAtMs);
        synchronized (pcmLock) {
            return new Recording(pcm.toByteArray(), duration);
        }
    }

    private void readLoop(int bufferSize, AudioConsumer consumer, LevelListener levelListener) {
        byte[] buffer = new byte[bufferSize];
        while (recording.get()) {
            int read = audioRecord.read(buffer, 0, buffer.length);
            if (read > 0) {
                synchronized (pcmLock) {
                    pcm.write(buffer, 0, read);
                }
                if (consumer != null) {
                    byte[] copy = new byte[read];
                    System.arraycopy(buffer, 0, copy, 0, read);
                    consumer.consume(copy, read);
                }
                if (levelListener != null) {
                    levelListener.onLevel(rms(buffer, read));
                }
            }
        }
    }

    private float rms(byte[] buffer, int length) {
        if (length < 2) {
            return 0f;
        }
        double sum = 0.0;
        int samples = length / 2;
        for (int i = 0; i + 1 < length; i += 2) {
            int lo = buffer[i] & 0xff;
            int hi = buffer[i + 1];
            short sample = (short) ((hi << 8) | lo);
            double normalized = sample / 32768.0;
            sum += normalized * normalized;
        }
        double value = Math.sqrt(sum / Math.max(1, samples));
        return (float) Math.max(0.0, Math.min(1.0, value * 8.0));
    }

    static final class Recording {
        final byte[] pcm;
        final long durationMs;

        Recording(byte[] pcm, long durationMs) {
            this.pcm = pcm;
            this.durationMs = durationMs;
        }
    }
}
