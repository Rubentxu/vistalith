import ELK from "elkjs/lib/elk.bundled.js";

/**
 * ELK layout runner for the workflow/agent lens (SPK-010, ADR-015). Pure
 * and dependency-injectable: the Web Worker (`layout.worker.ts`) runs this
 * off the main thread, tests call it directly, and the spike measurements
 * replay it in Node. Layout is a pure function of the node/edge sets —
 * status changes never re-trigger it.
 */

export const FLOW_NODE_WIDTH = 176;
export const FLOW_NODE_HEIGHT = 44;

export interface LayoutNode {
  id: string;
  label: string;
  kind: string;
  status?: string;
}

export interface LayoutEdge {
  id: string;
  source: string;
  target: string;
  kind: string;
}

export interface LayoutResult {
  positions: Record<string, { x: number; y: number }>;
  durationMs: number;
}

export async function runElkLayout(
  nodes: LayoutNode[],
  edges: LayoutEdge[],
): Promise<LayoutResult> {
  const elk = new ELK();
  const start = performance.now();
  const layouted = await elk.layout({
    id: "flow-root",
    layoutOptions: {
      "elk.algorithm": "layered",
      "elk.direction": "DOWN",
      "elk.layered.spacing.nodeNodeBetweenLayers": "48",
      "elk.spacing.nodeNode": "24",
      "elk.edgeRouting": "ORTHOGONAL",
    },
    children: nodes.map((node) => ({
      id: node.id,
      width: FLOW_NODE_WIDTH,
      height: FLOW_NODE_HEIGHT,
    })),
    edges: edges.map((edge) => ({
      id: edge.id,
      sources: [edge.source],
      targets: [edge.target],
    })),
  });
  const positions: LayoutResult["positions"] = {};
  for (const child of layouted.children ?? []) {
    positions[child.id] = { x: child.x ?? 0, y: child.y ?? 0 };
  }
  return { positions, durationMs: performance.now() - start };
}
