/**
 * TypeScript mirror of the Vistalith protocol types.
 *
 * Shapes match exactly what `vistalithd` (vistalith-domain + vistalith-graph)
 * serializes. The SubjectRef mirror follows ADR-011: identity is
 * `namespace:kind:id`; `revision` is auxiliary metadata and never part of
 * identity. Renderer IDs are never semantic IDs.
 */

export type Namespace =
  | "sddk"
  | "arch"
  | "code"
  | "verification"
  | "work"
  | "agentic"
  | "visual"
  | "vistalith"
  | (string & {});

export interface SubjectRef {
  namespace: Namespace;
  kind: string;
  id: string;
  revision?: string;
}

export type AuthorityClass =
  | "authoritative"
  | "derived"
  | "advisory"
  | "ephemeral";

export interface Provenance {
  source: string;
  source_revision?: string;
  note?: string;
  confidence?: number;
}

export interface RelationRef {
  from: SubjectRef;
  kind: string;
  to: SubjectRef;
}

export interface RelationFact {
  relation: RelationRef;
  authority: AuthorityClass;
  provenance: Provenance;
}

export interface SubjectNode {
  subject: SubjectRef;
  authority: AuthorityClass;
  provenance: Provenance;
  properties?: Record<string, unknown>;
  deprecated: boolean;
  last_event_sequence: number;
}

/** GET /graph — canonical, revision-ordered graph state. */
export interface GraphState {
  revision: number;
  subjects: SubjectNode[];
  relations: RelationFact[];
}

export interface Health {
  status: string;
  service: string;
  graph_revision: number;
  events: number;
}

// --- Events (SPEC-002) ------------------------------------------------------

export type EventType =
  | "subject-defined"
  | "subject-updated"
  | "subject-deprecated"
  | "relation-declared"
  | "patch-applied"
  | "patch-rejected";

interface EventBase {
  event_id: string;
  actor: string;
  /** RFC3339 timestamp. */
  timestamp: string;
  subjects: SubjectRef[];
  correlation_id: string;
  causation_id?: string;
  trace_id?: string;
}

export interface SubjectDefinedPayload {
  subject: SubjectRef;
  authority: AuthorityClass;
  provenance: Provenance;
  properties?: Record<string, unknown>;
}

export interface SubjectUpdatedPayload {
  subject: SubjectRef;
  properties: Record<string, unknown>;
}

export interface SubjectDeprecatedPayload {
  subject: SubjectRef;
  reason?: string;
}

export interface RelationDeclaredPayload {
  fact: RelationFact;
}

export interface PatchAppliedPayload {
  patch_id: string;
  operations: PatchOperation[];
}

export interface PatchRejectedPayload {
  patch_id: string;
  reason: string;
}

export type EventPayload =
  | SubjectDefinedPayload
  | SubjectUpdatedPayload
  | SubjectDeprecatedPayload
  | RelationDeclaredPayload
  | PatchAppliedPayload
  | PatchRejectedPayload;

export type VEvent = EventBase &
  (
    | { type: "subject-defined"; payload: SubjectDefinedPayload }
    | { type: "subject-updated"; payload: SubjectUpdatedPayload }
    | { type: "subject-deprecated"; payload: SubjectDeprecatedPayload }
    | { type: "relation-declared"; payload: RelationDeclaredPayload }
    | { type: "patch-applied"; payload: PatchAppliedPayload }
    | { type: "patch-rejected"; payload: PatchRejectedPayload }
  );

/** A durable log entry: the event plus log-assigned coordinates. */
export type StoredEvent = { sequence: number; revision: number } & VEvent;

export interface AppendedEvent {
  event_id: string;
  sequence: number;
  revision: number;
}

// --- Graph patches (SPEC-004) ----------------------------------------------

export type PatchOperation =
  | {
      op: "upsert-subject";
      subject: SubjectRef;
      authority: AuthorityClass;
      provenance: Provenance;
      properties?: Record<string, unknown>;
    }
  | { op: "declare-relation"; fact: RelationFact }
  | { op: "deprecate-subject"; subject: SubjectRef; reason?: string };

export interface GraphPatch {
  patch_id: string;
  /** Optimistic concurrency token: the graph revision the patch targets. */
  base_revision: number;
  proposed_by: string;
  operations: PatchOperation[];
}

export type PatchOutcome =
  | { status: "applied"; patch_id: string; revision: number }
  | { status: "rejected"; patch_id: string; reason: string };

// --- Identity helpers -------------------------------------------------------

/** Renders the stable identity string `namespace:kind:id` (never revision). */
export function subjectRefToString(ref: SubjectRef): string {
  return `${ref.namespace}:${ref.kind}:${ref.id}`;
}

/** Parses `namespace:kind:id` with an optional trailing `@revision`. */
export function parseSubjectRef(raw: string): SubjectRef {
  const at = raw.lastIndexOf("@");
  const [identity, revision] =
    at > 0 ? [raw.slice(0, at), raw.slice(at + 1)] : [raw, undefined];
  const parts = identity.split(":");
  if (parts.length !== 3 || parts.some((p) => p.length === 0)) {
    throw new Error(
      `invalid subject reference: '${raw}' (expected namespace:kind:id[@revision])`,
    );
  }
  const ref: SubjectRef = {
    namespace: parts[0] as Namespace,
    kind: parts[1] as string,
    id: parts[2] as string,
  };
  if (revision) ref.revision = revision;
  return ref;
}

/** Semantic identity equality: revision never participates (ADR-011). */
export function isSameSubject(a: SubjectRef, b: SubjectRef): boolean {
  return a.namespace === b.namespace && a.kind === b.kind && a.id === b.id;
}
