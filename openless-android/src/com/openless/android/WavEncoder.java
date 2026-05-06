package com.openless.android;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.nio.ByteBuffer;
import java.nio.ByteOrder;

final class WavEncoder {
    private WavEncoder() {
    }

    static byte[] encode16kMono(byte[] pcm) {
        int sampleRate = 16000;
        short channels = 1;
        short bitsPerSample = 16;
        int byteRate = sampleRate * channels * bitsPerSample / 8;
        short blockAlign = (short) (channels * bitsPerSample / 8);
        int dataSize = pcm.length;
        int chunkSize = 36 + dataSize;

        ByteArrayOutputStream out = new ByteArrayOutputStream(44 + dataSize);
        try {
            out.write(ascii("RIFF"));
            writeInt(out, chunkSize);
            out.write(ascii("WAVE"));
            out.write(ascii("fmt "));
            writeInt(out, 16);
            writeShort(out, (short) 1);
            writeShort(out, channels);
            writeInt(out, sampleRate);
            writeInt(out, byteRate);
            writeShort(out, blockAlign);
            writeShort(out, bitsPerSample);
            out.write(ascii("data"));
            writeInt(out, dataSize);
            out.write(pcm);
        } catch (IOException impossible) {
            throw new IllegalStateException(impossible);
        }
        return out.toByteArray();
    }

    private static byte[] ascii(String value) {
        return value.getBytes(java.nio.charset.StandardCharsets.US_ASCII);
    }

    private static void writeInt(ByteArrayOutputStream out, int value) throws IOException {
        out.write(ByteBuffer.allocate(4).order(ByteOrder.LITTLE_ENDIAN).putInt(value).array());
    }

    private static void writeShort(ByteArrayOutputStream out, short value) throws IOException {
        out.write(ByteBuffer.allocate(2).order(ByteOrder.LITTLE_ENDIAN).putShort(value).array());
    }
}
