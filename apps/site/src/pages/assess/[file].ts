// Serves hull's ready.mjs (the read-only assessment) at /assess/<tag>.mjs, one file per hull tag,
// byte for byte from src/data/hull/assess/<tag>.mjs.txt (kept as .txt so lint and fmt leave it
// alone). Every tag in `hosted` in src/data/hull/adopt.json stays served: old links pin them.
import source from "../../data/hull/adopt.json";

const files = import.meta.glob("../../data/hull/assess/*.mjs.txt", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

export function getStaticPaths() {
  return Object.keys(source.hosted).map((tag) => ({ params: { file: `${tag}.mjs` } }));
}

export const GET = ({ params }: { params: { file: string } }) =>
  new Response(files[`../../data/hull/assess/${params.file}.txt`], {
    headers: { "Content-Type": "text/javascript; charset=utf-8" },
  });
