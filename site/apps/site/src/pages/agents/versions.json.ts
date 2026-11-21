// /agents/versions.json: the versions the agent doc names, byte for byte from agents/versions.json.
const [json] = Object.values(
  import.meta.glob("../../../../../agents/versions.json", {
    query: "?raw",
    import: "default",
    eager: true,
  }),
) as string[];

export const GET = () =>
  new Response(json, { headers: { "Content-Type": "application/json; charset=utf-8" } });
