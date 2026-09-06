import {
  Background,
  Controls,
  type Edge,
  MiniMap,
  type Node,
  ReactFlow,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import type { GraphState, SubjectNode } from "@vistalith/client";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  FLOW_NODE_HEIGHT,
  FLOW_NODE_WIDTH,
  type LayoutEdge,
  type LayoutNode,
  runElkLayout,
} from "../flow/layout.ts";

/**
 * Workflow/agent lens (slice 21, SPK-010, ADR-015): React Flow + ELK.
 * Projection only — SDDK stays the workflow authority (ADR-015), the lens
 * reads whatever the graph already knows.
 *
 * Scale contract (SPK-010): layout runs OFF the main thread (Web Worker,
 * with a same-thread fallback for test environments) and is keyed to the
 * node/edge SET — live status updates flow through the polled graph query
 * and re-render badges in place without re-layout, so positions never
 * jump while a run progresses. React Flow virtualizes: only visible nodes
 * render, which is what makes 10k nodes feasible.
 */

const FLOW_KINDS = new Set([
  "workflow",
  "workflow-node",
  "workflow-run",
  "agent",
  "frame",
]);

interface FlowModel {
  nodes: LayoutNode[];
  edges: LayoutEdge[];
  signature: string;
}

function buildFlowModel(graph: GraphState): FlowModel {
  const included = graph.subjects.filter((subject) =>
    FLOW_KINDS.has(subject.subject.kind),
  );
  const identities = new Set(included.map((subject) => subjectRefKey(subject)));
  const nodes: LayoutNode[] = included.map((subject) => ({
    id: subjectRefKey(subject),
    label:
      (subject.properties?.name as string | undefined) ?? subject.subject.id,
    kind: subject.subject.kind,
    status: subject.properties?.status as string | undefined,
  }));
  const edges: LayoutEdge[] = [];
  for (const fact of graph.relations) {
    const source = identityString(fact.relation.from);
    const target = identityString(fact.relation.to);
    if (!identities.has(source) || !identities.has(target)) continue;
    edges.push({
      id: `${source}-${fact.relation.kind}-${target}`,
      source,
      target,
      kind: fact.relation.kind,
    });
  }
  // Structural signature: only a change in the node/edge SET re-layouts.
  // Structural signature: node/edge id sets ONLY. The revision changes on
  // every event (live status updates) and must never re-trigger layout.
  const signature = `${nodes.length}:${edges.length}:${hash(idsSignature(nodes, edges))}`;
  return { nodes, edges, signature };
}

function idsSignature(nodes: LayoutNode[], edges: LayoutEdge[]): string {
  return [
    nodes.map((node) => node.id).join(","),
    edges.map((edge) => edge.id).join(","),
  ].join("|");
}

function hash(input: string): string {
  let value = 5381;
  for (let index = 0; index < input.length; index += 1) {
    value = ((value << 5) + value + input.charCodeAt(index)) | 0;
  }
  return (value >>> 0).toString(36);
}

function subjectRefKey(subject: SubjectNode): string {
  return `${subject.subject.namespace}:${subject.subject.kind}:${subject.subject.id}`;
}

function identityString(ref: {
  namespace: string;
  kind: string;
  id: string;
}): string {
  return `${ref.namespace}:${ref.kind}:${ref.id}`;
}

export function FlowPanel({ graph }: { graph: GraphState }) {
  const model = useMemo(() => buildFlowModel(graph), [graph]);
  const [positions, setPositions] = useState<
    Record<string, { x: number; y: number }>
  >({});
  const [layoutMs, setLayoutMs] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const layoutSignature = useRef<string | null>(null);
  const tokenRef = useRef(0);
  const workerRef = useRef<Worker | null>(null);

  // Layout only when the structural signature changes (SPK-010: live
  // status updates must not re-layout). Runs off the main thread when the
  // environment provides workers.
  useEffect(() => {
    if (
      model.nodes.length === 0 ||
      model.signature === layoutSignature.current
    ) {
      return;
    }
    layoutSignature.current = model.signature;
    tokenRef.current += 1;
    const token = tokenRef.current;

    const apply = (
      positions: Record<string, { x: number; y: number }>,
      durationMs: number,
    ) => {
      if (tokenRef.current !== token) return; // superseded
      setPositions(positions);
      setLayoutMs(durationMs);
    };

    let cancelled = false;
    if (typeof Worker === "function") {
      try {
        const worker = new Worker(
          new URL("../flow/layout.worker.ts", import.meta.url),
          { type: "module" },
        );
        workerRef.current = worker;
        worker.onmessage = (
          event: MessageEvent<
            | {
                token: number;
                positions: Record<string, { x: number; y: number }>;
                durationMs: number;
              }
            | { token: number; error: string }
          >,
        ) => {
          if (event.data.token !== tokenRef.current) return;
          if ("error" in event.data) setError(event.data.error);
          else apply(event.data.positions, event.data.durationMs);
        };
        worker.onerror = () => setError("layout worker failed");
        worker.postMessage({ token, nodes: model.nodes, edges: model.edges });
        return () => {
          cancelled = true;
          worker.terminate();
        };
      } catch {
        // worker construction failed (e.g. test environment) — fall through
      }
    }
    void runElkLayout(model.nodes, model.edges)
      .then((result) => {
        if (!cancelled) apply(result.positions, result.durationMs);
      })
      .catch((cause: unknown) => {
        if (!cancelled) {
          setError(cause instanceof Error ? cause.message : String(cause));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [model]);

  useEffect(() => () => workerRef.current?.terminate(), []);

  const flowNodes: Node[] = useMemo(
    () =>
      model.nodes.map((node) => ({
        id: node.id,
        position: positions[node.id] ?? { x: 0, y: 0 },
        data: {
          label: `${node.label}${node.status ? ` · ${node.status}` : ""}`,
        },
        className: [
          "flow-node",
          `flow-node-${node.kind}`,
          node.status ? `flow-status-${node.status}` : "",
        ]
          .filter(Boolean)
          .join(" "),
        draggable: false,
      })),
    [model.nodes, positions],
  );

  const flowEdges: Edge[] = useMemo(
    () =>
      model.edges.map((edge) => ({
        id: edge.id,
        source: edge.source,
        target: edge.target,
        label: edge.kind,
        className: "flow-edge",
      })),
    [model.edges],
  );

  if (model.nodes.length === 0) {
    return (
      <div className="flow-panel" data-testid="flow-panel">
        <p className="empty">
          no workflow/agent subjects in this graph — define agents or sync an
          SDDK workflow
        </p>
      </div>
    );
  }

  return (
    <div className="flow-panel" data-testid="flow-panel">
      <p className="c4-revision" data-testid="flow-stats">
        workflow lens · graph revision {graph.revision} · {model.nodes.length}{" "}
        nodes · {model.edges.length} edges ·{" "}
        {layoutMs === null ? "layouting…" : `layout ${Math.round(layoutMs)} ms`}
        {" · "}
        {typeof Worker === "function" ? "worker layout" : "inline layout"}
      </p>
      {error ? <p className="chat-error">{error}</p> : null}
      <div className="flow-canvas">
        <ReactFlow
          nodes={flowNodes}
          edges={flowEdges}
          nodeTypes={{}}
          onlyRenderVisibleElements
          fitView
          minZoom={0.02}
          maxZoom={2}
          nodesDraggable={false}
          nodesConnectable={false}
          proOptions={{ hideAttribution: true }}
        >
          <Background gap={24} />
          <Controls showInteractive={false} />
          <MiniMap pannable zoomable />
        </ReactFlow>
      </div>
      <p className="flow-legend">
        node size {FLOW_NODE_WIDTH}×{FLOW_NODE_HEIGHT} · colors by kind · status
        suffix live from the polled graph
      </p>
    </div>
  );
}
