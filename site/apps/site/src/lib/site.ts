// The production origin (Cloudflare custom domain).
export const SITE_URL = "https://keelstack.dev";

export function canonicalUrl(pathname: string): string {
  const url = new URL(pathname, SITE_URL);
  return `${url.origin}${url.pathname}`;
}
