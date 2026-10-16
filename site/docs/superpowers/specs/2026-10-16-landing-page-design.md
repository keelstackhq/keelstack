# keelstack landing page: design

Date: 2026-10-16 · Status: approved (defaults, confirmed by the owner) · App: `apps/site`

## Goal

One static page that says what keelstack is, what each tool does, how the tools fit together, and
how to start today. Plain and technical; no marketing claims.

- **Audience:** developers and teams who start or maintain TypeScript/Rust monorepos.
- **Tone:** plain and technical, no fluff. Copy comes from the hull, splice and hoard READMEs and
  specs. Never invent features, numbers or dates; gaps are `TODO(copy)`.
- **Style:** clean and minimal, system font stack, one accent colour, light and dark mode through
  `prefers-color-scheme` (no toggle, no JS).
- **Domain:** `TODO(domain)` (not chosen yet; listed under Blocked in `docs/agents/project.md`).

## Sections (one page, top to bottom)

1. **Header:** wordmark "keelstack", skip link, in-page nav (Tools, How it fits, Get started).
2. **Hero:** `h1` "keelstack" and the one-line pitch "Tools for bootstrapping projects."
   (the phrase every README uses), plus one sentence: start a monorepo from a template, keep it
   current, cache its CI. A note that all three tools are pre-release.
3. **Tools:** one card per tool, each with name, one-line summary, two or three facts from its
   README, its status, and a link to `github.com/keelstackhq/<tool>`.
   - hull: opinionated, AI-native monorepo template; owns structure, stack, tool versions and
     config; app code stays yours. Status: pre-0.1.
   - splice: start a repo from versioned template pieces and keep it current; Copier alternative
     in one static binary, no Python, no Rust toolchain. Status: under construction.
   - hoard: Turborepo (and Vite+) remote cache for GitHub Actions in one line of YAML; GitHub
     Actions cache or your own R2 bucket. Status: not released yet.
4. **How they fit together:** an ordered list of three steps: template (hull) → bootstrap and
   update (`splice new`, `splice update`) → cached CI (`uses: keelstackhq/hoard@v1`).
5. **Get started:** hull's dev renderer command (the only path that works today) with a copy
   button, and the planned `splice new` equivalent, marked "flags unconfirmed" as in hull's README.
6. **Footer:** GitHub links (org, hull, splice, hoard, decisions), license line `TODO(copy)`.

Names follow D-001: `keelstackhq` org and npm scope, even where splice's README still says
`keelstack`.

## Structure

- `src/layouts/Base.astro`: document shell, meta, global CSS (inlined by Astro).
- `src/data/tools.ts`: tool data (name, summary, facts, status, repo URL). Tested: every tool links
  under `https://github.com/keelstackhq/`, has a non-empty summary and status.
- `src/components/*.astro`: Header, ToolCard, Fit, GetStarted, Footer. Zero JS.
- `src/components/copy-button.tsx`: the only island (`client:visible`). Copies the snippet text
  with `navigator.clipboard.writeText`, announces "Copied" in an `aria-live` region, and renders
  nothing without the Clipboard API, so the page works without JS. Tested with happy-dom.
- `src/pages/404.astro`: `wrangler.jsonc` uses `not_found_handling: "404-page"`.

## Constraints

- Static output only; no client JS except the copy button (Preact, about 4 KB gzipped).
- Under ~100 KB transferred per page; no images except an inline SVG favicon.
- Accessibility: semantic landmarks, one `h1`, skip link, visible focus rings, WCAG AA contrast in
  both themes, keyboard reachable controls, `prefers-reduced-motion` respected.
- Responsive from 320 px; cards stack on narrow screens, three columns from about 900 px.
- No new dependencies beyond the template catalog.

## Out of scope

Deploying (needs Cloudflare secrets), a docs site, analytics, a theme toggle, blog.
