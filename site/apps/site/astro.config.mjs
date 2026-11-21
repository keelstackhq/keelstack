import { satteri } from "@astrojs/markdown-satteri";
import react from "@astrojs/react";
import { defineConfig } from "astro/config";
import { markdownOptions } from "./src/lib/markdown.ts";

export default defineConfig({
  output: "static",
  markdown: { syntaxHighlight: false, processor: satteri(markdownOptions) },
  integrations: [react()],
});
