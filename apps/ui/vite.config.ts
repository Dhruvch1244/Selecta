import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath, URL } from "node:url";
import { createReadStream, existsSync } from "node:fs";
import { join, normalize } from "node:path";
import { defineConfig, type Plugin } from "vite";

// Dev-only: serve mock-backend fixtures (real audio) from dev-fixtures/ at
// /__fixtures__. Deliberately not in public/: Vite copies public/ into every
// production build, which once embedded 100 MB of test MP3s in the app binary.
const fixturesDir = fileURLToPath(new URL("./dev-fixtures/", import.meta.url));
const devFixtures = (): Plugin => ({
  name: "selecta-dev-fixtures",
  apply: "serve",
  configureServer(server) {
    server.middlewares.use("/__fixtures__", (req, res, next) => {
      const file = normalize(join(fixturesDir, decodeURIComponent((req.url ?? "").split("?")[0] ?? "")));
      if (!file.startsWith(fixturesDir) || !existsSync(file)) return next();
      res.setHeader("Content-Type", file.endsWith(".json") ? "application/json" : "audio/mpeg");
      createReadStream(file).pipe(res);
    });
  },
});

// Tauri expects a fixed dev port and must not have Vite clear its terminal output.
export default defineConfig({
  plugins: [react(), tailwindcss(), devFixtures()],
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
