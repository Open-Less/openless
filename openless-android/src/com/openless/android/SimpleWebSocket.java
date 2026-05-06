package com.openless.android;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.URI;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.security.SecureRandom;
import java.util.Base64;
import java.util.LinkedHashMap;
import java.util.Map;

import javax.net.ssl.SSLSocketFactory;

final class SimpleWebSocket implements AutoCloseable {
    private final java.net.Socket socket;
    private final InputStream in;
    private final OutputStream out;
    private final SecureRandom random = new SecureRandom();

    SimpleWebSocket(String url, Map<String, String> headers) throws Exception {
        URI uri = URI.create(url);
        int port = uri.getPort() > 0 ? uri.getPort() : ("wss".equals(uri.getScheme()) ? 443 : 80);
        if (!"wss".equals(uri.getScheme())) {
            throw new IllegalArgumentException("仅支持 wss:// 协议。");
        }
        socket = SSLSocketFactory.getDefault().createSocket(uri.getHost(), port);
        socket.setSoTimeout(15000);
        in = socket.getInputStream();
        out = socket.getOutputStream();
        handshake(uri, headers == null ? new LinkedHashMap<>() : headers);
    }

    void sendBinary(byte[] payload) throws Exception {
        sendFrame(0x2, payload);
    }

    void setReadTimeoutMs(int timeoutMs) throws Exception {
        socket.setSoTimeout(timeoutMs);
    }

    byte[] readBinary() throws Exception {
        while (true) {
            Frame frame = readFrame();
            if (frame.opcode == 0x2) {
                return frame.payload;
            }
            if (frame.opcode == 0x8) {
                throw new IllegalStateException("WebSocket 已关闭。");
            }
            if (frame.opcode == 0x9) {
                sendFrame(0xA, frame.payload);
            }
        }
    }

    @Override
    public void close() {
        try {
            sendFrame(0x8, new byte[0]);
        } catch (Exception ignored) {
        }
        try {
            socket.close();
        } catch (Exception ignored) {
        }
    }

    private void handshake(URI uri, Map<String, String> headers) throws Exception {
        byte[] keyBytes = new byte[16];
        random.nextBytes(keyBytes);
        String key = Base64.getEncoder().encodeToString(keyBytes);
        String path = uri.getRawPath();
        if (path == null || path.isEmpty()) {
            path = "/";
        }
        if (uri.getRawQuery() != null) {
            path += "?" + uri.getRawQuery();
        }
        StringBuilder request = new StringBuilder();
        request.append("GET ").append(path).append(" HTTP/1.1\r\n");
        request.append("Host: ").append(uri.getHost()).append("\r\n");
        request.append("Upgrade: websocket\r\n");
        request.append("Connection: Upgrade\r\n");
        request.append("Sec-WebSocket-Key: ").append(key).append("\r\n");
        request.append("Sec-WebSocket-Version: 13\r\n");
        for (Map.Entry<String, String> entry : headers.entrySet()) {
            request.append(entry.getKey()).append(": ").append(entry.getValue()).append("\r\n");
        }
        request.append("\r\n");
        out.write(request.toString().getBytes(StandardCharsets.US_ASCII));
        out.flush();

        String response = readHttpHeader();
        if (!response.startsWith("HTTP/1.1 101") && !response.startsWith("HTTP/1.0 101")) {
            throw new IllegalStateException("WebSocket 握手失败：" + response.split("\r\n")[0]);
        }
        String accept = headerValue(response, "Sec-WebSocket-Accept");
        String expected = Base64.getEncoder().encodeToString(MessageDigest.getInstance("SHA-1")
                .digest((key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").getBytes(StandardCharsets.US_ASCII)));
        if (!expected.equals(accept)) {
            throw new IllegalStateException("WebSocket 握手失败：accept key 无效。");
        }
    }

    private String headerValue(String response, String name) {
        String prefix = name.toLowerCase(java.util.Locale.US) + ":";
        String[] lines = response.split("\r\n");
        for (String line : lines) {
            String lower = line.toLowerCase(java.util.Locale.US);
            if (lower.startsWith(prefix)) {
                return line.substring(line.indexOf(':') + 1).trim();
            }
        }
        return "";
    }

    private String readHttpHeader() throws Exception {
        ByteArrayOutputStream header = new ByteArrayOutputStream();
        int matched = 0;
        byte[] end = new byte[]{'\r', '\n', '\r', '\n'};
        while (true) {
            int b = in.read();
            if (b < 0) {
                throw new IllegalStateException("WebSocket 握手期间连接已结束。");
            }
            header.write(b);
            matched = b == end[matched] ? matched + 1 : (b == end[0] ? 1 : 0);
            if (matched == end.length) {
                return header.toString(StandardCharsets.US_ASCII.name());
            }
        }
    }

    private void sendFrame(int opcode, byte[] payload) throws Exception {
        ByteArrayOutputStream frame = new ByteArrayOutputStream();
        frame.write(0x80 | opcode);
        int len = payload.length;
        if (len <= 125) {
            frame.write(0x80 | len);
        } else if (len <= 65535) {
            frame.write(0x80 | 126);
            frame.write((len >> 8) & 0xff);
            frame.write(len & 0xff);
        } else {
            frame.write(0x80 | 127);
            frame.write(ByteBuffer.allocate(8).putLong(len).array());
        }
        byte[] mask = new byte[4];
        random.nextBytes(mask);
        frame.write(mask);
        for (int i = 0; i < payload.length; i++) {
            frame.write(payload[i] ^ mask[i % 4]);
        }
        out.write(frame.toByteArray());
        out.flush();
    }

    private Frame readFrame() throws Exception {
        int b0 = in.read();
        int b1 = in.read();
        if (b0 < 0 || b1 < 0) {
            throw new IllegalStateException("读取 WebSocket 帧时连接已结束。");
        }
        int opcode = b0 & 0x0f;
        boolean masked = (b1 & 0x80) != 0;
        long len = b1 & 0x7f;
        if (len == 126) {
            len = (in.read() << 8) | in.read();
        } else if (len == 127) {
            byte[] buf = readExact(8);
            len = ByteBuffer.wrap(buf).getLong();
        }
        byte[] mask = masked ? readExact(4) : null;
        byte[] payload = readExact((int) len);
        if (masked) {
            for (int i = 0; i < payload.length; i++) {
                payload[i] = (byte) (payload[i] ^ mask[i % 4]);
            }
        }
        return new Frame(opcode, payload);
    }

    private byte[] readExact(int len) throws Exception {
        byte[] out = new byte[len];
        int offset = 0;
        while (offset < len) {
            int read = in.read(out, offset, len - offset);
            if (read < 0) {
                throw new IllegalStateException("读取 WebSocket 数据时连接已结束。");
            }
            offset += read;
        }
        return out;
    }

    private static final class Frame {
        final int opcode;
        final byte[] payload;

        Frame(int opcode, byte[] payload) {
            this.opcode = opcode;
            this.payload = payload;
        }
    }
}
