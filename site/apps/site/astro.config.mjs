import { satteri } from "@astrojs/markdown-satteri";
import react from "@astrojs/react";
import cascivo from "@cascivo/astro";
import { defineConfig } from "astro/config";
import { markdownOptions } from "./src/lib/markdown.ts";

export default defineConfig({
  output: "static",
  markdown: { syntaxHighlight: false, processor: satteri(markdownOptions) },
  // D-030: cascivo through @cascivo/astro (layer order, theme, theme script); light only.
  integrations: [
    react(),
    cascivo({ theme: "@cascivo/themes/light.css", themeScript: { defaultTheme: "light" } }),
  ],
});
