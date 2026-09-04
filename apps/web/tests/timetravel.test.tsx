import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import type { GraphDiff } from "@vistalith/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { HistoryDiff, TimeTravelBar } from "../src/components/TimeTravel.tsx";

afterEach(cleanup);

describe("TimeTravelBar (SPEC-011)", () => {
  it("lists past revisions and reports live by default", () => {
    const onSelect = vi.fn();
    render(
      <TimeTravelBar currentRevision={4} asOf={null} onSelect={onSelect} />,
    );
    const select = screen.getByLabelText("time travel:") as HTMLSelectElement;
    expect(select.value).toBe("");
    expect(screen.getByText("live (revision 4)")).toBeInTheDocument();
    // Revisions 1..3 are offered, newest first.
    const options = [...select.querySelectorAll("option")].map(
      (option) => option.value,
    );
    expect(options).toEqual(["", "3", "2", "1"]);
  });

  it("emits the selected revision and returns to live", () => {
    const onSelect = vi.fn();
    const { rerender } = render(
      <TimeTravelBar currentRevision={4} asOf={null} onSelect={onSelect} />,
    );
    fireEvent.change(screen.getByLabelText("time travel:"), {
      target: { value: "2" },
    });
    expect(onSelect).toHaveBeenLastCalledWith(2);

    rerender(
      <TimeTravelBar currentRevision={4} asOf={2} onSelect={onSelect} />,
    );
    // The read-only marker explains that the lens shows a historical replay.
    expect(screen.getByText("read-only · revision 2")).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("time travel:"), {
      target: { value: "" },
    });
    expect(onSelect).toHaveBeenLastCalledWith(null);
  });
});

describe("HistoryDiff (SPEC-011)", () => {
  it("renders the structural diff counts and identities", () => {
    const diff: GraphDiff = {
      added_subjects: [
        { namespace: "agentic", kind: "thread", id: "f1" },
        { namespace: "arch", kind: "container", id: "svc-2" },
      ],
      removed_subjects: [],
      changed_subjects: [
        {
          subject: { namespace: "arch", kind: "container", id: "svc" },
          changes: [{ key: "status", from: "draft", to: "final" }],
        },
      ],
      added_relations: [],
      removed_relations: [
        {
          from: { namespace: "arch", kind: "container", id: "a" },
          kind: "depends_on",
          to: { namespace: "arch", kind: "container", id: "b" },
        },
      ],
      changed_relations: [],
    };
    render(<HistoryDiff diff={diff} from={3} />);
    expect(screen.getByText("diff revision 3 → live")).toBeInTheDocument();
    expect(screen.getByText(/2 subjects:/)).toBeInTheDocument();
    expect(screen.getByText(/agentic:thread:f1/)).toBeInTheDocument();
    expect(screen.getByText(/1 changed subject/)).toBeInTheDocument();
    expect(screen.getByText(/1 relation\(s\)/)).toBeInTheDocument();
  });

  it("reports when there is nothing different", () => {
    const diff: GraphDiff = {
      added_subjects: [],
      removed_subjects: [],
      changed_subjects: [],
      added_relations: [],
      removed_relations: [],
      changed_relations: [],
    };
    render(<HistoryDiff diff={diff} from={2} />);
    expect(screen.getByText("no structural differences")).toBeInTheDocument();
  });
});
