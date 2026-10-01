/** Boots the real app with a scripted backend: every IPC call the UI makes is
 *  answered by demo-backend.ts (plus the video's scene, `?scene=`), so the
 *  recording shows the shipped screens with demo data and never touches a real
 *  site, service or folder. `?path=` is the screen the app opens on. */
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { createBackend } from "./demo-backend";

const params = new URLSearchParams(location.search);
const handle = await createBackend(params.get("scene"));

mockWindows("main");
mockIPC(handle, { shouldMockEvents: true });

// The app's router starts from the URL; this page lives at /scripts/video/…
history.replaceState(null, "", params.get("path") ?? "/sites");
await import("/src/main.tsx");
