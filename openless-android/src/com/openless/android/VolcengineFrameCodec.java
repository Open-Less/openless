package com.openless.android;

import java.nio.ByteBuffer;
import java.nio.ByteOrder;

final class VolcengineFrameCodec {
    static final int FULL_CLIENT_REQUEST = 0x1;
    static final int AUDIO_ONLY_REQUEST = 0x2;
    static final int FULL_SERVER_RESPONSE = 0x9;
    static final int ERROR_MESSAGE = 0xf;

    static final int FLAG_NONE = 0x0;
    static final int FLAG_POSITIVE_SEQUENCE = 0x1;
    static final int FLAG_LAST_PACKET = 0x2;
    static final int FLAG_NEGATIVE_SEQUENCE = 0x3;

    static final int SERIALIZATION_NONE = 0x0;
    static final int SERIALIZATION_JSON = 0x1;

    private VolcengineFrameCodec() {
    }

    static byte[] build(int messageType, int flags, int serialization, byte[] payload, Integer sequence) {
        int sequenceBytes = flags == FLAG_POSITIVE_SEQUENCE || flags == FLAG_NEGATIVE_SEQUENCE ? 4 : 0;
        ByteBuffer buffer = ByteBuffer.allocate(4 + sequenceBytes + 4 + payload.length);
        buffer.order(ByteOrder.BIG_ENDIAN);
        buffer.put((byte) 0x11);
        buffer.put((byte) ((messageType << 4) | flags));
        buffer.put((byte) (serialization << 4));
        buffer.put((byte) 0x00);
        if (sequenceBytes > 0) {
            buffer.putInt(sequence == null ? 0 : sequence);
        }
        buffer.putInt(payload.length);
        buffer.put(payload);
        return buffer.array();
    }

    static Parsed parse(byte[] data) {
        if (data == null || data.length < 8) {
            return null;
        }
        int headerSize = (data[0] & 0x0f) * 4;
        if (headerSize < 4 || data.length < headerSize + 4) {
            return null;
        }
        int messageType = (data[1] >> 4) & 0x0f;
        int flags = data[1] & 0x0f;
        int compression = data[2] & 0x0f;
        if (compression != 0) {
            return null;
        }
        ByteBuffer buffer = ByteBuffer.wrap(data).order(ByteOrder.BIG_ENDIAN);
        int offset = headerSize;
        Integer sequence = null;
        if (flags == FLAG_POSITIVE_SEQUENCE || flags == FLAG_NEGATIVE_SEQUENCE) {
            if (data.length < offset + 4) {
                return null;
            }
            sequence = buffer.getInt(offset);
            offset += 4;
        }
        Integer errorCode = null;
        if (messageType == ERROR_MESSAGE) {
            if (data.length < offset + 8) {
                return null;
            }
            errorCode = buffer.getInt(offset);
            int size = buffer.getInt(offset + 4);
            offset += 8;
            if (size < 0 || data.length < offset + size) {
                return null;
            }
            return new Parsed(messageType, flags, sequence, errorCode, slice(data, offset, size));
        }
        int size = buffer.getInt(offset);
        offset += 4;
        if (size < 0 || data.length < offset + size) {
            return null;
        }
        return new Parsed(messageType, flags, sequence, null, slice(data, offset, size));
    }

    private static byte[] slice(byte[] data, int offset, int size) {
        byte[] out = new byte[size];
        System.arraycopy(data, offset, out, 0, size);
        return out;
    }

    static final class Parsed {
        final int messageType;
        final int flags;
        final Integer sequence;
        final Integer errorCode;
        final byte[] payload;

        Parsed(int messageType, int flags, Integer sequence, Integer errorCode, byte[] payload) {
            this.messageType = messageType;
            this.flags = flags;
            this.sequence = sequence;
            this.errorCode = errorCode;
            this.payload = payload;
        }

        boolean isFinal() {
            return flags == FLAG_LAST_PACKET
                    || flags == FLAG_NEGATIVE_SEQUENCE
                    || (sequence != null && sequence < 0);
        }
    }
}
