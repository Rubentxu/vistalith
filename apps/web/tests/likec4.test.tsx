import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { VistalithClient } from "@vistalith/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LikeC4Panel } from "../src/components/LikeC4Panel.tsx";

afterEach(cleanup);

const EXPORTED_DSL = `specification {
  element system
  element container
  relationship calls
}

model {
  system checkout "Checkout" {
    metadata {
      vistalith 'arch:system:checkout'
    }
  }
  container payments "Payments" {
    metadata {
      vistalith 'arch:container:payments'
    }
  }
  checkout -[calls]-> payments
}

views {
  view overview {
    include *
  }
}
`;

const IMPORT_REPORT = {
  defined_subjects: [],
  updated_subjects: [],
  unchanged_subjects: [
    { namespace: "arch", kind: "system", id: "checkout" },
    { namespace: "arch", kind: "container", id: "payments" },
  ],
  deprecated_subjects: [],
  declared_relations: [],
  skipped_relations: [
    {
      from: { namespace: "arch", kind: "system", id: "checkout" },
      kind: "calls",
      to: { namespace: "arch", kind: "container", id: "payments" },
    },
  ],
};

const C4_DIFF = {
  from_revision: 1,
  to_revision: 2,
  added_elements: [
    {
      identity: "arch:component:gateway",
      name: "Gateway",
      authority: "authoritative",
      deprecated: false,
    },
  ],
  removed_elements: [],
  changed_elements: [
    {
      identity: "arch:system:checkout",
      level: "system",
      changes: [{ key: "name", from: "Checkout", to: "Checkout Prime" }],
    },
  ],
  added_relationships: [],
  removed_relationships: [],
  changed_relationships: [],
};

function textResponse(status: number, body: string): Response {
  return { status, text: async () => body } as unknown as Response;
}

function jsonResponse(status: number, body: unknown): Response {
  return textResponse(status, JSON.stringify(body));
}

describe("LikeC4Panel (slice 19, SPK-008)", () => {
  it("exports the DSL, imports it back and reports an identity no-op", async () => {
    const fetchImpl = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const url = new URL(String(input));
      if (url.pathname === "/views/c4/likec4" && init?.method === undefined) {
        return Promise.resolve(textResponse(200, EXPORTED_DSL));
      }
      if (url.pathname === "/views/c4/likec4" && init?.method === "POST") {
        expect(init.headers).toMatchObject({
          "content-type": "text/plain; charset=utf-8",
        });
        expect(String(init.body)).toContain("vistalith 'arch:system:checkout'");
        return Promise.resolve(jsonResponse(200, IMPORT_REPORT));
      }
      return Promise.resolve(jsonResponse(404, { error: "not found" }));
    });
    const client = new VistalithClient({
      baseUrl: "http://127.0.0.1:7420",
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });

    render(<LikeC4Panel client={client} onGraphChanged={() => {}} />);

    fireEvent.click(screen.getByRole("button", { name: "export DSL" }));
    const source = await waitFor(() => screen.getByLabelText("LikeC4 DSL"));
    expect(source).toHaveValue(EXPORTED_DSL);

    fireEvent.click(screen.getByRole("button", { name: "import DSL" }));
    await waitFor(() => screen.getByTestId("likec4-report"));
    expect(
      screen.getByText(/unchanged 2 · relations declared 0 · skipped 1/),
    ).toBeInTheDocument();
  });

  it("renders the architecture revision diff", async () => {
    const fetchImpl = vi.fn((input: RequestInfo | URL) => {
      const url = new URL(String(input));
      if (url.pathname === "/views/c4/diff") {
        expect(url.searchParams.get("from")).toBe("1");
        return Promise.resolve(jsonResponse(200, C4_DIFF));
      }
      return Promise.resolve(jsonResponse(404, { error: "not found" }));
    });
    const client = new VistalithClient({
      baseUrl: "http://127.0.0.1:7420",
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });

    render(<LikeC4Panel client={client} onGraphChanged={() => {}} />);

    const fromInput = screen.getByDisplayValue("0");
    fireEvent.change(fromInput, { target: { value: "1" } });
    fireEvent.click(screen.getByRole("button", { name: "diff → now" }));
    await waitFor(() => screen.getByTestId("likec4-diff"));
    expect(screen.getByText(/revisions 1 → 2/)).toBeInTheDocument();
    expect(screen.getByText(/Gateway/)).toBeInTheDocument();
    expect(
      screen.getByText(/name: "Checkout" → "Checkout Prime"/),
    ).toBeInTheDocument();
  });
});
