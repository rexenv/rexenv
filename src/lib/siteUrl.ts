import { useQuery } from "@tanstack/react-query";
import { getGlobalStatus } from "@/lib/ipc";

/** A site's address: `https://host`, or `https://host:port` when the edge's HTTPS port is not
 *  443 (`core::edge_ports`, docs/PLAN-headless-web.md T2). The frontend's ONE builder — the same
 *  rule as Rust's `edge_ports::site_url`. */
export function siteUrl(host: string, edgeHttpsPort = 443): string {
  return edgeHttpsPort === 443 ? `https://${host}` : `https://${host}:${edgeHttpsPort}`;
}

/** The edge's HTTPS port from the Sidebar's `global-status` poll; 443 until the first answer. */
export function useEdgeHttpsPort(): number {
  const { data } = useQuery({ queryKey: ["global-status"], queryFn: getGlobalStatus });
  return data?.edgeHttpsPort ?? 443;
}

/** `siteUrl` bound to the edge's port from the Sidebar's `global-status` poll (one cache entry,
 *  refreshed every 2 s; 443 until the first answer). */
export function useSiteUrl(): (host: string) => string {
  const port = useEdgeHttpsPort();
  return (host: string) => siteUrl(host, port);
}
