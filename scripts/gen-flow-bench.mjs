#!/usr/bin/env node
// SPK-010 bench fixture generator: a deterministic raw-VEvent log with N
// workflow-plane subjects (workflow / workflow-node / workflow-run / agent)
// and depends_on/contains/executed_by relations, shaped exactly like
// crates/vistalith-graph/tests/fixtures/sample-world.json so `vistalithd
// --fixture` loads it. Same input flags always yield byte-identical output.
//
//   node scripts/gen-flow-bench.mjs --nodes 1000 --out /tmp/flow-bench-1k.json
//
// `--nodes N` sizes the workflow-node population; workflows, runs and
// agents scale with it (1 workflow per 25 nodes, 1 run per node/4, 1 agent
// per 10 nodes).

import { writeFileSync } from "node:fs";

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const index = args.indexOf(`--${name}`);
  return index >= 0 ? args[index + 1] : fallback;
};
const nodeCount = Number(flag("nodes", 1000));
const out = flag("out", `flow-bench-${nodeCount}.json`);
if (!Number.isInteger(nodeCount) || nodeCount <= 0) {
  console.error("--nodes must be a positive integer");
  process.exit(1);
}

// Deterministic PRNG (LCG) — no Math.random anywhere.
let seed = 0x2545f491;
const random = () => {
  seed = (seed * 1_664_525 + 1_013_904_223) >>> 0;
  return seed / 0x1_0000_0000;
};

let sequence = 0;
const event = (payload, subjects, kind) => {
  const hex = (n) => n.toString(16).padStart(12, "0");
  sequence += 1;
  return {
    event_id: `01990000-0000-7000-8000-${hex(sequence)}`,
    actor: "bench:generator",
    timestamp: "2026-09-06T00:00:00Z",
    subjects,
    correlation_id: `01990000-0000-7000-9000-${hex(sequence)}`,
    type: kind,
    payload,
  };
};

const ref = (kind, id) => ({ namespace: "work", kind, id });
const identity = (r) => `${r.namespace}:${r.kind}:${r.id}`;

const events = [];
const workflowCount = Math.max(1, Math.round(nodeCount / 25));
const agentCount = Math.max(1, Math.round(nodeCount / 10));
const runCount = Math.max(1, Math.round(nodeCount / 4));

const subjects = [];
for (let w = 0; w < workflowCount; w += 1) {
  subjects.push(ref("workflow", `flow-${w}`));
}
for (let n = 0; n < nodeCount; n += 1) {
  subjects.push(ref("workflow-node", `step-${n}`));
}
for (let r = 0; r < runCount; r += 1) {
  subjects.push(ref("workflow-run", `run-${r}`));
}
for (let a = 0; a < agentCount; a += 1) {
  subjects.push(ref("agent", `agent-${a}`));
}

for (const subject of subjects) {
  const properties = { name: subject.id };
  if (subject.kind === "workflow") {
    properties.description = `bench workflow ${subject.id}`;
  }
  if (subject.kind === "workflow-node") {
    properties.status = random() < 0.5 ? "completed" : "pending";
  }
  if (subject.kind === "workflow-run") {
    properties.status = ["running", "completed", "failed"][
      Math.floor(random() * 3)
    ];
  }
  if (subject.kind === "agent") {
    properties.role = "bench-worker";
  }
  events.push(
    event(
      {
        subject,
        authority: "authoritative",
        provenance: { source: "bench:generator" },
        properties,
      },
      [subject],
      "subject-defined",
    ),
  );
}

// workflow contains its nodes; nodes depend on their predecessor
const nodesPerWorkflow = Math.ceil(nodeCount / workflowCount);
for (let n = 0; n < nodeCount; n += 1) {
  const workflow = subjects[n % workflowCount]; // workflow, flow-k
  const step = subjects[workflowCount + n];
  events.push(
    event(
      {
        fact: {
          relation: {
            from: workflow,
            kind: "contains",
            to: step,
          },
          authority: "authoritative",
          provenance: { source: "bench:generator" },
        },
      },
      [workflow, step],
      "relation-declared",
    ),
  );
  if (n % nodesPerWorkflow !== 0 && n > 0) {
    const previous = subjects[workflowCount + n - 1];
    events.push(
      event(
        {
          fact: {
            relation: { from: step, kind: "depends_on", to: previous },
            authority: "authoritative",
            provenance: { source: "bench:generator" },
          },
        },
        [step, previous],
        "relation-declared",
      ),
    );
  }
}

// runs execute steps, agents execute runs
for (let r = 0; r < runCount; r += 1) {
  const run = subjects[workflowCount + nodeCount + r];
  const step = subjects[workflowCount + (r % nodeCount)];
  const agent = subjects[workflowCount + nodeCount + runCount + (r % agentCount)];
  events.push(
    event(
      {
        fact: {
          relation: { from: run, kind: "executed_by", to: agent },
          authority: "authoritative",
          provenance: { source: "bench:generator" },
        },
      },
      [run, agent],
      "relation-declared",
    ),
    event(
      {
        fact: {
          relation: { from: run, kind: "contributes_to", to: step },
          authority: "authoritative",
          provenance: { source: "bench:generator" },
        },
      },
      [run, step],
      "relation-declared",
    ),
  );
}

const relationCount = events.filter((e) => e.type === "relation-declared").length;
writeFileSync(
  out,
  JSON.stringify(
    {
      description: `SPK-010 bench fixture: ${subjects.length} subjects (${nodeCount} workflow-node), ${relationCount} relations. Deterministic; generated by scripts/gen-flow-bench.mjs.`,
      events,
    },
    null,
    1,
  ),
);
console.log(
  `wrote ${out}: ${subjects.length} subjects (${nodeCount} workflow-node, ` +
    `${workflowCount} workflows, ${runCount} runs, ${agentCount} agents), ` +
    `${relationCount} relations, ${events.length} events`,
);
