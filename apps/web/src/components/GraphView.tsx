import type {
  AuthorityClass,
  GraphState,
  SubjectNode,
} from "@vistalith/client";
import { subjectRefToString } from "@vistalith/client";
import {
  adjacencyOf,
  computeLayout,
  NODE_HEIGHT,
  NODE_WIDTH,
} from "../layout.ts";
import { useSelection } from "../state/selection.ts";

const AUTHORITY_CLASS: Record<AuthorityClass, string> = {
  authoritative: "authority-authoritative",
  derived: "authority-derived",
  advisory: "authority-advisory",
  ephemeral: "authority-ephemeral",
};

const ADJACENT_CLASS: Record<AuthorityClass, string> = {
  authoritative: "edge-authoritative",
  derived: "edge-derived",
  advisory: "edge-advisory",
  ephemeral: "edge-ephemeral",
};

/**
 * Graph lens (slice 2): renders subjects and typed edges. Clicking a node
 * selects its SubjectRef — the same identity every other lens uses.
 */
export function GraphView({ graph }: { graph: GraphState }) {
  const selected = useSelection((s) => s.selected);
  const toggle = useSelection((s) => s.toggle);
  const layout = computeLayout(graph);
  const adjacent = selected ? adjacencyOf(graph, selected) : new Set<string>();
  const selectedIdentity = selected ? subjectRefToString(selected) : null;

  return (
    <div className="graph-view" data-testid="graph-view">
      <svg
        width={layout.width}
        height={layout.height}
        viewBox={`0 0 ${layout.width} ${layout.height}`}
        role="img"
        aria-label="Semantic World Graph"
      >
        <defs>
          <marker
            id="arrow"
            viewBox="0 0 10 10"
            refX="9"
            refY="5"
            markerWidth="7"
            markerHeight="7"
            orient="auto-start-reverse"
          >
            <path d="M 0 0 L 10 5 L 0 10 z" className="edge-arrow" />
          </marker>
        </defs>

        {layout.edges.map((edge) => {
          const active =
            selectedIdentity !== null && touches(edge.fact, selectedIdentity);
          return (
            <g
              key={edge.identity}
              className={[
                "edge",
                ADJACENT_CLASS[edge.fact.authority],
                active ? "edge-active" : "",
                selectedIdentity !== null && !active ? "edge-dim" : "",
              ]
                .filter(Boolean)
                .join(" ")}
            >
              <path d={edge.path} markerEnd="url(#arrow)" />
              <text x={edge.labelX} y={edge.labelY} textAnchor="middle">
                {edge.fact.relation.kind}
              </text>
            </g>
          );
        })}

        {layout.nodes.map(({ identity, node, x, y }) => {
          const isSelected = identity === selectedIdentity;
          const isAdjacent = adjacent.has(identity);
          return (
            // biome-ignore lint/a11y/useSemanticElements: SVG <g> cannot be a native <button>; keyboard activation is handled via onKeyDown.
            <g
              key={identity}
              transform={`translate(${x}, ${y})`}
              className={[
                "node",
                AUTHORITY_CLASS[node.authority],
                node.deprecated ? "node-deprecated" : "",
                isSelected ? "node-selected" : "",
                !isSelected && !isAdjacent && selectedIdentity !== null
                  ? "node-dim"
                  : "",
              ]
                .filter(Boolean)
                .join(" ")}
              onClick={() => toggle(node.subject)}
              onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                  toggle(node.subject);
                }
              }}
              role="button"
              tabIndex={0}
              aria-pressed={isSelected}
              aria-label={`select ${identity}`}
              data-testid="graph-node"
              data-identity={identity}
            >
              <rect
                width={NODE_WIDTH}
                height={NODE_HEIGHT}
                rx={8}
                className="node-box"
              />
              <text x={12} y={19} className="node-kind">
                {truncate(`${node.subject.namespace}:${node.subject.kind}`, 24)}
              </text>
              <text x={12} y={36} className="node-id">
                {truncate(node.subject.id, 24)}
              </text>
            </g>
          );
        })}
      </svg>
    </div>
  );
}

function touches(
  fact: GraphState["relations"][number],
  identity: string,
): boolean {
  return (
    subjectRefToString(fact.relation.from) === identity ||
    subjectRefToString(fact.relation.to) === identity
  );
}

function truncate(value: string, max: number): string {
  return value.length > max ? `${value.slice(0, max - 1)}…` : value;
}

export type { SubjectNode };
