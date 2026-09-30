import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// Vite 配置：端口 1420、strictPort、不清屏（docs/architecture.md §1）
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
});
