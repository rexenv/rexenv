import { useMutationState, type MutationKey } from "@tanstack/react-query";

// The variables of EVERY pending call of the mutation(s) under `key`, read from
// the mutation cache. A per-row busy flag must use this, never
// `m.isPending && m.variables === row`: `variables` holds only the LAST call,
// so pressing a second row's button put the first row back to idle while its
// own call was still running (PHP 8.0 then 8.1 Install, 16 Oct 2026). A call
// stays in the cache until it settles, so a row is busy exactly as long as
// its own call — however many rows are in flight.
export function usePendingVariables<V>(key: MutationKey): V[] {
  return useMutationState({
    filters: { mutationKey: key, status: "pending" },
    select: (m) => m.state.variables as V,
  });
}
