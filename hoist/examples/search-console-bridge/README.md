# Search Console bridge (reference Worker)

Google access tokens expire hourly, so a static proxy-injected credential cannot reach the
Search Console or Site Verification APIs. This Worker holds the service-account key and gives
`hoist org` one static bearer token instead (hoist spec §16, decision D-026). It is reference
code: deploy it in your own Cloudflare account; hoist never deploys it.

It forwards only `GET`, `POST` and `PUT` on `/sv/*` (Site Verification v1) and `/wm/*`
(Search Console v3). Anything else is refused.

## Human steps (once)

1. Google Cloud: create a project, enable the "Site Verification API" and the "Google Search
   Console API", create a service account and a JSON key.
2. Deploy: `npx wrangler deploy` in this folder, then `npx wrangler secret put SA_KEY` (paste
   the JSON key) and `npx wrangler secret put BRIDGE_TOKEN` (a random value of 32+ characters,
   e.g. `openssl rand -hex 32`). Delete the local key file afterwards.
3. Give hoist the token: set `HOIST_SEARCH_CONSOLE_TOKEN`, or store it as a network secret for
   the bridge host so a proxy injects it (the process then holds the dummy value
   `HOIST_SEARCH_CONSOLE_TOKEN=proxy-injected`).
4. Set `[search_console] bridge = "https://search-console-bridge.<account>.workers.dev"` in
   `org.toml` and run `hoist org apply --area search_console`.

The service account becomes a verified owner of each domain it verifies. To see the property in
the Search Console UI, add your own account as an owner there (a human step).
