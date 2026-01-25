import { defineConfig } from "vite";
import react from "@vitejs/plugin-react-swc";
import wasm from "vite-plugin-wasm";
import topLevelAwait from "vite-plugin-top-level-await";

export default defineConfig({
  plugins: [wasm(), topLevelAwait(), react()],
  server: {
    port: 3000,
  },
  optimizeDeps: {
    exclude: ["sim-core"],
  },
});
