import { buildLlmsFull } from "../lib/docs-md";

export const GET = () =>
  new Response(buildLlmsFull(), { headers: { "Content-Type": "text/plain; charset=utf-8" } });
