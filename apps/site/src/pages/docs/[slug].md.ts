import type { DocPage } from "../../data/docs";
import { readyDocs } from "../../data/docs";
import { docMarkdown } from "../../lib/docs-md";

export function getStaticPaths() {
  return readyDocs().map((page) => ({
    params: { slug: page.slug === "" ? "index" : page.slug },
    props: { page },
  }));
}

export const GET = ({ props }: { props: { page: DocPage } }) =>
  new Response(docMarkdown(props.page), {
    headers: { "Content-Type": "text/markdown; charset=utf-8" },
  });
