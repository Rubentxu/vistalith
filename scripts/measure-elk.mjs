#!/usr/bin/env node
// SPK-010 measurement: time the ELK layered layout over synthetic
// workflow-plane graphs (same shapes the bench fixture produces). This is
// the evidence for the off-main-thread decision in the spike report.
//
//   node scripts/measure-elk.mjs 1000 10000

import { performance } from "node:perf_hooks";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

// elkjs is a dependency of apps/web (the only consumer)
const require_ = createRequire(
  fileURLToPath(new URL("../apps/web/package.json", import.meta.url)),
);
const ELK = require_("elkjs/lib/elk.bundled.js").default;

const build = (nodeCount) => {
  const workflowCount = Math.max(1, Math.round(nodeCount / 25));
  const runCount = Math.max(1, Math.round(nodeCount / 4));
  const agentCount = Math.max(1, Math.round(nodeCount / 10));
  const nodes = [];
  const edges = [];
  let edgeSeq = 0;
  const edge = (source, target, kind) => ({
    id: `e${(edgeSeq += 1)}`,
    source,
    target,
    kind,
  });
  for (let w = 0; w < workflowCount; w += 1) {
    nodes.push({ id: `work:workflow:flow-${w}`, label: `flow-${w}`, kind: "workflow" });
  }
  for (let n = 0; n < nodeCount; n += 1) {
    nodes.push({ id: `work:workflow-node:step-${n}`, label: `step-${n}`, kind: "workflow-node" });
    edges.push(
      edge(
        `work:workflow:flow-${n % workflowCount}`,
        `work:workflow-node:step-${n}`,
        "contains",
      ),
    );
    if (n % Math.ceil(nodeCount / workflowCount) !== 0 && n > 0) {
      edges.push(
        edge(
          `work:workflow-node:step-${n}`,
          `work:workflow-node:step-${n - 1}`,
          "depends_on",
        ),
      );
    }
  }
  for (let r = 0; r < runCount; r += 1) {
    nodes.push({ id: `work:workflow-run:run-${r}`, label: `run-${r}`, kind: "workflow-run" });
    edges.push(
      edge(
        `work:workflow-run:run-${r}`,
        `work:workflow-node:step-${r % nodeCount}`,
        "contributes_to",
      ),
    );
  }
  for (let a = 0; a < agentCount; a += 1) {
    nodes.push({ id: `work:agent:agent-${a}`, label: `agent-${a}`, kind: "agent" });
    edges.push(
      edge(
        `work:workflow-run:run-${(a * 4) % runCount}`,
        `work:agent:agent-${a}`,
        "executed_by",
      ),
    );
  }
  return { nodes, edges };
};

const elk = new ELK();
const routing = process.env.ELK_ROUTING === "polyline"
  ? "POLYLINE"
  : "ORTHOGONAL";
const layout = async ({ nodes, edges }) => {
  const start = performance.now();
  await elk.layout({
    id: "flow-root",
    layoutOptions: {
      "elk.algorithm": "layered",
      "elk.direction": "DOWN",
      "elk.layered.spacing.nodeNodeBetweenLayers": "48",
      "elk.spacing.nodeNode": "24",
      "elk.edgeRouting": routing,
    },
    children: nodes.map((node) => ({
      id: node.id,
      width: 176,
      height: 44,
    })),
    edges: edges.map((edgeItem) => ({
      id: edgeItem.id,
      sources: [edgeItem.source],
      targets: [edgeItem.target],
    })),
  });
  return performance.now() - start;
};

for (const size of process.argv.slice(2).map(Number)) {
  const model = build(size);
  // warm-up run (JIT), then three measured runs
  await layout(model);
  const runs = [];
  for (let index = 0; index < 3; index += 1) {
    runs.push(await layout(model));
  }
  const best = Math.min(...runs).toFixed(0);
  const median = runs.sort((a, b) => a - b)[1].toFixed(0);
  console.log(
    `[${routing.toLowerCase()}] ${model.nodes.length} nodes / ${model.edges.length} edges: ` +
      `median ${median} ms (best ${best} ms of 3)`,
  );
}
