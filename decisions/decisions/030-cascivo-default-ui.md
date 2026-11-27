# D-030: cascivo is the default UI layer

- Date: 2026-11-27
- Scope: all
- Status: accepted
- Supersedes: —

## Context

The owner made cascivo (`@cascivo/react`, `@cascivo/themes`, `@cascivo/astro`) the default UI layer for
everything UI in keelstack, Astro included. hull already shipped cascivo for the Preact pieces, but
`astro-site` used Preact islands without cascivo, and agents had no rule that stops them writing
their own components, Tailwind or a second component library.

## Decision

- UI code uses cascivo first: `@cascivo/react` components and `--cascivo-*` tokens before own markup
  or CSS; shared components wrap cascivo in `packages/ui`. No other component library, Tailwind or
  CSS-in-JS without a new decision.
- Preact apps (`hull/template` pieces `preact-app`, `ui-storybook`) use cascivo through
  `preact/compat`, client-side rendering only (no SSR, no prerender).
- Astro sites (piece `astro-site`, `site/apps/site`) use cascivo through React islands
  (`@astrojs/react`) and the `@cascivo/astro` integration (`integrations: [react(), cascivo()]`).
  Compose cascivo in a `.tsx` component; never hydrate a `@cascivo/react` export directly from
  `.astro`. Server-rendered Preact islands fail under Astro (cascivo `docs/USING-WITH-ASTRO.md`).
- Agents look components up in https://cascivo.com/llms.txt (`llms-full.txt`) or the cascivo MCP
  server (`npx cascivo mcp init`, or `{"command":"npx","args":["-y","@cascivo/mcp"]}`); skills via
  `npx skills add cascivo/cascivo --skill '*' -y`, never vendored. The cascivo CLI copy-in is not
  the default.

## Consequences

- hull pins `@cascivo/react`, `@cascivo/astro`, `@astrojs/react` and React types in its catalog;
  the generated `AGENTS.md` gets a `## UI` section whenever a UI piece is selected.
- Out of scope: Preact SSR or prerender, server-rendered Preact islands in Astro, Workers SSR
  (`hono-worker` has no UI), `@cascivo/elements` and `@cascivo/solid` (0.0.0 placeholders). A
  terminal UI for `rust-cli` (cascivo-tui) is a follow-up.
