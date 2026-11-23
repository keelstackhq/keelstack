// Search Console bridge: a reference Cloudflare Worker for `hoist org` (hoist spec §16, D-026).
// It holds a Google service-account key as a Worker secret, mints short-lived access tokens and
// forwards only Site Verification v1 (`/sv/*`) and Search Console v3 (`/wm/*`) calls behind one
// static bearer token. Secrets: BRIDGE_TOKEN (random, >= 32 chars), SA_KEY (the JSON key file).

const UPSTREAM = {
  "/sv/": "https://www.googleapis.com/siteVerification/v1/",
  "/wm/": "https://www.googleapis.com/webmasters/v3/",
};
const SCOPES =
  "https://www.googleapis.com/auth/siteverification https://www.googleapis.com/auth/webmasters";
const ALLOWED_METHODS = new Set(["GET", "POST", "PUT"]);
let cached = null; // { token, exp } per isolate

export default {
  async fetch(request, env) {
    if (!(await authorized(request, env))) return json(401, { error: "unauthorized" });
    if (!ALLOWED_METHODS.has(request.method)) return json(405, { error: "method not allowed" });
    const url = new URL(request.url);
    const prefix = Object.keys(UPSTREAM).find((p) => url.pathname.startsWith(p));
    if (!prefix) return json(404, { error: "unknown path" });
    const target = UPSTREAM[prefix] + url.pathname.slice(prefix.length) + url.search;
    let token;
    try {
      token = await accessToken(env);
    } catch {
      return json(502, { error: "cannot mint a Google access token; check SA_KEY" });
    }
    const body = ["POST", "PUT"].includes(request.method) ? await request.text() : undefined;
    const upstream = await fetch(target, {
      method: request.method,
      headers: { authorization: `Bearer ${token}`, "content-type": "application/json" },
      body,
      redirect: "manual",
    });
    return new Response(upstream.body, {
      status: upstream.status,
      headers: { "content-type": upstream.headers.get("content-type") ?? "application/json" },
    });
  },
};

// Constant-time comparison of SHA-256 digests, so the token length and content do not leak.
async function authorized(request, env) {
  const want = env.BRIDGE_TOKEN;
  if (!want || want.length < 32) return false;
  const header = request.headers.get("authorization") ?? "";
  const got = header.startsWith("Bearer ") ? header.slice(7) : "";
  const [a, b] = await Promise.all([digest(got), digest(want)]);
  let diff = 0;
  for (let i = 0; i < a.length; i++) diff |= a[i] ^ b[i];
  return diff === 0;
}

async function digest(text) {
  return new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text)));
}

async function accessToken(env) {
  const now = Math.floor(Date.now() / 1000);
  if (cached && cached.exp - 60 > now) return cached.token;
  const key = JSON.parse(env.SA_KEY);
  const claims = {
    iss: key.client_email,
    scope: SCOPES,
    aud: "https://oauth2.googleapis.com/token",
    iat: now,
    exp: now + 3600,
  };
  const unsigned = `${b64url(JSON.stringify({ alg: "RS256", typ: "JWT" }))}.${b64url(JSON.stringify(claims))}`;
  const signingKey = await crypto.subtle.importKey(
    "pkcs8",
    pemToDer(key.private_key),
    { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const sig = await crypto.subtle.sign("RSASSA-PKCS1-v1_5", signingKey, new TextEncoder().encode(unsigned));
  const resp = await fetch("https://oauth2.googleapis.com/token", {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      grant_type: "urn:ietf:params:oauth:grant-type:jwt-bearer",
      assertion: `${unsigned}.${b64url(sig)}`,
    }),
  });
  if (!resp.ok) throw new Error(`token endpoint ${resp.status}`);
  const { access_token, expires_in } = await resp.json();
  cached = { token: access_token, exp: now + (expires_in ?? 3600) };
  return access_token;
}

function b64url(input) {
  const bytes = typeof input === "string" ? new TextEncoder().encode(input) : new Uint8Array(input);
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function pemToDer(pem) {
  const b64 = pem.replace(/-----[^-]+-----/g, "").replace(/\s+/g, "");
  return Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)).buffer;
}

function json(status, value) {
  return new Response(JSON.stringify(value), {
    status,
    headers: { "content-type": "application/json" },
  });
}
