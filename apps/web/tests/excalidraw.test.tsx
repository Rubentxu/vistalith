import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { VistalithClient } from "@vistalith/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ExcalidrawPanel } from "../src/components/ExcalidrawPanel.tsx";

afterEach(cleanup);

const SCENE = {
  type: "excalidraw",
  elements: [
    {
      id: "note-1",
      type: "text",
      text: "Remember the milk",
      x: 10,
      y: 20,
      width: 120,
      height: 40,
      customData: { vistalith: "vistalith:note:n-1" },
    },
  ],
};

const REPORT = {
  bound: [],
  created_primitives: [],
  skipped_bindings: [
    { namespace: "vistalith", kind: "sketch-element", id: "b-1" },
  ],
  unknown_subjects: [],
  unbound_elements: [],
};

function jsonResponse(status: number, body: unknown): Response {
  return {
    status,
    text: async () => JSON.stringify(body),
  } as unknown as Response;
}

describe("ExcalidrawPanel (slice 20, SPK-009)", () => {
  it("exports the scene with embedded identity and imports back as a no-op", async () => {
    const fetchImpl = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const url = new URL(String(input));
      if (url.pathname === "/canvas/excalidraw" && init?.method === undefined) {
        return Promise.resolve(jsonResponse(200, SCENE));
      }
      if (url.pathname === "/canvas/excalidraw" && init?.method === "POST") {
        const sent = JSON.parse(String(init.body));
        expect(sent.elements[0].customData.vistalith).toBe(
          "vistalith:note:n-1",
        );
        return Promise.resolve(jsonResponse(200, REPORT));
      }
      return Promise.resolve(jsonResponse(404, { error: "not found" }));
    });
    const client = new VistalithClient({
      baseUrl: "http://127.0.0.1:7420",
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });

    render(<ExcalidrawPanel client={client} onGraphChanged={() => {}} />);

    fireEvent.click(screen.getByRole("button", { name: "export scene" }));
    const source = await waitFor(() =>
      screen.getByLabelText("Excalidraw scene JSON"),
    );
    expect(source).toHaveValue(JSON.stringify(SCENE, null, 2));

    fireEvent.click(screen.getByRole("button", { name: "import scene" }));
    await waitFor(() => screen.getByTestId("excalidraw-report"));
    expect(
      screen.getByText(
        /bound 0 · created 0 ·\s*unchanged 1 · unknown 0 · unbound 0/,
      ),
    ).toBeInTheDocument();
  });

  it("reports unbound and unknown shapes explicitly", async () => {
    const fetchImpl = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      const url = new URL(String(input));
      if (url.pathname === "/canvas/excalidraw" && init?.method === "POST") {
        return Promise.resolve(
          jsonResponse(200, {
            bound: [],
            created_primitives: [],
            skipped_bindings: [],
            unknown_subjects: ["arch:system:missing"],
            unbound_elements: ["shape-2"],
          }),
        );
      }
      return Promise.resolve(jsonResponse(404, { error: "not found" }));
    });
    const client = new VistalithClient({
      baseUrl: "http://127.0.0.1:7420",
      fetchImpl: fetchImpl as unknown as typeof fetch,
    });

    render(<ExcalidrawPanel client={client} onGraphChanged={() => {}} />);

    fireEvent.change(screen.getByLabelText("Excalidraw scene JSON"), {
      target: { value: '{"elements":[]}' },
    });
    fireEvent.click(screen.getByRole("button", { name: "import scene" }));
    await waitFor(() => screen.getByTestId("excalidraw-report"));
    expect(screen.getByText(/unknown 1 · unbound 1/)).toBeInTheDocument();
  });
});
