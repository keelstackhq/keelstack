// Serves hull's ready.mjs (the read-only assessment) at the pinned URL from src/data/hull/adopt.json,
// byte for byte from src/data/hull/ready.mjs.txt (kept as .txt so lint and fmt leave it alone).
import source from "../../data/hull/adopt.json";

const files = import.meta.glob("../../data/hull/ready.mjs.txt", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

export function getStaticPaths() {
  return [{ params: { file: source.assess.url.replace("/assess/", "") } }];
}

export const GET = () =>
  new Response(files["../../data/hull/ready.mjs.txt"], {
    headers: { "Content-Type": "text/javascript; charset=utf-8" },
  });
