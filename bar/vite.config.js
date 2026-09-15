import { fileURLToPath } from "node:url";

import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";

// The webview loads the built files from disk, so every asset reference has
// to be relative — an absolute /assets/… path resolves against the filesystem
// root and 404s.
export default defineConfig({
    plugins: [vue()],
    base: "./",
    clearScreen: false,
    server: { port: 1420, strictPort: true },
    build: {
        target: "esnext",
        emptyOutDir: true,
        // Three pages, because the bar, the panel and the launcher are three
        // windows with nothing in common but the stylesheet.
        rollupOptions: {
            input: {
                main: fileURLToPath(new URL("index.html", import.meta.url)),
                panel: fileURLToPath(new URL("panel.html", import.meta.url)),
                launcher: fileURLToPath(new URL("launcher.html", import.meta.url))
            }
        }
    }
});
