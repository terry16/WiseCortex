/// <reference types="vitest" />
import { defineConfig } from "vite";

/** 本机后端端口，与 server 的 WC_BIND 默认值（127.0.0.1:7070）一致。 */
const BACKEND_PORT = 7070;

export default defineConfig({
  // 开发服务器（npm run dev）：把 /api、/ws 反代到本机 server。
  // 浏览器里 backend.ts 的 httpBase() 返回空串（走相对路径），生产由 nginx 反代；
  // 本机没有 nginx，缺了这段 dev 就打不到后端——请求会打回 vite 自己拿到 404。
  server: {
    proxy: {
      "/api": { target: `http://127.0.0.1:${BACKEND_PORT}`, changeOrigin: true },
      "/ws": { target: `ws://127.0.0.1:${BACKEND_PORT}`, ws: true },
    },
  },
  build: {
    // 不让 vite 清空 dist —— 改由 scripts/clean-dist.mjs 清（保留宝塔锁定的 .user.ini）。
    emptyOutDir: false,
    rollupOptions: {
      output: {
        // 把较大的第三方库（marked + highlight.js）拆成独立 chunk：主包更小、利于缓存。
        manualChunks: {
          markdown: ["marked", "highlight.js/lib/common"],
        },
      },
    },
  },
  test: {
    environment: "jsdom",
    globals: true,
  },
});
