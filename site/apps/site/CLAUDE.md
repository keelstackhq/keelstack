# Astro site

Astro 7 static site (`output: "static"`) with React islands via `@astrojs/react`, deployed to the
Cloudflare Pages project `keelstack-landing` with `wrangler pages deploy dist` (Pages ignores `wrangler.jsonc`). cascivo (`@cascivo/react`) renders to static HTML
in `.astro` sections; site CSS is unlayered so it beats cascivo layers.

- `astro.config.mjs` is template-owned but diverges on purpose (adds `@astrojs/react`); keep it.
- One island only: `PiecePicker` in Get started, `client:visible`. Everything else is static HTML.
- Fonts are self-hosted via Fontsource (Archivo, JetBrains Mono). Light theme only.
- `/llms.txt` is built from the same data files by `src/lib/llms.ts` (`src/pages/llms.txt.ts`).
- Astro runs through package scripts (`astro dev`, `astro build`); never `vp run`.
- Astro's own `vite` resolves to the Vite+ core through the root override: one vite in the lockfile.
- oxlint does not lint `.astro` templates; only TS/TSX is linted. Review `.astro` files by hand.
- oxfmt `.astro` support is experimental and off, so `format:check` excludes `.astro` files.
- `typecheck` is `tsc --noEmit` (TS/TSX only); `astro check` needs the TypeScript JS API.
- Tests: `vp test run`; component tests opt in to happy-dom with `// @vitest-environment happy-dom`.
- Preview deploy: `pnpm deploy:preview` with `PR_NUMBER` set.
