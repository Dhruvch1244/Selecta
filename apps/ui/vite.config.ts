import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath, URL } from "node:url";
import { defineConfig } from "vite";

// Tauri expects a fixed dev port and must not have Vite clear its terminal output.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  // dsgn writes component imports as "@/" + utilsDir ("@/src/lib/utils"), while
  // app code uses "@/lib/...". Both resolve to src/ so registry files stay
  // byte-identical to upstream (keeps `dsgn diff` / `dsgn update` clean).
  resolve: {
    alias: [
      { find: /^@\/src\//, replacement: fileURLToPath(new URL("./src/", import.meta.url)) },
      { find: /^@\//, replacement: fileURLToPath(new URL("./src/", import.meta.url)) },
    ],
  },
  clearScreen: false,
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
  build: { target: "es2023", sourcemap: true },
});
