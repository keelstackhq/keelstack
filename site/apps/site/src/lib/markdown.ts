import type { SatteriProcessorOptions } from "@astrojs/markdown-satteri";

/** Markdown options for the docs: smart quotes stay, dashes are off so `--flag` stays two hyphens. */
export const markdownOptions = {
  features: { smartPunctuation: { dashes: false } },
} satisfies SatteriProcessorOptions;
