import { beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError, VistalithClient } from "../src/client.ts";
import {
  isSameSubject,
  parseSubjectRef,
  subjectRefToString,
  type VEvent,
} from "../src/types.ts";

function jsonResponse(status: number, body: unknown): Response {
  return {
    status,
    text: async () => JSON.stringify(body),
  } as unknown as Response;
}

const SAMPLE_EVENT: VEvent = {
  event_id: "0198f6c0-0000-7000-8000-000000000001",
  actor: "test:client",
  timestamp: "2026-09-04T09:00:00Z",
  subjects: [{ namespace: "arch", kind: "container", id: "payment-service" }],
  correlation_id: "0198f6c0-0000-7000-8000-0000000000f1",
  type: "subject-defined",
  payload: {
    subject: { namespace: "arch", kind: "container", id: "payment-service" },
    authority: "authoritative",
    provenance: { source: "test:client" },
  },
};

describe("identity helpers (ADR-011)", () => {
  it("roundtrips subject refs through the identity string", () => {
    const raw = "sddk:work-item:TEST-MODEL-001";
    expect(subjectRefToString(parseSubjectRef(raw))).toBe(raw);
  });

  it("keeps revision out of identity but in the metadata", () => {
    const ref = parseSubjectRef("code:symbol:foo@abc123");
    expect(subjectRefToString(ref)).toBe("code:symbol:foo");
    expect(ref.revision).toBe("abc123");
  });

  it("rejects malformed refs", () => {
    expect(() => parseSubjectRef("sddk:work-item")).toThrow();
    expect(() => parseSubjectRef("")).toThrow();
  });

  it("compares identity, never revision", () => {
    const a = { namespace: "arch", kind: "system", id: "vistalith" };
    const b = { ...a, revision: "r7" };
    expect(isSameSubject(a, b)).toBe(true);
  });
});

describe("VistalithClient", () => {
  let fetchImpl: ReturnType<typeof vi.fn>;
  let client: VistalithClient;

  beforeEach(() => {
    fetchImpl = vi.fn();
    client = new VistalithClient({
      baseUrl: "http://127.0.0.1:7420/",
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
  });

  it("fetches graph state", async () => {
    fetchImpl.mockResolvedValue(
      jsonResponse(200, {
        revision: 5,
        subjects: [
          {
            subject: { namespace: "arch", kind: "container", id: "p" },
            authority: "authoritative",
            provenance: { source: "user" },
            deprecated: false,
            last_event_sequence: 1,
          },
        ],
        relations: [],
      }),
    );

    const graph = await client.graph();
    expect(fetchImpl).toHaveBeenCalledWith("http://127.0.0.1:7420/graph");
    expect(graph.revision).toBe(5);
    expect(graph.subjects).toHaveLength(1);
  });

  it("addresses subjects by identity path", async () => {
    fetchImpl.mockResolvedValue(jsonResponse(200, {}));
    await client.subject({
      namespace: "arch",
      kind: "system",
      id: "vistalith",
    });
    expect(fetchImpl).toHaveBeenCalledWith(
      "http://127.0.0.1:7420/subjects/arch/system/vistalith",
    );
  });

  it("encodes subject path segments", async () => {
    fetchImpl.mockResolvedValue(jsonResponse(200, {}));
    await client.subject({
      namespace: "code",
      kind: "symbol",
      id: "weird id",
    });
    expect(fetchImpl).toHaveBeenCalledWith(
      "http://127.0.0.1:7420/subjects/code/symbol/weird%20id",
    );
  });

  it("posts events and expects 201", async () => {
    fetchImpl.mockResolvedValue(
      jsonResponse(201, {
        event_id: SAMPLE_EVENT.event_id,
        sequence: 0,
        revision: 1,
      }),
    );
    const appended = await client.appendEvent(SAMPLE_EVENT);
    expect(appended.sequence).toBe(0);
    const [, init] = fetchImpl.mock.calls[0] as [string, RequestInit];
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body as string).type).toBe("subject-defined");
  });

  it("throws ApiError on duplicate event (409)", async () => {
    fetchImpl.mockResolvedValue(
      jsonResponse(409, { error: "duplicate event id ..." }),
    );
    await expect(client.appendEvent(SAMPLE_EVENT)).rejects.toThrow(ApiError);
  });

  it("returns patch outcomes for applied (200) and rejected (409)", async () => {
    fetchImpl.mockResolvedValueOnce(
      jsonResponse(200, {
        status: "applied",
        patch_id: "p1",
        revision: 6,
      }),
    );
    fetchImpl.mockResolvedValueOnce(
      jsonResponse(409, {
        status: "rejected",
        patch_id: "p2",
        reason: "stale base revision: patch proposed at 5, graph is at 6",
      }),
    );

    const applied = await client.proposePatch({
      patch_id: "p1",
      base_revision: 5,
      proposed_by: "test",
      operations: [],
    });
    expect(applied.status).toBe("applied");

    const rejected = await client.proposePatch({
      patch_id: "p2",
      base_revision: 5,
      proposed_by: "test",
      operations: [],
    });
    expect(rejected.status).toBe("rejected");
    expect(rejected.reason).toContain("stale");
  });
});

describe("conversation + C4 endpoints (slice 3)", () => {
  let fetchImpl: ReturnType<typeof vi.fn>;
  let client: VistalithClient;

  beforeEach(() => {
    fetchImpl = vi.fn();
    client = new VistalithClient({
      baseUrl: "http://127.0.0.1:7420",
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
  });

  it("creates threads and returns the SubjectRef identity", async () => {
    fetchImpl.mockResolvedValue(
      jsonResponse(201, { thread: "agentic:thread:abc-123" }),
    );
    const thread = await client.createThread("Slice-3 chat");
    expect(thread).toBe("agentic:thread:abc-123");
    const [, init] = fetchImpl.mock.calls[0] as [string, RequestInit];
    expect(init.method).toBe("POST");
    expect(JSON.parse(init.body as string)).toEqual({ title: "Slice-3 chat" });
  });

  it("sends a message and returns the completed turn", async () => {
    fetchImpl.mockResolvedValue(
      jsonResponse(200, {
        thread: "agentic:thread:abc-123",
        message: "agentic:message:def-456",
        turn: 1,
        content: "here is the answer",
        usage: { input_tokens: 4, output_tokens: 8, total_tokens: 12 },
      }),
    );
    const reply = await client.sendMessage("abc-123", "hello");
    expect(reply.turn).toBe(1);
    expect(reply.usage.total_tokens).toBe(12);
    const [url, init] = fetchImpl.mock.calls[0] as [string, RequestInit];
    expect(url).toBe("http://127.0.0.1:7420/threads/abc-123/messages");
    expect(init).toBeDefined();
    expect(JSON.parse(init?.body as string)).toEqual({ content: "hello" });
  });

  it("fetches a thread view with typed items", async () => {
    fetchImpl.mockResolvedValue(
      jsonResponse(200, {
        thread: { thread: "agentic:thread:abc-123", title: "t", turns: 1 },
        messages: [
          {
            message: "agentic:message:m1",
            role: "user",
            content: "hi",
            turn: 1,
          },
        ],
      }),
    );
    const view = await client.thread("abc-123");
    expect(view.messages[0].role).toBe("user");
  });

  it("fetches the C4 projection", async () => {
    fetchImpl.mockResolvedValue(
      jsonResponse(200, {
        revision: 5,
        systems: [],
        containers: [
          {
            identity: "arch:container:payment-service",
            name: "Payment Service",
            authority: "authoritative",
            deprecated: false,
          },
        ],
        components: [],
        relationships: [],
      }),
    );
    const view = await client.c4View();
    expect(view.revision).toBe(5);
    expect(view.containers[0].identity).toBe("arch:container:payment-service");
  });
});

describe("visual intents (slice 4, SPEC-006)", () => {
  let fetchImpl: ReturnType<typeof vi.fn>;
  let client: VistalithClient;

  beforeEach(() => {
    fetchImpl = vi.fn();
    client = new VistalithClient({
      baseUrl: "http://127.0.0.1:7420",
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });
  });

  it("drafts intents without executing anything", async () => {
    fetchImpl.mockResolvedValue(
      jsonResponse(201, {
        intent: "agentic:visual-proposal:i1",
        base_revision: 6,
      }),
    );
    const draft = await client.draftIntent({
      target: "arch:container:payment-service",
      gesture: "rename",
      change: { operations: [] },
    });
    expect(draft.status).toBe("draft");
    expect(draft.base_revision).toBe(6);
    const [url] = fetchImpl.mock.calls[0] as [string];
    expect(url).toBe("http://127.0.0.1:7420/intents");
  });

  it("parses applied and governance outcomes", async () => {
    fetchImpl.mockResolvedValueOnce(
      jsonResponse(200, { outcome: "applied", revision: 7 }),
    );
    fetchImpl.mockResolvedValueOnce(
      jsonResponse(200, {
        outcome: "sddk-governed",
        subject: "sddk:work-item:TEST-MODEL-001",
      }),
    );
    const applied = await client.promoteIntent("i1");
    expect(applied).toEqual({ outcome: "applied", revision: 7 });

    const governed = await client.promoteIntent("i2");
    expect(governed.outcome).toBe("sddk-governed");
  });

  it("surfaces stale promotion as a normal 409 result", async () => {
    fetchImpl.mockResolvedValue(
      jsonResponse(409, {
        outcome: "stale",
        current_revision: 7,
        base_revision: 6,
      }),
    );
    const outcome = await client.promoteIntent("i1");
    expect(outcome.outcome).toBe("stale");
    if (outcome.outcome === "stale") {
      expect(outcome.base_revision).toBe(6);
      expect(outcome.current_revision).toBe(7);
    }
  });
});
