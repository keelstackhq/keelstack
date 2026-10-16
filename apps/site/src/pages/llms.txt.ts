import { buildLlmsTxt } from "../lib/llms";

export const GET = () =>
  new Response(buildLlmsTxt(), { headers: { "Content-Type": "text/plain; charset=utf-8" } });
