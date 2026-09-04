import { useQuery } from "@tanstack/react-query";
import type { GraphState, Health } from "@vistalith/client";
import { client } from "./api.ts";

export function useHealth(): Health | undefined {
  const query = useQuery({
    queryKey: ["health"],
    queryFn: () => client.health(),
    refetchInterval: 5_000,
    retry: 1,
  });
  return query.data;
}

export function useGraph(): GraphState | undefined {
  const query = useQuery({
    queryKey: ["graph"],
    queryFn: () => client.graph(),
    refetchInterval: 2_000,
    retry: 1,
  });
  return query.data;
}
