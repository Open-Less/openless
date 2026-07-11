import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

const appRoot = fileURLToPath(new URL(".", import.meta.url));

const host = process.env.TAURI_DEV_HOST;
const isMobileDev =
  process.env.TAURI_ENV_PLATFORM === "android" ||
  process.env.TAURI_ENV_PLATFORM === "ios";

export default defineConfig(async () => ({
  // tailwindcss：只服务统一聊天面板的官方 shadcn 组件（components/chat/chat.css
  // 入口，source 显式圈定 chat/ 与两个面板文件）；其余全局样式不走 tailwind。
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@android": path.resolve(appRoot, "android/frontend"),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: isMobileDev ? "0.0.0.0" : host || false,
    hmr: isMobileDev
      ? { protocol: "ws", host: host || "0.0.0.0", port: 1421 }
      : host
        ? { protocol: "ws", host, port: 1421 }
        : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: process.env.TAURI_PLATFORM === "windows" ? "chrome105" : "safari13",
    minify: !process.env.TAURI_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_DEBUG,
  },
}));
