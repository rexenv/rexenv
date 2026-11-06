import { LogOut } from "lucide-react";
import { Button } from "@/components/ui/button";
import { signOutOfWebUi } from "@/lib/ipc";

/**
 * Settings → General, over the web UI only: end this browser's session. The session's limits and the
 * single-use sign-in link are the server's (`web_rules::SESSION_IDLE`, `SESSION_MAX`,
 * `SIGN_IN_CODE_TTL`); signing back in takes a new `rex web url`.
 */
export function WebSessionCard() {
  return (
    <div className="rounded-[13px] border border-rex-border-subtle bg-rex-surface-1 px-5 py-4">
      <div className="flex items-center gap-4">
        <div className="flex-1">
          <div className="text-[0.84375rem] font-medium text-rex-text">Signed in to this server&apos;s web UI</div>
          <p className="mt-1 text-[0.75rem] leading-[1.55] text-rex-text-muted">
            Signing in again takes a new link from <span className="font-mono">rex web url</span> on the server.
          </p>
        </div>
        <Button size="sm" variant="secondary" onClick={() => void signOutOfWebUi()}>
          <LogOut className="h-3.5 w-3.5" /> Sign out
        </Button>
      </div>
    </div>
  );
}
