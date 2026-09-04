/**
 * Cross-lens selection store.
 *
 * RULE (visual/SUBJECT-REF.md): every lens maps renderer-native IDs to
 * SubjectRefs. Selection propagates SubjectRefs, not node IDs — so the store
 * holds `SubjectRef` values only.
 */

import { isSameSubject, type SubjectRef } from "@vistalith/client";
import { create } from "zustand";

interface SelectionState {
  selected: SubjectRef | null;
  select: (ref: SubjectRef) => void;
  /** Toggles: selecting the already-selected subject clears it. */
  toggle: (ref: SubjectRef) => void;
  clear: () => void;
}

export const useSelection = create<SelectionState>((set) => ({
  selected: null,
  select: (ref) => set({ selected: ref }),
  toggle: (ref) =>
    set((state) => ({
      selected:
        state.selected && isSameSubject(state.selected, ref) ? null : ref,
    })),
  clear: () => set({ selected: null }),
}));
