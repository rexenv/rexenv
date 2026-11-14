import { openExternal, wpAdminLoginUrl } from "@/lib/ipc";
import { toast } from "@/lib/toast";
import type { Site } from "@/types";

/** The one-time sign-in link for a site's WordPress admin — or, when WP-CLI
 *  cannot mint one, the ordinary login page, with the reason said. */
export async function magicLoginUrl(site: Pick<Site, "id" | "domain">, url: string): Promise<string> {
  try {
    return await wpAdminLoginUrl(site.id);
  } catch (e) {
    toast.error(`Auto-login unavailable — opening the WordPress login page instead.\n${String(e)}`);
    return `${url}/wp-admin/`;
  }
}

/** **Magic Login**: sign in to a site's WordPress admin in the browser. One
 *  function for the site page's button and the Sites row's (14 Nov 2026) — it
 *  lived in `routes/SiteDetail.tsx` while that was its only caller. */
export async function openWpAdmin(site: Pick<Site, "id" | "domain">, url: string): Promise<void> {
  await openExternal(await magicLoginUrl(site, url));
}
