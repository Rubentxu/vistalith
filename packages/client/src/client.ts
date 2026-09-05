import type {
  AgentInfo,
  AppendedEvent,
  C4View,
  CreateFrameInput,
  DraftIntentInput,
  ForkReply,
  ForkThreadInput,
  FrameSummary,
  FrameTurnReply,
  FrameView,
  GraphAtRevision,
  GraphDiff,
  GraphPatch,
  GraphState,
  Health,
  ImpactReport,
  IntentDetail,
  IntentSummary,
  McpServerConfig,
  McpServerInfo,
  PatchOutcome,
  PromotionOutcome,
  SemanticContextView,
  StoredEvent,
  SubjectNode,
  SubjectRef,
  SyncReport,
  ThreadReply,
  ThreadSummary,
  ThreadView,
  ToolsCatalog,
  VEvent,
  WhyPath,
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

  // --- Unified tool catalog + MCP (slice 6, SPEC-009) ---

  /** The unified catalog: native + MCP tools with permission decisions. */
  async tools(): Promise<ToolsCatalog> {
    return this.getJson<ToolsCatalog>("/tools");
  }

  /**
   * Grants `calls` more authorized invocations for a tool (scoped temporary
   * grant, `agentic/TOOLS-PERMISSIONS.md`).
   */
  async grantTool(
    id: string,
    calls = 1,
  ): Promise<{ tool: string; remaining: number }> {
    return this.postJson<{ tool: string; remaining: number }>(
      `/tools/${enc(id)}/grant`,
      { calls },
      { okStatus: 200, throwOnError: true },
    );
  }

  async revokeTool(id: string): Promise<{ tool: string; revoked: boolean }> {
    return this.postJson<{ tool: string; revoked: boolean }>(
      `/tools/${enc(id)}/revoke`,
      {},
      { okStatus: 200, throwOnError: true },
    );
  }

  async mcpServers(): Promise<McpServerInfo[]> {
    const body = await this.getJson<{ servers: McpServerInfo[] }>(
      "/mcp/servers",
    );
    return body.servers;
  }

  /** Connects an MCP server (stdio or Streamable HTTP) and discovers tools. */
  async addMcpServer(config: McpServerConfig): Promise<McpServerInfo> {
    return this.postJson<McpServerInfo>("/mcp/servers", config, {
      okStatus: 201,
      throwOnError: true,
    });
  }

  async removeMcpServer(name: string): Promise<{ removed: string }> {
    return this.deleteJson<{ removed: string }>(`/mcp/servers/${enc(name)}`);
  }

  // --- Algorithms + semantic context view (slice 7) ---

  /** Transitive dependents of a subject (SPK-004: impact). */
  async impact(
    namespace: string,
    kind: string,
    id: string,
    kinds?: string[],
  ): Promise<ImpactReport> {
    const query = kinds ? `?kinds=${enc(kinds.join(","))}` : "";
    return this.getJson<ImpactReport>(
      `/algorithms/impact/${enc(namespace)}/${enc(kind)}/${enc(id)}${query}`,
    );
  }

  /** Bounded, explainable graph slice (SPEC-005). */
  async contextView(
    request: import("./types.js").ContextViewRequest,
  ): Promise<SemanticContextView> {
    return this.postJson<SemanticContextView>("/views/context", request, {
      okStatus: 200,
      throwOnError: true,
    });
  }

  // --- Agents & frames (slice 8) ---

  async createAgent(input: {
    role: string;
    instructions?: string;
    model?: string;
    tools?: string[];
    budget_turns?: number;
    expected_outputs?: string[];
  }): Promise<{ agent: string }> {
    return this.postJson<{ agent: string }>("/agents", input, {
      okStatus: 201,
      throwOnError: true,
    });
  }

  async agents(): Promise<AgentInfo[]> {
    const body = await this.getJson<{ agents: AgentInfo[] }>("/agents");
    return body.agents;
  }

  async createFrame(
    input: CreateFrameInput,
  ): Promise<{ frame: string; thread: string }> {
    return this.postJson<{ frame: string; thread: string }>("/frames", input, {
      okStatus: 201,
      throwOnError: true,
    });
  }

  async frames(): Promise<FrameSummary[]> {
    const body = await this.getJson<{ frames: FrameSummary[] }>("/frames");
    return body.frames;
  }

  async frame(id: string): Promise<FrameView> {
    return this.getJson<FrameView>(`/frames/${enc(id)}`);
  }

  /** Runs one turn inside the frame's bounds. */
  async frameTurn(id: string, content: string): Promise<FrameTurnReply> {
    return this.postJson<FrameTurnReply>(
      `/frames/${enc(id)}/turns`,
      { content },
      { okStatus: 200, throwOnError: false },
    );
  }

  async closeFrame(
    id: string,
    outcome: "completed" | "aborted",
    summary?: string,
  ): Promise<FrameSummary> {
    return this.postJson<FrameSummary>(
      `/frames/${enc(id)}/close`,
      { outcome, summary },
      { okStatus: 200, throwOnError: true },
    );
  }

  /**
   * Projects the SDDK ledger into the SWG (M6). Requires the SDDK bridge.
   */
  async syncSddkWorkflow(): Promise<SyncReport> {
    return this.postJson<SyncReport>(
      "/sddk/sync",
      {},
      {
        okStatus: 200,
        throwOnError: true,
      },
    );
  }

  /** Why-path: what supports this subject (M9). */
  async why(
    namespace: string,
    kind: string,
    id: string,
    depth = 3,
  ): Promise<WhyPath> {
    return this.getJson<WhyPath>(
      `/why/${enc(namespace)}/${enc(kind)}/${enc(id)}?depth=${depth}`,
    );
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

  /**
   * Explicit promotion. Applied/submitted/governed → 200, stale → 409
   * (normal result). With the SDDK bridge configured server-side,
   * `approve` supplies the human approval for high-risk capabilities.
   */
  async promoteIntent(
    id: string,
    options: { actor?: string; approve?: boolean } = {},
  ): Promise<PromotionOutcome> {
    return this.postJson<PromotionOutcome>(
      `/intents/${enc(id)}/promote`,
      options,
      { okStatus: 200, throwOnError: false },
    );
  }

  /** Receipts recorded in the SDDK ledger (requires the bridge). */
  async sddkReceipts(): Promise<import("./types.js").SddkReceipt[]> {
    const body = await this.getJson<{
      receipts: import("./types.js").SddkReceipt[];
    }>("/sddk/receipts");
    return body.receipts;
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

  private async deleteJson<T>(path: string): Promise<T> {
    const response = await this.fetchImpl(`${this.baseUrl}${path}`, {
      method: "DELETE",
    });
    return this.parse<T>(response, { okStatus: 200, throwOnError: true });
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
