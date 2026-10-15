import { defineConfig } from "vite";
// @ts-expect-error type error without @types/node package
import process from "node:process";
// @ts-expect-error type error without @types/node package
import { resolve } from "node:path";
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(() => ({

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  build: {
    rollupOptions: {
      // 单入口：安卓版没有划词悬浮图标窗口，也就不需要第二个 HTML 入口。
      input: {
        main: resolve(import.meta.dirname, "index.html"),
      },
    },
  },
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    // 必须写死 127.0.0.1：默认的 "localhost" 在 Windows 上只会绑到 IPv6 回环
    // (::1)，而 WebView2 走 IPv4 连 127.0.0.1 会被拒，页面直接白/黑屏。
    // 安卓上 `tauri android dev` 会把 devUrl 端口反代到设备，同样是字面 IPv4 最稳。
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
