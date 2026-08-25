import path from "path";
import dotenv from "dotenv";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

dotenv.config({ path: ".env.development" });

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react()],
  server: {
    allowedHosts: process.env.ALLOWED_HOSTS?.split(","),
    // Dev-only CORS bypass: leave VITE_KOMODO_HOST unset so the app
    // hits location.origin, and set VITE_KOMODO_PROXY to the real Core
    // to have vite forward the api paths.
    proxy: process.env.VITE_KOMODO_PROXY
      ? Object.fromEntries(
          ["/auth", "/user", "/read", "/write", "/execute", "/ws"].map(
            (path) => [
              path,
              {
                target: process.env.VITE_KOMODO_PROXY,
                changeOrigin: true,
                secure: false,
                ws: path === "/ws",
              },
            ],
          ),
        )
      : undefined,
  },
  resolve: {
    alias: [
      { find: "@", replacement: path.resolve(import.meta.dirname, "./src") },
      // monaco-editor >= 0.53 has an exports map ("./*.js": "./esm/vs/*.js"),
      // so legacy deep imports like "monaco-editor/esm/vs/..." no longer
      // resolve. monaco-worker-manager (used by monaco-yaml's worker) still
      // imports the legacy path — rewrite it to the exports-map form.
      {
        find: /^monaco-editor\/esm\/vs\/(.*)$/,
        replacement: "monaco-editor/$1",
      },
    ],
    dedupe: [
      "@mantine/core",
      "@mantine/form",
      "@mantine/hooks",
      "@mantine/notifications",
      "@monaco-editor/react",
      "@tanstack/react-table",
      "@tanstack/react-query",
      "lucide-react",
      "mogh_auth_client",
      "monaco-editor",
      "monaco-yaml",
      "react",
      "react-dom",
      "react-router-dom",
    ],
  },
  optimizeDeps: {
    exclude: ["mogh_ui"],
    // mogh_ui is excluded from prebundling, so its deps get served as
    // source ESM. @mantine/form default-imports CJS fast-deep-equal,
    // which only works prebundled.
    //
    // path-browserify is a CJS dep of monaco-yaml's yaml.worker. Vite's dep
    // scanner doesn't traverse `?worker` graphs, so without this it gets
    // served raw ("module is not defined" inside the worker). Force it
    // through prebundling to get CJS -> ESM interop.
    //
    // Same shape again for @tanstack/react-store (reached through the
    // excluded mogh_ui): it named-imports useSyncExternalStoreWithSelector
    // from CJS use-sync-external-store, which blanks the whole app in dev
    // with "does not provide an export named".
    include: [
      "@mantine/form",
      "fast-deep-equal",
      "path-browserify",
      "@tanstack/react-store",
      "use-sync-external-store/shim/with-selector",
    ],
  },
  css: {
    preprocessorOptions: {
      scss: {
        additionalData: '@use "mogh_ui/theme.scss" as theme;',
      },
    },
  },
});
