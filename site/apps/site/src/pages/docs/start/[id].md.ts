import { scenarios } from "../../../data/scenarios";
import { startScenarioMd } from "../../../lib/start-md";

export const getStaticPaths = () => scenarios.map((s) => ({ params: { id: s.id } }));

export const GET = ({ params }: { params: { id: string } }) =>
  new Response(startScenarioMd(params.id), {
    headers: { "Content-Type": "text/markdown; charset=utf-8" },
  });
