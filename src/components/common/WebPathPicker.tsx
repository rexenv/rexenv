import { useEffect, useState } from "react";
import { create } from "zustand";
import { Button } from "@/components/ui/button";
import { Overlay } from "@/components/ui/dialog";
import { browseDir } from "@/lib/ipc";
import type { DirListing } from "@/types";

/**
 * The web UI's folder and file picker (docs/PLAN-headless-web.md T8). In a browser the window's
 * native dialog would open on the SERVER's invisible display, so `pickFolder` / `pickSqlFile` /
 * `pickZipFiles` ask this instead over the web transport: it lists the server's folders through
 * `browse_dir`, which answers only under the home and Sites folders (`core::browse`, ledger #931)
 * and words its own refusals — this file holds no rule.
 */
interface Request {
  title: string;
  /** Empty = choose a folder; otherwise files with these extensions. */
  extensions: string[];
  multiple: boolean;
  start?: string;
  resolve: (picked: string[] | null) => void;
}

const usePicker = create<{ req: Request | null; set: (r: Request | null) => void }>((set) => ({
  req: null,
  set: (req) => set({ req }),
}));

/** Ask for a path in the browser. Resolves with the chosen paths, or `null` when cancelled. */
export function requestPath(opts: Omit<Request, "resolve">): Promise<string[] | null> {
  return new Promise((resolve) => usePicker.getState().set({ ...opts, resolve }));
}

export function WebPathPicker() {
  const req = usePicker((s) => s.req);
  const close = usePicker((s) => s.set);
  const [listing, setListing] = useState<DirListing | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [chosen, setChosen] = useState<string[]>([]);

  const open = (path?: string) => {
    if (!req) return;
    setError(null);
    browseDir(path ?? null, req.extensions)
      .then((l) => {
        setListing(l);
        setChosen([]);
      })
      .catch((e) => setError(String(e)));
  };

  useEffect(() => {
    if (req) open(req.start);
    else setListing(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- a new request is the only trigger
  }, [req]);

  if (!req) return null;
  const finish = (picked: string[] | null) => {
    req.resolve(picked);
    close(null);
  };
  const folderMode = req.extensions.length === 0;
  const toggle = (path: string) =>
    setChosen((c) => (c.includes(path) ? c.filter((p) => p !== path) : req.multiple ? [...c, path] : [path]));

  return (
    <Overlay onClose={() => finish(null)} cardClassName="w-[560px]">
      <div className="text-[0.875rem] font-semibold text-rex-text">{req.title}</div>
      <div className="mt-2 flex flex-wrap gap-1.5">
        {listing?.roots.map((r) => (
          <Button key={r} size="sm" variant="ghost" onClick={() => open(r)}>
            <span className="font-mono text-[0.75rem]">{r}</span>
          </Button>
        ))}
      </div>
      <div className="mt-2 truncate font-mono text-[0.75rem] text-rex-text-muted">{listing?.path}</div>
      {error && <div className="mt-2 text-[0.75rem] text-status-error-bright">{error}</div>}
      <ul className="mt-2 h-[280px] overflow-auto rounded-lg border border-rex-border-subtle">
        {listing?.parent && (
          <li>
            <button type="button" className="w-full px-3 py-1.5 text-left font-mono text-[0.78125rem] hover:bg-rex-surface-2" onClick={() => open(listing.parent ?? undefined)}>
              ..
            </button>
          </li>
        )}
        {listing?.entries.map((e) => (
          <li key={e.path}>
            <button
              type="button"
              className={`w-full px-3 py-1.5 text-left font-mono text-[0.78125rem] hover:bg-rex-surface-2 ${chosen.includes(e.path) ? "bg-rex-surface-2 text-rex-text" : "text-rex-text-muted"}`}
              onClick={() => (e.dir ? open(e.path) : toggle(e.path))}
            >
              {e.dir ? `${e.name}/` : e.name}
            </button>
          </li>
        ))}
      </ul>
      <div className="mt-4 flex justify-end gap-2">
        <Button variant="ghost" onClick={() => finish(null)}>
          Cancel
        </Button>
        <Button
          disabled={folderMode ? !listing : chosen.length === 0}
          onClick={() => finish(folderMode ? (listing ? [listing.path] : null) : chosen)}
        >
          {folderMode ? "Choose this folder" : "Choose"}
        </Button>
      </div>
    </Overlay>
  );
}
