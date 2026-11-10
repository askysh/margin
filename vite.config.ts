import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: {
    target: "safari16",
    outDir: "dist",
    // The main window, and the recording chip's own small page (D53).
    rollupOptions: { input: { main: "index.html", chip: "chip.html" } },
  },
});
