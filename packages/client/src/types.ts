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
  /** Configured conversation provider, e.g. `fake/echo-1`. */
  provider?: string;
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

// --- Conversation (SPEC-007) -----------------------------------------------

export type MessageRole = "user" | "assistant" | "system" | "tool";

export interface ModelUsage {
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
}

export interface ThreadSummary {
  thread: string;
  title: string;
  turns: number;
  last_model?: string;
  /** Source thread identity when this thread is a fork (SPEC-011). */
  forked_from?: string | null;
}

export interface ThreadMessage {
  message: string;
  role: MessageRole;
  content: string;
  turn: number;
  /** Original message identity when this item was copied by a fork. */
  forked_of?: string | null;
}

export interface ThreadView {
  thread: ThreadSummary;
  messages: ThreadMessage[];
}

export interface ThreadReply {
  thread: string;
  message: string;
  turn: number;
  content: string;
  usage: ModelUsage;
}

// --- Fork / diff / time travel (SPEC-011) ------------------------------------

export interface ForkThreadInput {
  /** Last source turn carried into the fork; defaults to the latest turn. */
  up_to_turn?: number;
  note?: string;
}

export interface ForkReply {
  fork: string;
  source: string;
  up_to_turn: number;
  copied_events: number;
}

/** SubjectRefs serialize in the flat wire format. */
export type SubjectRefWire = {
  namespace: Namespace;
  kind: string;
  id: string;
  revision?: number;
};

export interface PropertyChange {
  key: string;
  from?: unknown;
  to?: unknown;
}

export interface SubjectChange {
  subject: SubjectRefWire;
  changes: PropertyChange[];
}

export interface GraphDiff {
  added_subjects: SubjectRefWire[];
  removed_subjects: SubjectRefWire[];
  changed_subjects: SubjectChange[];
  added_relations: {
    from: SubjectRefWire;
    kind: string;
    to: SubjectRefWire;
  }[];
  removed_relations: {
    from: SubjectRefWire;
    kind: string;
    to: SubjectRefWire;
  }[];
  changed_relations: {
    relation: { from: SubjectRefWire; kind: string; to: SubjectRefWire };
    from: unknown;
    to: unknown;
  }[];
}

/** GraphState plus the `as_of_revision` marker set by time-travel reads. */
export interface GraphAtRevision extends GraphState {
  as_of_revision?: number;
}

// --- Visual intents (SPEC-006) ----------------------------------------------

export interface IntentSummary {
  intent: string;
  target: string | null;
  gesture: string;
  status:
    | "draft"
    | "applied"
    | "sddk-governed"
    | "stale"
    | "rejected"
    | "discarded"
    | string;
  base_revision: number;
  stale: boolean;
}

export interface IntentDetail {
  summary: IntentSummary;
  change: unknown;
  current_revision: number;
}

export interface DraftIntentInput {
  /** Target SubjectRef identity string (`namespace:kind:id`). */
  target: string;
  gesture: string;
  /** Patch operations payload: `{ operations: [...] }`. */
  change: unknown;
  reason?: string;
  actor?: string;
}

// --- C4 projection ----------------------------------------------------------

export interface C4Element {
  identity: string;
  name: string;
  description?: string;
  authority: AuthorityClass;
  deprecated: boolean;
}

export interface C4Relationship {
  source: string;
  target: string;
  kind: string;
  authority: AuthorityClass;
}

export interface C4View {
  revision: number;
  systems: C4Element[];
  containers: C4Element[];
  components: C4Element[];
  relationships: C4Relationship[];
}

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

// --- Unified tool catalog + MCP (SPEC-009) ------------------------------------

export type ToolConsequence = "readonly" | "write" | "destructive";
export type PermissionDecision = "allow" | "ask" | "deny";

export type ToolSourceWire =
  | { kind: "native" }
  | { kind: "mcp"; server: string };

export interface ToolInfo {
  id: string;
  description: string;
  consequence: ToolConsequence;
  source: ToolSourceWire;
  parameters: unknown;
  permission: PermissionDecision;
  grant_remaining: number;
}

export interface GrantInfo {
  tool: string;
  remaining: number;
}

export interface ToolsCatalog {
  tools: ToolInfo[];
  grants: GrantInfo[];
}

export type McpServerConfig = {
  /** Local, unique server name; tool ids are namespaced under it. */
  name: string;
  /** stdio transport: executable to spawn. */
  command?: string;
  args?: string[];
  /** Streamable HTTP transport: base URL of the MCP endpoint. */
  url?: string;
};

export interface McpServerInfo {
  name: string;
  transport: "stdio" | "http";
  status: "connected" | "unhealthy";
  tools: number;
}

// --- Graph algorithms + semantic context view (SPEC-005, slice 7) --------------

export interface ImpactReport {
  root: string;
  impacted: string[];
}

export interface ContextViewRequest {
  /** Root identities as `ns:kind:id` strings. */
  roots: string[];
  /** Relation kinds the slice may traverse; omitted = every kind. */
  relations?: string[];
  max_depth?: number;
  include_derived?: boolean;
  include_advisory?: boolean;
  /** Approximate token budget (≈ chars / 4). Default 8000. */
  token_budget?: number;
}

export interface ContextItem {
  subject: string;
  authority: string;
  depth: number;
  reason:
    | { reason: "root" }
    | { reason: "via"; from: string; kind: string; depth: number };
  properties: Record<string, unknown>;
  last_event_sequence: number;
  last_touch: string;
  last_actor: string;
  estimated_tokens: number;
}

export interface ContextExclusion {
  subject: string;
  exclusion:
    | { reason: "unknown-subject" }
    | { reason: "last-touched-before-cutoff"; last_touch: string }
    | { reason: "authority-class"; class: string }
    | { reason: "deeper-than-max-depth"; depth: number }
    | { reason: "token-budget-exhausted" };
}

export interface SemanticContextView {
  roots: string[];
  items: ContextItem[];
  exclusions: ContextExclusion[];
  estimated_tokens: number;
  token_budget: number;
  truncated: boolean;
}

// --- Agents & frames (slice 8) --------------------------------------------------

export interface AgentInfo {
  agent: string;
  role: string;
  instructions: string;
  tools: string[];
  budget_turns?: number | null;
}

export interface FrameSummary {
  frame: string;
  goal: string;
  status:
    | "open"
    | "completed"
    | "aborted"
    | "turns-exhausted"
    | "budget-exhausted";
  turns: number;
  max_turns: number;
  used_tokens: number;
  token_budget: number;
  permitted_tools: string[];
  outcome?: string | null;
  summary?: string | null;
}

export interface FrameMessage {
  message: string;
  role: string;
  content: string;
  turn: number;
}

export interface FrameView {
  frame: FrameSummary;
  messages: FrameMessage[];
}

export interface CreateFrameInput {
  goal: string;
  agent?: string;
  /** Root identities as `ns:kind:id` strings. */
  subjects?: string[];
  permitted_tools?: string[];
  max_turns?: number;
  token_budget?: number;
}

export interface FrameTurnReply {
  frame: string;
  turn: number;
  content: { turns_used: number; used_tokens: number };
  auto_closed: string | null;
}

// --- Governed SDDK promotion (slice 9, SPK-012) ------------------------------

export type PromotionOutcome =
  | { outcome: "applied"; revision: number }
  | { outcome: "sddk-governed"; subject: string; note?: string }
  | {
      outcome: "submitted-to-sddk";
      subject: string;
      proposal: string;
      decision: "allowed" | "denied" | "approval-required";
      receipt_id: string | null;
    }
  | { outcome: "stale"; current_revision: number; base_revision: number }
  | { outcome: "rejected"; reason: string };

export interface SddkReceipt {
  receipt_id: string;
  project_id: string;
  cycle_id: string | null;
  capability: string;
  request_hash: string;
  status: string;
  result: unknown;
  started_at: string;
  completed_at: string | null;
}

// --- SDDK workflow sync + why path (slice 10, M6/M9) ---------------------------

export interface SyncReport {
  subjects_created: number;
  subjects_updated: number;
  relations_declared: number;
  events_skipped: number;
}

export interface WhyLink {
  depth: number;
  kind: string;
  from: string;
  to: string;
}

export interface WhyPath {
  subject: string;
  links: WhyLink[];
  evidence: WhyLink[];
  max_depth_reached: number;
}

// --- Streaming turns (slice 11) --------------------------------------------------

export interface StreamTurnHandlers {
  /** Called per text delta as the model streams. */
  onDelta?: (text: string) => void;
  /** Called once with the durable turn coordinates. */
  onDone?: (reply: {
    turn: number;
    message: string;
    content: string;
    usage: {
      input_tokens: number;
      output_tokens: number;
      total_tokens: number;
    };
  }) => void;
  onError?: (message: string) => void;
}
