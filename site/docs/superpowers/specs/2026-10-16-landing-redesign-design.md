# Landing page redesign (light, cascivo)

Supersedes `2026-10-16-landing-page-design.md` for layout, look and stack. Source of truth for
values: the design brief (`DESIGN_BRIEF.md`) and its reference HTML, summarised here.

## Goal

Replace the text-heavy page with a light, welcoming one. A visitor (human or agent) gets the value
in five seconds: **seconds to a bootstrapped repo, then you only write the logic.** Content stays
plain and structured for agents; `/llms.txt` carries the same facts.

## Decisions

| Topic      | Decision                                                                                                                                                                                |
| ---------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| UI library | `@cascivo/react` through `@astrojs/react` (replaces `@astrojs/preact`; cascivo breaks under preact compat). Edits template-owned `astro.config.mjs`, recorded in the project notes.     |
| Islands    | One React island: the piece picker in Get started (`client:visible`). Every other cascivo component renders to static HTML with no client directive.                                    |
| Fonts      | Self-hosted: `@fontsource-variable/archivo` (wdth + wght axes) and `@fontsource/jetbrains-mono` 400/600, latin subset only, `font-display: swap`. No third-party requests.              |
| Theme      | Light only. No `prefers-color-scheme` dark variant, no toggle.                                                                                                                          |
| Budget     | HTML + CSS + fonts for `/` under ~150 KB transferred; the island's JS (React + signals + cascivo) is counted and reported separately and loads only when Get started scrolls into view. |
| Copy       | Brief copy is approved by the owner. D-001 names (`keelstackhq`). Hero variant default: "Seconds to a repo. Then it's all your logic."; the other two live in data, unused.             |

## Tokens

Defined once as CSS custom properties in `src/styles/tokens.css` and mapped onto cascivo's
`--cascivo-*` variables where cascivo components read them (button, badge, segmented control).
Site CSS is unlayered, so it beats cascivo's `cascivo.*` layers.

| Token       | Value     | Use                                                  |
| ----------- | --------- | ---------------------------------------------------- |
| paper       | `#F5F3EE` | page background                                      |
| surface     | `#FFFFFF` | cards, stat strip, Get started                       |
| ink         | `#15161A` | text, primary button, dark section                   |
| ink-2       | `#3C3E44` | body copy                                            |
| muted       | `#5A5C63` | labels, captions                                     |
| line        | `#DEDAD0` | borders; dashed variant `#BDB8AC` (`line-dashed`)    |
| chip        | `#ECE9E1` | neutral chips, inline code                           |
| accent      | `#8DF0C8` | highlights, "You write" panel, footer, success marks |
| accent-ink  | `#0B7A55` | link hover, accent on light, focus ring              |
| dark-line   | `#2A2C32` | borders inside dark blocks                           |
| dark-text-2 | `#C9C7C0` | secondary text on ink                                |

Accent is only a background behind ink text, or text on ink. Never accent text on paper.

Type: Archivo headlines 900, `font-stretch: 112%`, letter-spacing −0.04 to −0.05em, line-height
0.9–1, `text-wrap: balance`. H1 `clamp(48px, 7vw, 104px)`, H2 `clamp(36px, 5vw, 64px)`, footer
`clamp(48px, 9vw, 140px)`. Body 17–20px / 1.5, `text-wrap: pretty`. JetBrains Mono: section labels
13px, status chips 11px, tool names 34px, code 13.5–14px.

Layout: max width 1240px, 24px side padding, section padding 96px. Every grid uses
`repeat(auto-fit, minmax(min(100%, Npx), 1fr))`. No horizontal scroll at 360px. Radii 16px panels,
10px buttons/chips, 99px pills. Focus ring: 2px `#0B7A55` outline, 2px offset, everywhere.

## Sections and components

All in `apps/site/src/components/`, one `.astro` file per section; copy in `src/data/`.

1. `Nav.astro`: sticky, translucent paper (`rgba(245,243,238,.92)`) + `backdrop-filter: blur(8px)`;
   diamond mark + "keelstack", `pre-release` pill (cascivo `Badge`), links Tools (`#tools`), For
   agents (`#agents`), Get started (`#start`), GitHub ↗ (org URL).
2. `Hero.astro`: two columns (`minmax(min(100%,480px),1fr)`). Eyebrow, H1 with the second phrase on
   an accent highlight (`box-decoration-break: clone`), paragraph, buttons "Get started →" (solid
   ink, hover accent-ink) and "keelstackhq on GitHub" (outline, hover white). cascivo `Button` with
   `asChild` wrapping `<a>`. Right: `HeroTerminal.astro`, dark card titled
   "~/my-repo · splice 0.1 preview", the splice command, four ✓ lines, full-bleed accent line
   "→ your turn: write the app". Illustrative output, labelled as preview.
3. `StatStrip.astro`: white band, 4 cells from `data/stats.ts` (1 binary, 1 CI job, 1 line of YAML,
   1 pull request) with captions from the brief.
4. `Split.astro` (`01 · The split`): dashed panel "keelstack handles" with chips (`data/split.ts`);
   accent panel "You write — The logic." with the seed-files sentence.
5. `Tools.astro` + `ToolCard.astro` (`02 · Three tools`, `id="tools"`): hull / splice / hoard cards
   with role label, status pill (pre-0.1 filled ink+accent; under construction outlined; not
   released yet dashed), name, pitch, detail line, repo link pinned to the bottom.
6. `Agents.astro` (`03 · Built for agents`, `id="agents"`): full-bleed ink; H2 + paragraph; 4-row
   table (CLAUDE.md, --json --yes, ci-ok, update bot) from `data/agents.ts`, as a `<dl>`.
7. `StaysCurrent.astro` (`04 · Stays current`): three columns with a 3px top rule: Replaced /
   3-way merged / Stay yours (last rule and label in accent-ink).
8. `GetStarted.astro` (`05 · Get started`, `id="start"`): heading "Pick your pieces.", intro with
   inline `<code>`, then `<PiecePicker client:visible />`. The static HTML contains the default
   command, so it reads correctly before hydration and for agents.
9. `Footer.astro`: accent background, giant "Skip the setup.", CTAs, link row: keelstack, hull,
   splice, hoard, decisions (`https://github.com/keelstackhq/decisions`), tagline.

`404.astro` uses the same layout, nav and footer with a short "Page not found" block.

## Piece picker island

- `src/lib/command.ts`: pure `buildCommand(selected: string[], mode: "today" | "splice")` and
  `PIECES` order (`preact-app:web`, `hono-worker:api`, `ui-storybook:ui`, `astro-site:site`,
  `rust-tool:cli`, `tauri-app:desktop`, `docs:docs`). Output is exactly the brief's strings:
  - today: `node scripts/render.mjs new --template template --out ../my-repo \`↵
    `  --piece base <sel…> --data scope=acme`↵`cd ../my-repo && git init && pnpm install && pnpm run ci`
  - splice: `pnpm dlx @keelstackhq/splice new gh:keelstackhq/hull//template my-repo \`↵`  <sel…>`
  - selection keeps `PIECES` order, each as `--piece <piece>:<name>`.
- `src/components/PiecePicker.tsx` (React):
  - "base · always" fixed chip (not a button).
  - Each piece: cascivo `Button` with `aria-pressed`; on = accent fill + "✓", off = outline + "+".
    Default on: web, api.
  - Mode: cascivo `SegmentedControl` (radio semantics, arrow keys), labels "Today" /
    "With splice 0.1", accessible name "Command version".
  - Copy: cascivo `Button`, label "Copy" → "Copied" for 1400 ms; on clipboard failure the label
    reads "Copy failed"; button hidden when `navigator.clipboard` is missing. Status announced via
    `aria-live="polite"`. (cascivo `CopyButton` is icon-only with a fixed 2 s reset, so it does not
    fit the brief.)
  - `<pre>` with the command; caption below: today "In a hull checkout.", splice
    "Preview: flags unconfirmed until splice 0.1 ships."
- Reduced motion: no animation is added, so nothing to gate; any later transition sits behind
  `prefers-reduced-motion: no-preference`.

## `/llms.txt`

`src/pages/llms.txt.ts` (static endpoint) builds plain Markdown from `data/*.ts`: one-line summary,
the three tools (status, pitch, repo), the agent facts, and both commands for the default pieces.

## Data and removals

- Rewrite `data/tools.ts` to the brief's tool cards; add `stats.ts`, `split.ts`, `agents.ts`,
  `current.ts`, `hero.ts`.
- Delete `Header.astro`, `Fit.astro`, `copy-button.tsx` and its test; drop `preact`,
  `@astrojs/preact` from the site.
- Add to catalog (exact pins, older than 3 days): `@astrojs/react` 7.0.1, `@fontsource-variable/archivo`
  5.3.0, `@fontsource/jetbrains-mono` 5.3.0, `@testing-library/react` 16.3.3 (+ `@testing-library/dom`
  if required); site uses `react`, `react-dom`, `@preact/signals-react`, `@cascivo/react` from the
  catalog. TS: `jsx: react-jsx`, `jsxImportSource: react`, plus `*.css` module declaration.

## Testing

- Unit (Vitest, node): `command.ts` exact output per mode, selection order, empty selection;
  data invariants (D-001 links, no empty strings, numbered section labels 01–05, accent rules are
  not encoded in data).
- Component (happy-dom + `@testing-library/react`): default state shows web+api pressed and the
  today command; toggling updates `aria-pressed` and the command; switching mode changes command and
  caption; copy writes the current command and shows "Copied" then reverts after 1400 ms (fake
  timers); copy failure message; no copy button without clipboard.
- `llms.txt` endpoint test: contains each tool name and both commands.
- Verification: `pnpm run ci`, `pnpm --filter @keelstackhq/site build`, Playwright on
  `astro preview` at 360, 390 and 1280 px: full-page screenshots, `scrollWidth <= innerWidth`, no
  console errors, picker works after hydration, focus ring visible; report transferred sizes.

## Out of scope

Dark mode, theme toggle, A/B testing infrastructure for the hero variants, analytics, the custom
domain, real splice output in the hero.

## Amendment 2026-10-22: splice 0.2.0 released

splice 0.2.0 and hull v0.1.1 are released, so the preview parts above no longer apply:

- Get started shows one command, no Today/splice mode switch:
  `pnpm dlx @keelstackhq/splice@0.2.0 new \` / `gh:keelstackhq/hull//template my-repo --ref v0.1.1 \` /
  `--piece …`, caption "Pinned to hull v0.1.1. Drop --ref to get the latest hull release."
- The hero terminal shows that command with the default pieces and real output lines from running
  it (an excerpt of the `create` lines plus a count), not illustrative ✓ lines.
- Tool statuses: hull `v0.1.1`, splice `v0.2.0`, hoard `v0.1.0` (release tags); the hoard detail
  shows the README's `uses: keelstackhq/hoard@v0` step in mono.
