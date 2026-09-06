import type {
  C4Diff,
  LikeC4ImportReport,
  VistalithClient,
} from "@vistalith/client";
import { useState } from "react";

/**
 * LikeC4 round-trip (SPK-008): export the C4 projection as LikeC4 DSL,
 * edit/import it back. Identity travels in `metadata { vistalith }`, so an
 * untouched round-trip reports unchanged subjects and no revision bump.
 * Also renders the architecture revision diff between two revisions.
 */
export function LikeC4Panel({
  client,
  onGraphChanged,
}: {
  client: VistalithClient;
  onGraphChanged: () => void;
}) {
  const [dsl, setDsl] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<LikeC4ImportReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [diff, setDiff] = useState<C4Diff | null>(null);
  const [from, setFrom] = useState("0");

  const exportModel = async () => {
    setBusy(true);
    setError(null);
    try {
      setDsl(await client.likec4Model());
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const importModel = async () => {
    if (dsl === null) return;
    setBusy(true);
    setError(null);
    setReport(null);
    try {
      const result = await client.importLikec4(dsl);
      setReport(result);
      onGraphChanged();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  const runDiff = async () => {
    setBusy(true);
    setError(null);
    setDiff(null);
    try {
      setDiff(await client.c4Diff(Number(from)));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="likec4-panel" data-testid="likec4-panel">
      <h3>LikeC4 round-trip</h3>
      <div className="likec4-actions">
        <button
          type="button"
          onClick={() => void exportModel()}
          disabled={busy}
        >
          export DSL
        </button>
        <button
          type="button"
          onClick={() => void importModel()}
          disabled={busy || dsl === null}
        >
          import DSL
        </button>
      </div>
      {dsl !== null ? (
        <textarea
          aria-label="LikeC4 DSL"
          className="likec4-source"
          spellCheck={false}
          rows={12}
          value={dsl}
          onChange={(event) => setDsl(event.target.value)}
        />
      ) : (
        <p className="empty">
          export the C4 projection, edit it, import it back
        </p>
      )}
      {report ? (
        <p className="likec4-report" data-testid="likec4-report">
          defined {report.defined_subjects.length} · updated{" "}
          {report.updated_subjects.length} · deprecated{" "}
          {report.deprecated_subjects.length} · unchanged{" "}
          {report.unchanged_subjects.length} · relations declared{" "}
          {report.declared_relations.length} · skipped{" "}
          {report.skipped_relations.length}
        </p>
      ) : null}

      <h3>Architecture diff</h3>
      <div className="likec4-actions">
        <label>
          from revision{" "}
          <input
            type="number"
            min={0}
            value={from}
            onChange={(event) => setFrom(event.target.value)}
          />
        </label>
        <button type="button" onClick={() => void runDiff()} disabled={busy}>
          diff → now
        </button>
      </div>
      {diff ? <C4DiffView diff={diff} /> : null}
      {error ? <p className="likec4-error">{error}</p> : null}
    </section>
  );
}

function C4DiffView({ diff }: { diff: C4Diff }) {
  const empty =
    diff.added_elements.length === 0 &&
    diff.removed_elements.length === 0 &&
    diff.changed_elements.length === 0 &&
    diff.added_relationships.length === 0 &&
    diff.removed_relationships.length === 0 &&
    diff.changed_relationships.length === 0;
  return (
    <div className="likec4-diff" data-testid="likec4-diff">
      <p className="c4-revision">
        revisions {diff.from_revision} → {diff.to_revision}
      </p>
      {empty ? <p className="empty">no architecture changes</p> : null}
      <ul>
        {diff.added_elements.map((element) => (
          <li key={`+${element.identity}`} className="diff-added">
            + {element.name} <code>{element.identity}</code>
          </li>
        ))}
        {diff.removed_elements.map((element) => (
          <li key={`-${element.identity}`} className="diff-removed">
            − {element.name} <code>{element.identity}</code>
          </li>
        ))}
        {diff.changed_elements.map((change) => (
          <li key={`~${change.identity}`} className="diff-changed">
            ~ <code>{change.identity}</code>{" "}
            {change.changes
              .map(
                (c) =>
                  `${c.key}: ${JSON.stringify(c.from ?? null)} → ${JSON.stringify(c.to ?? null)}`,
              )
              .join(", ")}
          </li>
        ))}
        {diff.added_relationships.map((relationship) => (
          <li
            key={`+${relationship.source}-${relationship.kind}-${relationship.target}`}
            className="diff-added"
          >
            + <code>{relationship.source}</code> -{relationship.kind}-&gt;{" "}
            <code>{relationship.target}</code>
          </li>
        ))}
        {diff.removed_relationships.map((relationship) => (
          <li
            key={`-${relationship.source}-${relationship.kind}-${relationship.target}`}
            className="diff-removed"
          >
            − <code>{relationship.source}</code> -{relationship.kind}-&gt;{" "}
            <code>{relationship.target}</code>
          </li>
        ))}
        {diff.changed_relationships.map((change) => (
          <li
            key={`~${change.source}-${change.kind}-${change.target}`}
            className="diff-changed"
          >
            ~ <code>{change.source}</code> -{change.kind}-&gt;{" "}
            <code>{change.target}</code>{" "}
            {change.changes.map((c) => `${c.key} → ${String(c.to)}`).join(", ")}
          </li>
        ))}
      </ul>
    </div>
  );
}
