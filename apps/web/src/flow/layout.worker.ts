import type { LayoutEdge, LayoutNode, LayoutResult } from "./layout.ts";
import { runElkLayout } from "./layout.ts";

/**
 * Off-main-thread ELK layout (SPK-010): the layered algorithm on thousands
 * of nodes takes seconds — it must never block the render loop. The worker
 * receives the node/edge sets and returns positions; a token lets the
 * panel discard stale results after a re-layout superseded this one.
 */
self.onmessage = async (
  event: MessageEvent<{
    token: number;
    nodes: LayoutNode[];
    edges: LayoutEdge[];
  }>,
) => {
  const { token, nodes, edges } = event.data;
  try {
    const result: LayoutResult = await runElkLayout(nodes, edges);
    self.postMessage({ token, ...result });
  } catch (error) {
    self.postMessage({ token, error: String(error) });
  }
};
