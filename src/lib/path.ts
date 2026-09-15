/** A path's parts as the OS wrote them. The backend hands the UI native paths —
 *  `C:\Users\…\w7-linked-plugin` on Windows — and a `split("/")` returns that
 *  whole string as the "last part": the Dell's "Link folder" put the full path
 *  in the folder-name field and the link was refused (ledger #629). Every path's
 *  last part goes through here — a Rust test scans the frontend for
 *  `split("/")`; a shown join uses `joinPath` (not scanned: routes and counts
 *  join on `/` too). */

/** The last part of `path`, either separator, trailing separators ignored. */
export function baseName(path: string): string {
  return path.split(/[\\/]+/).filter(Boolean).pop() ?? "";
}

/** `name` inside `parent`, with the separator `parent` already uses. */
export function joinPath(parent: string, name: string): string {
  const sep = parent.includes("\\") && !parent.includes("/") ? "\\" : "/";
  return parent.replace(/[\\/]+$/, "") + sep + name;
}
