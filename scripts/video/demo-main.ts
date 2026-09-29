/** Boots the real app with a scripted backend: every IPC call the UI makes is
 *  answered by demo-backend.ts, so the recording shows the shipped screens with
 *  demo data and never touches a real site, service or folder. */
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { handle } from "./demo-backend";

mockWindows("main");
mockIPC(handle, { shouldMockEvents: true });

// The app's router starts from the URL; this page lives at /scripts/video/…
history.replaceState(null, "", "/sites");
await import("/src/main.tsx");
