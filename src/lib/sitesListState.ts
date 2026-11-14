import { create } from "zustand";
import type { SiteType } from "@/types";

export type SitesStatusFilter = "all" | "running" | "stopped";
export type SitesTypeFilter = "all" | SiteType;
export type SitesSort = "name" | "status" | "recent";

/** **The Sites list's view — filter, type, sort, search, scroll — outlives a visit
 * to a site.**
 *
 * It was `useState` inside the route, so it reset every time the route remounted:
 * filter to "lms", open one, come back, and all 25 sites were on screen again with
 * the list scrolled to the top (found reading the tree, 14 Nov 2026). The list is
 * where a user picks the NEXT site, so the view they built to pick it has to be
 * there when they return.
 *
 * In memory only, on purpose: it is a view, not a setting. A new launch opens on
 * All (the owner's ruling of 11 Sep 2026 — every site visible on arrival), and a
 * view restored from disk would break that. */
interface SitesListState {
  query: string;
  filter: SitesStatusFilter;
  typeFilter: SitesTypeFilter;
  sort: SitesSort;
  scrollTop: number;
  set: (patch: Partial<Omit<SitesListState, "set" | "reset">>) => void;
  /** "Show all sites": every narrowing gone. The sort stays — it hides nothing. */
  reset: () => void;
}

export const useSitesListState = create<SitesListState>((set) => ({
  query: "",
  filter: "all",
  typeFilter: "all",
  sort: "name",
  scrollTop: 0,
  set: (patch) => set(patch),
  reset: () => set({ query: "", filter: "all", typeFilter: "all" }),
}));
