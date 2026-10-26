import react from "@astrojs/react";
import { defineConfig } from "astro/config";

export default defineConfig({
  output: "static",
  markdown: { syntaxHighlight: false },
  integrations: [react()],
});
