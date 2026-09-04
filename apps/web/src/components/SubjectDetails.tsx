import {
  type GraphState,
  isSameSubject,
  type SubjectNode,
  subjectRefToString,
} from "@vistalith/client";
import { useSelection } from "../state/selection.ts";

/**
 * Details lens for the selected SubjectRef, plus its relations. Selecting a
 * relation endpoint re-selects that SubjectRef: selection propagates
 * identities across lenses, never renderer ids.
 */
export function SubjectDetails({ graph }: { graph: GraphState }) {
  const selected = useSelection((s) => s.selected);
  const select = useSelection((s) => s.select);

  if (!selected) {
    return (
      <div className="subject-details" data-testid="subject-details">
        <p className="empty">
          select a subject in the list, the graph or an edge endpoint
        </p>
      </div>
    );
  }

  const node = graph.subjects.find((candidate) =>
    isSameSubject(candidate.subject, selected),
  );
  if (!node) {
    return (
      <div className="subject-details" data-testid="subject-details">
        <p className="empty">
          subject <code>{subjectRefToString(selected)}</code> is not in the
          current graph revision
        </p>
      </div>
    );
  }

  const incoming = graph.relations.filter(
    (f) => subjectRefToString(f.relation.to) === subjectRefToString(selected),
  );
  const outgoing = graph.relations.filter(
    (f) => subjectRefToString(f.relation.from) === subjectRefToString(selected),
  );

  return (
    <div className="subject-details" data-testid="subject-details">
      <header>
        <span className={`badge badge-${node.authority}`}>
          {node.authority}
        </span>
        {node.deprecated ? (
          <span className="badge badge-deprecated">deprecated</span>
        ) : null}
        <h2>
          <code>{subjectRefToString(node.subject)}</code>
        </h2>
        {node.subject.revision ? (
          <p className="revision">
            source revision <code>{node.subject.revision}</code>
          </p>
        ) : null}
      </header>

      <h3>Provenance</h3>
      <dl className="provenance">
        <dt>source</dt>
        <dd>{node.provenance.source}</dd>
        {node.provenance.source_revision ? (
          <>
            <dt>source revision</dt>
            <dd>
              <code>{node.provenance.source_revision}</code>
            </dd>
          </>
        ) : null}
        {node.provenance.note ? (
          <>
            <dt>note</dt>
            <dd>{node.provenance.note}</dd>
          </>
        ) : null}
        {typeof node.provenance.confidence === "number" ? (
          <>
            <dt>confidence</dt>
            <dd>{node.provenance.confidence}</dd>
          </>
        ) : null}
      </dl>

      {Object.keys(node.properties ?? {}).length > 0 ? (
        <>
          <h3>Properties</h3>
          <dl className="properties">
            {Object.entries(node.properties ?? {}).map(([key, value]) => (
              <span key={key} className="property">
                <dt>{key}</dt>
                <dd>
                  {typeof value === "string" ? value : JSON.stringify(value)}
                </dd>
              </span>
            ))}
          </dl>
        </>
      ) : null}

      <h3>Relations</h3>
      <RelationList
        title="outgoing"
        items={outgoing.map((f) => ({
          key: `${f.relation.kind}->${subjectRefToString(f.relation.to)}`,
          label: `${f.relation.kind} → ${subjectRefToString(f.relation.to)}`,
          target: f.relation.to,
          authority: f.authority,
        }))}
        onSelect={select}
      />
      <RelationList
        title="incoming"
        items={incoming.map((f) => ({
          key: `${subjectRefToString(f.relation.from)}->${f.relation.kind}`,
          label: `${subjectRefToString(f.relation.from)} → ${f.relation.kind}`,
          target: f.relation.from,
          authority: f.authority,
        }))}
        onSelect={select}
      />
    </div>
  );
}

interface RelationItem {
  key: string;
  label: string;
  target: SubjectNode["subject"];
  authority: string;
}

function RelationList({
  title,
  items,
  onSelect,
}: {
  title: string;
  items: RelationItem[];
  onSelect: (ref: SubjectNode["subject"]) => void;
}) {
  if (items.length === 0) return null;
  return (
    <div className="relation-group">
      <h4>{title}</h4>
      <ul>
        {items.map((item) => (
          <li key={item.key}>
            <button
              type="button"
              className="relation-item"
              onClick={() => onSelect(item.target)}
              data-identity={subjectRefToString(item.target)}
            >
              <span className={`edge-chip edge-${item.authority}`}>
                {item.authority}
              </span>
              {item.label}
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
