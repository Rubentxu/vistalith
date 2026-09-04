import type { GraphDiff, SubjectChange } from "@vistalith/client";

/**
 * Time travel controls (SPEC-011): pick a past revision and the graph lens
 * renders that strict replay instead of the live projection. "live" returns
 * to the current revision.
 */
export function TimeTravelBar({
  currentRevision,
  asOf,
  onSelect,
}: {
  currentRevision: number;
  asOf: number | null;
  onSelect: (revision: number | null) => void;
}) {
  const revisions: number[] = [];
  for (let revision = currentRevision - 1; revision >= 1; revision -= 1) {
    revisions.push(revision);
  }
  return (
    <div className="time-travel-bar" data-testid="time-travel-bar">
      <label htmlFor="revision-select">time travel:</label>
      <select
        id="revision-select"
        value={asOf === null ? "" : String(asOf)}
        onChange={(event) => {
          const value = event.target.value;
          onSelect(value === "" ? null : Number(value));
        }}
      >
        <option value="">live (revision {currentRevision})</option>
        {revisions.map((revision) => (
          <option key={revision} value={revision}>
            revision {revision}
          </option>
        ))}
      </select>
      {asOf !== null ? (
        <span className="badge badge-advisory">
          read-only · revision {asOf}
        </span>
      ) : null}
    </div>
  );
}

function identityOf(subject: {
  namespace: string;
  kind: string;
  id: string;
}): string {
  return `${subject.namespace}:${subject.kind}:${subject.id}`;
}

/** Structural diff between the selected revision and the live graph. */
export function HistoryDiff({ diff, from }: { diff: GraphDiff; from: number }) {
  const empty =
    diff.added_subjects.length === 0 &&
    diff.removed_subjects.length === 0 &&
    diff.changed_subjects.length === 0 &&
    diff.added_relations.length === 0 &&
    diff.removed_relations.length === 0 &&
    diff.changed_relations.length === 0;
  return (
    <section className="history-diff" data-testid="history-diff">
      <h3>diff revision {from} → live</h3>
      {empty ? (
        <p className="empty">no structural differences</p>
      ) : (
        <ul className="history-diff-list">
          {diff.added_subjects.length > 0 ? (
            <li>
              + {diff.added_subjects.length} subject
              {diff.added_subjects.length === 1 ? "" : "s"}:{" "}
              <span className="diff-identities">
                {diff.added_subjects.map(identityOf).join(", ")}
              </span>
            </li>
          ) : null}
          {diff.removed_subjects.length > 0 ? (
            <li>
              − {diff.removed_subjects.length} subject
              {diff.removed_subjects.length === 1 ? "" : "s"}:{" "}
              <span className="diff-identities">
                {diff.removed_subjects.map(identityOf).join(", ")}
              </span>
            </li>
          ) : null}
          {diff.changed_subjects.length > 0 ? (
            <li>
              ~ {diff.changed_subjects.length} changed subject
              {diff.changed_subjects.length === 1 ? "" : "s"}:{" "}
              <span className="diff-identities">
                {diff.changed_subjects
                  .map((change: SubjectChange) => identityOf(change.subject))
                  .join(", ")}
              </span>
            </li>
          ) : null}
          {diff.added_relations.length > 0 ? (
            <li>+ {diff.added_relations.length} relation(s)</li>
          ) : null}
          {diff.removed_relations.length > 0 ? (
            <li>− {diff.removed_relations.length} relation(s)</li>
          ) : null}
          {diff.changed_relations.length > 0 ? (
            <li>~ {diff.changed_relations.length} changed relation(s)</li>
          ) : null}
        </ul>
      )}
    </section>
  );
}
