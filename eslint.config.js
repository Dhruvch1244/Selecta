import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["**/dist/**", "**/node_modules/**", "target/**", "**/src-tauri/**", "packages/protocol/src/generated/**"] },
  ...tseslint.configs.recommended,
  {
    rules: {
      "@typescript-eslint/no-unused-vars": ["error", { argsIgnorePattern: "^_", varsIgnorePattern: "^_" }],
    },
  },
  {
    // Pillar 1: the engine and the protocol must stay UI-free. With npm
    // workspaces hoisting, React is *resolvable* from these packages, so the
    // boundary is enforced here and CI fails on a stray import.
    files: ["packages/engine/**", "packages/protocol/**"],
    rules: {
      "no-restricted-imports": ["error", { patterns: [
        { group: ["react", "react-dom", "react/*", "zustand", "@tauri-apps/*", "@/*", "@selecta/ui"], message: "packages/engine and packages/protocol must not depend on UI or the Tauri shell." },
      ] }],
    },
  },
);
