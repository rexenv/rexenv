/** The `rex` CLI: not on PATH until Settings → General → Command-line tool →
 *  Install (`cli_install`, one admin prompt, a symlink at /usr/local/bin/rex).
 *  `window.__scene.createShop()` adds what `rex site create shop.rex` + `rex site php shop.rex 8.3` leave behind, so
 *  the app shows the site the terminal created. */
import type { SceneCtx } from "../demo-backend";

export default function rexCli(ctx: SceneCtx) {
  let installed = false;
  const status = () => ({
    available: true,
    installed,
    current: installed,
    linkPath: "/usr/local/bin/rex",
    bundledPath: "/Applications/rexenv.app/Contents/MacOS/rex",
    onPath: null,
  });
  (window as unknown as { __scene: object }).__scene = {
    createShop() {
      ctx.sites.push(ctx.site({ id: "9", name: "shop", domain: "shop.rex", type: "wordpress", phpVersion: "8.3", createdAt: "2026-09-30 11:20:00" }));
    },
  };
  return {
    cli_status: () => status(),
    cli_install: async () => {
      await ctx.sleep(1600);
      installed = true;
      return status();
    },
  };
}
