import type {
  AppendedEvent,
  C4View,
  DraftIntentInput,
  ForkReply,
  ForkThreadInput,
  GraphAtRevision,
  GraphDiff,
  GraphPatch,
  GraphState,
  Health,
  IntentDetail,
  IntentSummary,
  PatchOutcome,
  PromotionOutcome,
  StoredEvent,
  SubjectNode,
  SubjectRef,
  ThreadReply,
  ThreadSummary,
  ThreadView,
  VEvent,
} from "./types.js";
import { subjectRefToString } from "./types.js";

/** Error raised for non-2xx responses; carries the parsed server message. */
export class ApiError extends Error {
  readonly status: number;

  constructor(status: number, message: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
  }
}

export interface VistalithClientOptions {
  /** Base URL of `vistalithd`, e.g. `http://127.0.0.1:7420`. No trailing slash. */
  baseUrl: string;
  /** Fetch implementation; defaults to global `fetch` (injectable for tests). */
  fetchImpl?: typeof fetch;
}

/**
 * Typed HTTP client for `vistalithd` (slice-1 API):
 * health, graph, subjects, events (append) and patches (propose).
 */
export class VistalithClient {
  private readonly baseUrl: string;
  private readonly fetchImpl: typeof fetch;

  constructor(options: VistalithClientOptions) {
    this.baseUrl = options.baseUrl.replace(/\/+$/, "");
    this.fetchImpl = options.fetchImpl ?? fetch;
  }

  async health(): Promise<Health> {
    return this.getJson<Health>("/health");
  }

  async graph(): Promise<GraphState> {
    return this.getJson<GraphState>("/graph");
  }

  /** Time travel (SPEC-011): the graph as of an earlier revision. */
  async graphAt(revision: number): Promise<GraphAtRevision> {
    return this.getJson<GraphAtRevision>(
      `/graph?at_revision=${encodeURIComponent(String(revision))}`,
    );
  }

  /**
   * Structural diff between two revisions (SPEC-011). `to` defaults to the
   * current revision on the server.
   */
  async diff(from: number, to?: number): Promise<GraphDiff> {
    const query = to === undefined ? `?from=${from}` : `?from=${from}&to=${to}`;
    return this.getJson<GraphDiff>(`/diff${query}`);
  }

  async subjects(): Promise<SubjectNode[]> {
    const body = await this.getJson<{ subjects: SubjectNode[] }>("/subjects");
    return body.subjects;
  }

  /** Fetches one subject by SubjectRef (identity only; revision is ignored). */
  async subject(ref: SubjectRef): Promise<SubjectNode> {
    const path = `/subjects/${enc(ref.namespace)}/${enc(ref.kind)}/${enc(ref.id)}`;
    return this.getJson<SubjectNode>(path);
  }

  async events(): Promise<StoredEvent[]> {
    const body = await this.getJson<{ events: StoredEvent[] }>("/events");
    return body.events;
  }

  /** Appends one durable event; throws `ApiError` on duplicates (409) or invalid events (422). */
  async appendEvent(event: VEvent): Promise<AppendedEvent> {
    return this.postJson<AppendedEvent>("/events", event, {
      okStatus: 201,
      throwOnError: true,
    });
  }

  /**
   * Proposes a graph patch. Returns the outcome for both statuses: applied
   * (200) and rejected (409) — rejections are durable events, so they are a
   * normal result, not a transport error.
   */
  async proposePatch(patch: GraphPatch): Promise<PatchOutcome> {
    return this.postJson<PatchOutcome>("/patches", patch, {
      okStatus: 200,
      throwOnError: false,
    });
  }

  // --- Conversation (slice 3) ---

  /** Starts a conversation thread; returns its SubjectRef identity string. */
  async createThread(title: string): Promise<string> {
    const body = await this.postJson<{ thread: string }>(
      "/threads",
      { title },
      { okStatus: 201, throwOnError: true },
    );
    return body.thread;
  }

  async threads(): Promise<ThreadSummary[]> {
    const body = await this.getJson<{ threads: ThreadSummary[] }>("/threads");
    return body.threads;
  }

  async thread(id: string): Promise<ThreadView> {
    return this.getJson<ThreadView>(`/threads/${enc(id)}`);
  }

  /** Sends a user message and waits for the completed turn. */
  async sendMessage(id: string, content: string): Promise<ThreadReply> {
    return this.postJson<ThreadReply>(
      `/threads/${enc(id)}/messages`,
      { content },
      { okStatus: 200, throwOnError: true },
    );
  }

  // --- Fork / diff / time travel (slice 5, SPEC-011) ---

  /**
   * Forks a thread at a turn boundary (default: latest turn). The fork is a
   * new durable thread whose items keep `forked_of` bindings to their
   * originals; promotion into SDDK stays an explicit act.
   */
  async forkThread(
    id: string,
    input: ForkThreadInput = {},
  ): Promise<ForkReply> {
    return this.postJson<ForkReply>(`/threads/${enc(id)}/fork`, input, {
      okStatus: 201,
      throwOnError: true,
    });
  }

  // --- C4 projection (slice 3) ---

  async c4View(): Promise<C4View> {
    return this.getJson<C4View>("/views/c4");
  }

  // --- Visual intents (slice 4, SPEC-006) ---

  /** Drafts an intent: never executes; resolution + base revision only. */
  async draftIntent(input: DraftIntentInput): Promise<IntentSummary> {
    const body = await this.postJson<{ intent: string; base_revision: number }>(
      "/intents",
      input,
      { okStatus: 201, throwOnError: true },
    );
    return {
      intent: body.intent,
      target: input.target,
      gesture: input.gesture,
      status: "draft",
      base_revision: body.base_revision,
      stale: false,
    };
  }

  async intents(): Promise<IntentSummary[]> {
    const body = await this.getJson<{ intents: IntentSummary[] }>("/intents");
    return body.intents;
  }

  /** Fetches one intent with its stale-aware preview data. */
  async intent(id: string): Promise<IntentDetail> {
    return this.getJson<IntentDetail>(`/intents/${enc(id)}`);
  }

  /** Explicit promotion. Applied/governed → 200, stale → 409 (normal result). */
  async promoteIntent(id: string, actor?: string): Promise<PromotionOutcome> {
    return this.postJson<PromotionOutcome>(
      `/intents/${enc(id)}/promote`,
      actor ? { actor } : {},
      { okStatus: 200, throwOnError: false },
    );
  }

  async discardIntent(
    id: string,
    reason?: string,
    actor?: string,
  ): Promise<void> {
    await this.postJson<null>(
      `/intents/${enc(id)}/discard`,
      { reason, actor },
      { okStatus: 200, throwOnError: true },
    );
  }

  private async getJson<T>(path: string): Promise<T> {
    const response = await this.fetchImpl(`${this.baseUrl}${path}`);
    return this.parse<T>(response, { okStatus: 200, throwOnError: true });
  }

  private async postJson<T>(
    path: string,
    body: unknown,
    options: { okStatus: number; throwOnError: boolean },
  ): Promise<T> {
    const response = await this.fetchImpl(`${this.baseUrl}${path}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });
    return this.parse<T>(response, options);
  }

  private async parse<T>(
    response: Response,
    options: { okStatus: number; throwOnError: boolean },
  ): Promise<T> {
    const text = await response.text();
    const body: unknown = text.length > 0 ? JSON.parse(text) : null;
    if (response.status === options.okStatus) {
      return body as T;
    }
    const message =
      body !== null &&
      typeof body === "object" &&
      "error" in body &&
      typeof (body as { error: unknown }).error === "string"
        ? (body as { error: string }).error
        : `unexpected status ${response.status}`;
    if (options.throwOnError || response.status >= 500) {
      throw new ApiError(response.status, message);
    }
    return body as T;
  }
}

function enc(segment: string): string {
  return encodeURIComponent(segment);
}

export { subjectRefToString };
