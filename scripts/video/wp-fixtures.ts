/** A WordPress site's contents for the videos that open its WordPress tab:
 *  plugins (one with an update), themes, users, and the database export /
 *  import answers. Shapes are the IPC types (`WpPlugin`, `WpTheme`, `WpUser`);
 *  the first `wp_plugins` call is the fast one (`checkUpdates:false`), the
 *  second fills in the update badges, as the real tab does. */
import type { SceneCtx } from "./demo-backend";
import type { WpPlugin, WpTheme, WpUser } from "@/types";

type Args = Record<string, unknown> | undefined;

export function wpState() {
  const plugins: WpPlugin[] = [
    { name: "akismet", status: "active", version: "5.5", update: "none", updateVersion: "", title: "Akismet Anti-spam: Spam Protection", file: "akismet/akismet.php" },
    { name: "contact-form-7", status: "active", version: "6.1.2", update: "none", updateVersion: "", title: "Contact Form 7", file: "contact-form-7/wp-contact-form-7.php" },
    { name: "wordpress-seo", status: "active", version: "26.1", update: "available", updateVersion: "26.2", title: "Yoast SEO", file: "wordpress-seo/wp-seo.php" },
    { name: "hello-dolly", status: "inactive", version: "1.7.2", update: "none", updateVersion: "", title: "Hello Dolly", file: "hello.php" },
  ];
  const themes: WpTheme[] = [
    { name: "twentytwentyfive", status: "active", version: "1.3", update: "none", updateVersion: "", title: "Twenty Twenty-Five", screenshot: null },
    { name: "twentytwentyfour", status: "inactive", version: "1.3", update: "none", updateVersion: "", title: "Twenty Twenty-Four", screenshot: null },
    { name: "twentytwentythree", status: "inactive", version: "1.6", update: "none", updateVersion: "", title: "Twenty Twenty-Three", screenshot: null },
  ];
  const users: WpUser[] = [
    { id: 1, login: "admin", email: "admin@agency-blog.rex", roles: "administrator", name: "admin" },
    { id: 2, login: "nadia", email: "nadia@agency-blog.rex", roles: "editor", name: "Nadia Rahman" },
  ];
  return { plugins, themes, users };
}

export function wpHandlers(ctx: SceneCtx, st = wpState()) {
  const withUpdates = (list: Array<WpPlugin | WpTheme>, check: boolean) =>
    list.map((p) => (check ? p : { ...p, update: "none", updateVersion: "" }));
  return {
    wp_plugins: (a: Args) => withUpdates(st.plugins, Boolean(a?.checkUpdates)),
    wp_themes: (a: Args) => withUpdates(st.themes, Boolean(a?.checkUpdates)),
    wp_users: () => st.users,
    wp_primary_admin: () => 1,
    wp_network_sites: () => [],
    wp_install_active: () => null,
    wp_org_plugin_icons: () => ({}),
    repo_assets: () => [],
    repo_unmanaged: () => [],
    repo_site_jobs: () => [],
    adminer_set_theme: () => null,
    // The Tools sub-tab's cards.
    wp_core_versions: () => [
      { version: "7.1", status: "latest" },
      { version: "7.0.4", status: "outdated" },
      { version: "6.9.6", status: "outdated" },
    ],
    wp_options: (a: Args) => {
      const s = ctx.sites.find((x) => x.id === a?.id);
      const f = (name: string, label: string, kind: string, value: string, min: number | null = null, max: number | null = null) => ({ name, label, kind, min, max, value, editable: true, note: null });
      return {
        fields: [
          f("blogname", "Site title", "text", s?.name ?? "My Site"),
          f("blogdescription", "Tagline", "text", ""),
          f("admin_email", "Admin email", "email", `admin@${s?.domain ?? "site.rex"}`),
          f("timezone_string", "Timezone", "timezone", "Asia/Dhaka"),
          f("date_format", "Date format", "text", "F j, Y"),
          f("time_format", "Time format", "text", "g:i a"),
          f("start_of_week", "Week starts on", "weekday", "1"),
          f("posts_per_page", "Posts per page", "int", "10", 1, 1000),
          f("default_role", "New user default role", "role", "subscriber"),
          f("users_can_register", "Anyone can register", "bool", "0"),
        ],
        timezones: ["UTC", "Asia/Dhaka", "Europe/London", "America/New_York"],
        roles: [
          { name: "Administrator", role: "administrator" },
          { name: "Editor", role: "editor" },
          { name: "Author", role: "author" },
          { name: "Contributor", role: "contributor" },
          { name: "Subscriber", role: "subscriber" },
        ],
      };
    },
    wp_cron_events: () => [
      { hook: "wp_version_check", nextRun: "2026-09-30 16:02:11", nextRunRelative: "5 hours 11 minutes", recurrence: "12 hours", args: "" },
      { hook: "wp_update_plugins", nextRun: "2026-09-30 16:02:11", nextRunRelative: "5 hours 11 minutes", recurrence: "12 hours", args: "" },
      { hook: "wp_scheduled_delete", nextRun: "2026-10-01 09:14:40", nextRunRelative: "22 hours 23 minutes", recurrence: "1 day", args: "" },
    ],
    wp_debug_get: () => false,
    wp_debug_flag_get: () => false,
    wp_maintenance_get: () => false,
    wp_permalink_get: () => "/%postname%/",
    wp_languages: () => [
      { language: "en_US", englishName: "English (United States)", nativeName: "English (United States)", status: "active" },
      { language: "bn_BD", englishName: "Bengali (Bangladesh)", nativeName: "বাংলা", status: "uninstalled" },
      { language: "fr_FR", englishName: "French (France)", nativeName: "Français", status: "uninstalled" },
    ],
    wp_db_export: async (a: Args) => {
      await ctx.sleep(1400);
      const s = ctx.sites.find((x) => x.id === a?.id);
      return `/Users/demo/Downloads/${s?.domain ?? "site.rex"}-db.sql`;
    },
    wp_db_import: async () => {
      await ctx.sleep(2200);
      return null;
    },
    "plugin:dialog|open": () => "/Users/demo/Downloads/agency-blog-backup.sql",
    reveal_path: () => null,
    open_external: () => null,
    wp_admin_login_url: (a: Args) => {
      const s = ctx.sites.find((x) => x.id === a?.id);
      return `https://${s?.domain}/?rexenv_login=3f2c7e1a-9b44-4d2e-8c1f-6a0b5d7e9f21&rexenv_user=1`;
    },
  };
}
