# Landing redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild `apps/site` as the light keelstack landing page from the design brief, using cascivo components through React.

**Architecture:** Astro static site; `@astrojs/react` replaces `@astrojs/preact`. Sections are `.astro` files that render cascivo React components to static HTML (no client directive). One React island (`PiecePicker`, `client:visible`) builds the get-started command from a pure `buildCommand`. Copy lives in `src/data/*.ts`; `/llms.txt` is generated from the same data.

**Tech Stack:** Astro 7.3.8, React 19.3, `@cascivo/react` 1.9.0 + `@cascivo/themes` 1.0.3, `@preact/signals-react` 3.12.0, Fontsource (Archivo variable, JetBrains Mono), Vitest via `vite-plus/test`, happy-dom, `@testing-library/react`.

**Spec:** `docs/superpowers/specs/2026-10-16-landing-redesign-design.md` (values) and the design brief reference HTML (exact layout). Read both.

## Global Constraints

- Versions are exact pins in the root `pnpm-workspace.yaml` catalog; packages use `catalog:`. Pins must be ≥3 days old (`minimumReleaseAge: 4320`). New pins: `@astrojs/react` 7.0.1, `@fontsource-variable/archivo` 5.3.0, `@fontsource/jetbrains-mono` 5.3.0, `@testing-library/react` 16.3.3, `@testing-library/dom` 10.4.2, `@types/react` 19.3.0, `@types/react-dom` 19.3.0. Remove `@types/react` from `peerDependencyRules.ignoreMissing` and its comment.
- Tokens (exact): paper `#F5F3EE`, surface `#FFFFFF`, ink `#15161A`, ink-2 `#3C3E44`, muted `#5A5C63`, line `#DEDAD0`, line-dashed `#BDB8AC`, chip `#ECE9E1`, accent `#8DF0C8`, accent-ink `#0B7A55`, dark-line `#2A2C32`, dark-text-2 `#C9C7C0`. Terminal text `#E9E7E1`, terminal dim `#8B8D94`, dots `#3A3C42`, mode track `#22242A`.
- Accent only as a background behind ink, or as text on ink. Never accent text on paper/white.
- Headlines: Archivo 900, `font-stretch: 112%`; H1 `clamp(48px,7vw,104px)` lh .92 ls −.045em; H2 `clamp(36px,5vw,64px)` lh 1 ls −.04em; footer `clamp(48px,9vw,140px)` lh .9 ls −.05em; `text-wrap: balance` on headings, `pretty` on paragraphs.
- Layout: `max-width: 1240px`, padding `0 24px`, section padding 96px; every grid is `repeat(auto-fit, minmax(min(100%, Npx), 1fr))`; radii 16 / 10 / 99px. No horizontal scroll at 360px.
- Focus ring on every interactive element: `outline: 2px solid #0B7A55; outline-offset: 2px` (`:focus-visible`).
- Light only: no `prefers-color-scheme` rules; `<meta name="color-scheme" content="light">`.
- Copy: the brief's text verbatim (see reference HTML); D-001 names; never invent features or numbers.
- Site CSS is unlayered (it must beat cascivo's `cascivo.*` layers). No client JS outside `PiecePicker`.
- `.astro` files are not linted or format-checked: review them by hand. Run `pnpm format` before each commit.
- Commits end with the session's `Co-Authored-By` / `Claude-Session` trailer lines.

## Review Focus

1. **No JS / agent readers:** the static HTML of `/` contains the full default command (today mode, web+api) inside `<pre>`, and every piece chip with its pressed state. Test in Task 4 (`renderToString`).
2. **360 px wide:** long command lines, the 34px tool names and the terminal card wrap instead of overflowing. Checked in Task 7 (`scrollWidth <= innerWidth` at 360).
3. **Clipboard missing or denied:** copy button hidden when `navigator.clipboard` is absent; "Copy failed" when `writeText` rejects. Tests in Task 4.
4. **Repeated copy clicks:** "Copied" stays 1400 ms after the _last_ click; an older timer never flips it back early. Test in Task 4.
5. **Every piece deselected:** commands stay valid shell with no dangling `\` or trailing spaces. Tests in Task 2.

---

### Task 1: Stack swap (React, cascivo, fonts, tokens)

**Files:**

- Modify: `pnpm-workspace.yaml`, `apps/site/package.json`, `apps/site/astro.config.mjs`, `apps/site/tsconfig.json`, `apps/site/src/layouts/Base.astro`
- Create: `apps/site/src/styles/tokens.css`, `apps/site/src/styles/global.css`, `apps/site/src/env.d.ts`, `apps/site/src/lib/cascivo.test.tsx`
- Delete: `apps/site/src/components/copy-button.tsx`, `apps/site/src/components/copy-button.test.tsx`
- Modify temporarily: `apps/site/src/components/GetStarted.astro` (drop the CopyButton import so the build passes; Task 6 rewrites it)

**Interfaces:**

- Produces: CSS custom properties `--paper --surface --ink --ink-2 --muted --line --line-dashed --chip --accent --accent-ink --dark-line --dark-text-2` and `--font-sans` (`"Archivo Variable", system-ui, sans-serif`), `--font-mono` (`"JetBrains Mono", ui-monospace, monospace`); utility classes in `global.css`: `.wrap` (max-width/padding), `.label` (mono 13px muted section label), `.h2`, `.btn`, `.btn-primary`, `.btn-outline`. `Base.astro` props unchanged (`title`, `description`).

- [ ] **Step 1: Write the failing test** `src/lib/cascivo.test.tsx` (node env):

```tsx
import { renderToString } from "react-dom/server";
import { Badge, Button } from "@cascivo/react";
import { expect, test } from "vite-plus/test";

test("cascivo renders to static HTML under React", () => {
  const html = renderToString(<Button variant="secondary">Go</Button>);
  expect(html).toContain("<button");
  expect(html).toContain('data-variant="secondary"');
  expect(renderToString(<Badge>pre-release</Badge>)).toContain("pre-release");
});
```

- [ ] **Step 2:** `pnpm --filter @keelstackhq/site test` → FAIL (cannot resolve `@cascivo/react` / jsx is preact).
- [ ] **Step 3:** Add catalog pins (Global Constraints). Site deps: `@astrojs/react`, `@cascivo/react`, `@preact/signals-react`, `react`, `react-dom`, `@fontsource-variable/archivo`, `@fontsource/jetbrains-mono`, `astro`; devDeps add `@testing-library/react`, `@testing-library/dom`, `@types/react`, `@types/react-dom`; remove `preact`, `@astrojs/preact`. `astro.config.mjs`: `integrations: [react()]`. `tsconfig.json`: `compilerOptions: { "jsxImportSource": "react" }`. `env.d.ts`: `declare module "*.css";`. `pnpm install`.
- [ ] **Step 4:** Tokens + global styles. `Base.astro` imports, in order: `@fontsource-variable/archivo/wdth.css` (wdth+wght axes; if that entry does not exist use the file the package README names for both axes), `@fontsource/jetbrains-mono/latin-400.css`, `latin-600.css`, `@cascivo/themes/light.css`, then `tokens.css`, `global.css`. Map cascivo vars the used components read (find names in `node_modules/@cascivo/react/dist/{button,badge,segmented-control}/*.css`) to the site tokens in `tokens.css`. Body: paper background, ink text, `--font-sans`, antialiased; `a` ink, hover accent-ink; focus ring rule; `color-scheme: light`; favicon: accent square with a rotated ink diamond (brief thumbnail SVG). Keep the skip link.
- [ ] **Step 5:** Delete the copy-button files; strip the island from `GetStarted.astro`.
- [ ] **Step 6:** `pnpm --filter @keelstackhq/site test` → PASS; `pnpm --filter @keelstackhq/site build` → exit 0; `grep -c "@font-face" apps/site/dist/_astro/*.css` ≥ 1; `grep -ri preact apps/site/dist | grep -v signals` → nothing.
- [ ] **Step 7:** `pnpm run ci` green, then commit `feat(site): switch to React islands, cascivo, self-hosted fonts and brief tokens`.

### Task 2: `buildCommand`

**Files:** Create `apps/site/src/lib/command.ts`, `apps/site/src/lib/command.test.ts`

**Interfaces:**

- Produces: `type Mode = "today" | "splice"`; `interface Piece { piece: string; name: string }`; `const PIECES: readonly Piece[]` (order: preact-app:web, hono-worker:api, ui-storybook:ui, astro-site:site, rust-tool:cli, tauri-app:desktop, docs:docs); `const DEFAULT_ON: readonly string[] = ["web", "api"]`; `function buildCommand(on: ReadonlySet<string>, mode: Mode): string` (keys are piece `name`s); `const CAPTIONS: Record<Mode, string>` = today `"In a hull checkout."`, splice `"Preview: flags unconfirmed until splice 0.1 ships."`.

- [ ] **Step 1: Failing tests** in `command.test.ts`:

```ts
const def = new Set(DEFAULT_ON);
test("today, defaults", () =>
  expect(buildCommand(def, "today")).toBe(
    "node scripts/render.mjs new --template template --out ../my-repo \\\n" +
      "  --piece base --piece preact-app:web --piece hono-worker:api --data scope=acme\n" +
      "cd ../my-repo && git init && pnpm install && pnpm run ci",
  ));
test("splice, defaults", () =>
  expect(buildCommand(def, "splice")).toBe(
    "pnpm dlx @keelstackhq/splice new gh:keelstackhq/hull//template my-repo \\\n" +
      "  --piece preact-app:web --piece hono-worker:api",
  ));
test("keeps PIECES order whatever the insertion order", () =>
  expect(buildCommand(new Set(["docs", "web"]), "splice")).toContain(
    "--piece preact-app:web --piece docs:docs",
  ));
test("today, nothing selected", () =>
  expect(buildCommand(new Set(), "today")).toContain("  --piece base --data scope=acme\n"));
test("splice, nothing selected: no dangling backslash", () =>
  expect(buildCommand(new Set(), "splice")).toBe(
    "pnpm dlx @keelstackhq/splice new gh:keelstackhq/hull//template my-repo",
  ));
test("no line ends in a space", () => {
  for (const mode of ["today", "splice"] as const)
    for (const on of [def, new Set<string>()]) expect(buildCommand(on, mode)).not.toMatch(/ \n| $/);
});
test("PIECES lists the seven pieces", () =>
  expect(PIECES.map((p) => `${p.piece}:${p.name}`)).toEqual([
    "preact-app:web",
    "hono-worker:api",
    "ui-storybook:ui",
    "astro-site:site",
    "rust-tool:cli",
    "tauri-app:desktop",
    "docs:docs",
  ]));
```

- [ ] **Step 2:** run → FAIL (module missing). **Step 3:** implement. **Step 4:** run → PASS. **Step 5:** `pnpm format`, commit `feat(site): command builder for the piece picker`.

### Task 3: Page data

**Files:** Rewrite `apps/site/src/data/tools.ts` and `tools.test.ts`; create `apps/site/src/data/content.ts`, `apps/site/src/data/content.test.ts`

**Interfaces:**

- Produces from `tools.ts`: `org` (unchanged), `interface Tool { name: "hull"|"splice"|"hoard"; role: string; status: string; statusStyle: "filled"|"outline"|"dashed"; pitch: string; detail: string; detailMono: boolean; repo: string }`, `tools: Tool[]`. Values from the brief: roles `01 · monorepo template` / `02 · bootstrap + update` / `03 · cached CI`; statuses `pre-0.1` filled, `under construction` outline, `not released yet` dashed; pitch and detail lines verbatim from the reference HTML; splice detail is mono.
- Produces from `content.ts`: `hero = { eyebrow, variants: { seconds: {lead, highlight}, skip: {...}, agent: {...} }, active: "seconds", body }`; `terminal = { title: "~/my-repo · splice 0.1 preview", command: string[] /*3 lines*/, checks: { text: string; dim?: string }[] /*4*/, turn: "→ your turn: write the app" }`; `stats: { value: string; caption: string }[]` (4); `split = { handles: string[] /*12 chips*/, youWrite: string }`; `agentRows: { key: string; text: string }[]` (4); `current: { label: string; title: string; text: string; accent?: boolean }[]` (3, last accent); `sections: Record<"split"|"tools"|"agents"|"current"|"start", { label: string; heading: string }>` with labels `01 · The split` … `05 · Get started`; `footerLinks: { label: string; href: string }[]` (hull, splice, hoard, decisions → `${org.url}/decisions`); `nav: { label: string; href: string }[]` (`#tools`, `#agents`, `#start`, GitHub ↗ → org url).
- Remove `fitSteps`, `quickStart`, `spliceStart`.

- [ ] **Step 1: Failing tests** (keep the existing D-001 tests, adapted): tools order; `repo === org.url/name`; statuses `["pre-0.1","under construction","not released yet"]`; `sections` labels match `/^0[1-5] · /` in order 01..05; `stats.length === 4` and every value starts with `"1 "`; `split.handles.length === 12`; `agentRows.map(r=>r.key)` equals `["CLAUDE.md","--json --yes","ci-ok","update bot"]`; `current.map(c=>c.title)` equals `["Replaced","3-way merged","Stay yours"]` and only the last has `accent`; every footer/nav href starts with `#` or `https://github.com/keelstackhq`; `JSON.stringify` of all exports has no `@keelstack/`, `github.com/keelstack/`, `gh:keelstack/` and no empty strings.
- [ ] **Step 2:** run → FAIL. **Step 3:** implement with the brief's copy. **Step 4:** PASS (other files importing removed exports will break the build; that is fixed in Task 5/6, so run only the data tests here). **Step 5:** commit `feat(site): page copy for the redesign`.

### Task 4: `PiecePicker` island

**Files:** Create `apps/site/src/components/PiecePicker.tsx`, `apps/site/src/components/PiecePicker.test.tsx`, `apps/site/src/components/PiecePicker.css`

**Interfaces:**

- Consumes: `PIECES`, `DEFAULT_ON`, `buildCommand`, `CAPTIONS`, `Mode` from Task 2.
- Produces: `export default function PiecePicker(): JSX.Element` (no props), used as `<PiecePicker client:visible />`. `export const COPIED_MS = 1400`.

Markup: fixed `<span class="pp-base">base · always</span>`; per piece a cascivo `Button` (`type="button"`, `aria-pressed`, `data-on`) labelled `✓ preact-app:web` / `+ preact-app:web` (mark in `aria-hidden` span); dark card with cascivo `SegmentedControl` (`ariaLabel="Command version"`, options `today` "Today", `splice` "With splice 0.1") and a cascivo `Button` "Copy" (rendered only when `typeof navigator !== "undefined" && navigator.clipboard`; render it after mount to keep SSR/hydration equal); `<pre><code>` with the command; caption `<p>`; `<span aria-live="polite" class="visually-hidden">` with the copy status. Styles from the reference HTML (chips on = accent bg + ink border; off = transparent + ink border; mode track `#22242A`, selected segment accent bg/ink text, unselected `#C9C7C0`; copy button `1px solid #3A3C42`, hover border accent). Copy logic: clear the pending timer on each click; on success label "Copied" for `COPIED_MS`; on rejection "Copy failed".

- [ ] **Step 1: Failing tests** (`// @vitest-environment happy-dom`, `@testing-library/react`, fake timers):
  - `renders web and api pressed, others not, and the today command` (`aria-pressed` per button; `pre` text equals `buildCommand(new Set(DEFAULT_ON),"today")`).
  - `server render contains the default command and pressed states` (`renderToString(<PiecePicker/>)` contains `--piece hono-worker:api --data scope=acme` and `aria-pressed="true"` twice).
  - `toggling a piece updates aria-pressed and the command` (click `astro-site:site` → pressed, command contains `--piece astro-site:site`; click again → removed).
  - `mode switch changes command and caption` (click radio "With splice 0.1" → `aria-checked="true"`, command starts `pnpm dlx`, caption = `CAPTIONS.splice`).
  - `copy writes the current command and shows Copied for 1400 ms` (stub `navigator.clipboard.writeText` resolving; after click label "Copied", `advanceTimersByTime(1399)` still "Copied", `+1` → "Copy").
  - `a second click restarts the 1400 ms` (click, advance 1000, click, advance 1000 → still "Copied").
  - `copy failure shows Copy failed` (`writeText` rejects).
  - `no copy button without the Clipboard API`.
- [ ] **Step 2:** run → FAIL. **Step 3:** implement. **Step 4:** PASS. **Step 5:** commit `feat(site): piece picker island with cascivo controls`.

### Task 5: Top half: Nav, Hero, StatStrip, Split

**Files:** Create `apps/site/src/components/{Nav,Hero,HeroTerminal,StatStrip,Split,SectionHead}.astro`; delete `Header.astro`, `Fit.astro`; modify `apps/site/src/pages/index.astro` (render `Nav`, then `<main id="main">` with `Hero`, `StatStrip`, `Split`, then the old `ToolCard`/`GetStarted`/`Footer` are replaced in Task 6 — until then keep only sections that build).

**Interfaces:**

- Consumes: `hero`, `terminal`, `stats`, `split`, `sections`, `nav`, `org` (Task 3); `.wrap`, `.label`, `.h2`, `.btn*` (Task 1).
- Produces: `SectionHead.astro` props `{ label: string; heading: string; id?: string; dark?: boolean; maxWidth?: string }` → mono label + `<h2>` (label accent on dark). Used again in Task 6.

Use cascivo `Badge` for the `pre-release` pill and `Button asChild` around `<a>` for the two hero CTAs (classes `.btn-primary`, `.btn-outline`). Exact styles per the reference HTML lines for nav, header, stat strip and split. Terminal: `aria-label="Example splice session (preview)"`, decorative dots `aria-hidden`.

- [ ] **Step 1:** `pnpm --filter @keelstackhq/site build` and check dist: `grep -c 'all your logic.' dist/index.html` = 1, `grep -c 'pre-release' dist/index.html` ≥ 1, `grep -c '1 pull request' dist/index.html` = 1, `grep -c 'The logic.' dist/index.html` = 1, `grep -c '<script' dist/index.html` = 0.
- [ ] **Step 2:** `pnpm run ci` green; commit `feat(site): nav, hero, stat strip and split sections`.

### Task 6: Bottom half: Tools, Agents, StaysCurrent, GetStarted, Footer, 404

**Files:** Create `apps/site/src/components/{Tools,Agents,StaysCurrent}.astro`; rewrite `ToolCard.astro`, `GetStarted.astro`, `Footer.astro`, `apps/site/src/pages/404.astro`; finish `index.astro` (section order: Hero, StatStrip, Split, Tools, Agents, StaysCurrent, GetStarted; Footer after `main`; title `keelstack: seconds to a repo, then it's all your logic`, description from hero body).

**Interfaces:** Consumes `tools`, `agentRows`, `current`, `sections`, `footerLinks`, `org`, `SectionHead`, `PiecePicker`.

ToolCard status pill: cascivo `Badge` with a site class per `statusStyle`. Agents table as `<dl>` rows (`grid-template-columns: 150px minmax(0,1fr)`, collapsing to one column under 420px). GetStarted: `id="start"`, white background, intro paragraph with `<code>pnpm install</code>`, `<PiecePicker client:visible />`. Footer: `<footer>` accent background, "Skip the setup." (as `<p>` styled as display text; not a heading), CTAs, link row with `<nav aria-label="Projects">`. 404: Nav + short block ("Page not found", link home) + Footer.

- [ ] **Step 1:** build; check dist: `grep -c 'id="tools"' dist/index.html` = 1, same for `agents`, `start`; `grep -c 'astro-island' dist/index.html` = 1; `grep -c 'scope=acme' dist/index.html` ≥ 1 (SSR command); `grep -c 'Skip the setup.' dist/index.html` = 1; `dist/404.html` exists.
- [ ] **Step 2:** `pnpm run ci` green; commit `feat(site): tools, agents, stays current, get started, footer, 404`.

### Task 7: `/llms.txt`, docs and browser verification

**Files:** Create `apps/site/src/lib/llms.ts`, `apps/site/src/lib/llms.test.ts`, `apps/site/src/pages/llms.txt.ts`; modify `docs/agents/project.md`, `apps/site/CLAUDE.md`.

**Interfaces:** Produces `function buildLlmsTxt(): string`; the endpoint exports `GET = () => new Response(buildLlmsTxt(), { headers: { "Content-Type": "text/plain; charset=utf-8" } })`.

- [ ] **Step 1: Failing test:** output starts with `# keelstack\n\n> `; contains `## Tools`, each tool name with its status and repo URL; `## For agents` with the four keys; `## Get started` with `buildCommand(new Set(DEFAULT_ON),"today")` and the splice command plus `CAPTIONS.splice`.
- [ ] **Step 2:** FAIL → implement → PASS; build; `dist/llms.txt` exists and starts with `# keelstack`.
- [ ] **Step 3:** Docs. `apps/site/CLAUDE.md`: React islands via `@astrojs/react` with cascivo (`astro.config.mjs` diverges from the template on purpose), self-hosted fonts, light only, one island. `docs/agents/project.md`: replace the "No web fonts" / "copy-button island" convention with the new rules, point the design spec line at the redesign spec, add a Progress entry, note the template-owned `astro.config.mjs` change.
- [ ] **Step 4: Browser check** (Playwright, `executablePath: '/opt/pw-browsers/chromium'`, script in the scratchpad, against `pnpm --filter @keelstackhq/site preview`): full-page screenshots at 360, 390, 1280 px; at each width `document.documentElement.scrollWidth <= innerWidth`; zero console errors; scroll to `#start`, click `astro-site:site` and the "With splice 0.1" radio, assert the `<pre>` text; Tab to the first nav link and screenshot the focus ring. Report transferred bytes for `/` (HTML+CSS+fonts) and the island JS separately.
- [ ] **Step 5:** `pnpm run ci` green; commit `feat(site): llms.txt and docs for the redesign`.
