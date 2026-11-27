import react from "@astrojs/react";
import cascivo from "@cascivo/astro";
import { defineConfig } from "astro/config";

export default defineConfig({
  output: "static",
  // React islands render cascivo; @cascivo/astro injects the layer order, theme and theme script.
  integrations: [react(), cascivo()],
});
