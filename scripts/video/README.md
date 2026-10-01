# video — narrated tutorial recordings of the real UI

**Dev tooling only. Nothing here ships.**

Records rexenv's own screens (the shipped React components, not a mockup) driven by a
script, against a scripted backend, with a text-to-speech voiceover. No real site,
service or folder is touched, and nothing clicks on the machine's own screen. The app
runs in a headless browser, inside a stage page that draws the window frame, cursor,
camera and captions.

| File | What it is |
|---|---|
| `stage.html` / `stage.ts` | The 1920×1080 frame, driven through `window.stage`. It holds the app window at its real 1180×760 size, a cursor, the click ripple, camera zoom, captions (top or bottom), spotlights, title cards and the sync flash. It also draws the parts that live outside the webview: a terminal (`stage.terminal`), and the macOS menu bar with rexenv's tray menu (`stage.tray`). A stage-level cursor (`stage.scursor`) is used for those. |
| `demo.html` / `demo-main.ts` | Boots the app (`src/main.tsx`) with `mockIPC`, so every IPC call goes to the demo backend. `?scene=` selects the video's scene; `?path=` is the screen it opens on. |
| `demo-backend.ts` | The base backend: demo sites and services, the shell's quiet answers, and the WordPress provision job. Every answer is a fresh copy. |
| `scenes/<name>.ts` | One video's fixtures and replayed jobs. It gets the shared state (`ctx`) and returns handlers that answer before the base. Each file's header says which Rust code it mirrors. **When that flow changes in the backend, change the scene too**: a tutorial that shows steps the app never takes teaches the wrong thing. | `?scene=a,b` loads several into one app, so a montage moves between their screens through the sidebar instead of reloading (a reload is ~1.5 s of empty stage).
| `wp-fixtures.ts` | A WordPress site's contents (plugins, themes, users, Tools cards), shared by the WordPress scenes. |
| `adminer/index.html` | A stand-in for the Database Browser frame. The real one is served over `rexdb://`, which exists only inside the Tauri webview. |
| `voice.mjs` | Narration: synthesis (edge-tts, cached), the aligned track, SRT and preview page. |
| `lib.mjs` | Recorder helpers. Options: `openStage({ narration, scene, path, ready, timezoneId })`, plus the montage knobs `orient` (`"portrait"` = 1080×1920), `speed` (job replay length), `rate` (voice), `gap` (between lines), `switchSettle`. Actions: glide, click (`pre`/`post` pauses), type, `camera`, `union`, `containerOf`, `spotlight`, `term.*`, `say`/`voiceDone`, `switchScene`, `headline`. It also handles save, sync and trim. |
| `record-<name>.mjs` | One scene per video, with its narration lines at the top. |
| `videos.json` | The series: order, tier, title, one line each. `script`/`args` let one recorder make several cuts (the intro's vertical one). |
| `record-all.mjs` | Records everything (or the names given), then rebuilds the index. |
| `make-index.mjs` | Writes `out/index.html`. |

## Run

```
# once, in this folder:
npm install                                   # Playwright 1.61.1
npx playwright install chromium               # skipped if already in the cache
python3 -m venv .venv && .venv/bin/pip install edge-tts

# terminal 1 — from the repo root, the same dev server the wk-checks use:
npx vite --port 5199 --strictPort

# terminal 2 — in this folder:
npm run all                                   # every video, ~25 min
npm run mail                                  # or one of them (see package.json)
```

Environment variables:
- `VIDEO_BASE_URL` overrides `http://localhost:5199`.
- `VIDEO_VOICE` sets the voice. The default is `en-US-AndrewNeural`. Run
  `.venv/bin/edge-tts --list-voices` for the rest, Bangla included:
  `bn-BD-NabanitaNeural`, `bn-BD-PradeepNeural`.

## Output (`out/`)

| File | Use |
|---|---|
| `index.html` | Every video with its length and links. |
| `<name>.webm` | The video: 1920×1080, 25 fps, silent. Plays in Chrome, Firefox, VLC and on YouTube; not in QuickTime. |
| `<name>.voice.m4a` / `.wav` | The voiceover, exactly the video's length. Place it at 0:00 under the video. |
| `<name>.srt` | Subtitles of the narration. |
| `<name>.preview.html` | Open in Chrome: plays the video with the voice in step. |
| `<name>.cues.json` | When each line starts. `node -e 'import("./voice.mjs").then(v=>v.rebuild("<name>"))'` rebuilds the audio, SRT and preview from it without re-recording. |

For an MP4, install a full ffmpeg; the one Playwright downloads has no audio encoder:

```
ffmpeg -i x.webm -i x.voice.m4a -c:v libx264 -crf 18 -pix_fmt yuv420p -c:a copy -movflags +faststart x.mp4
```

## How the voice stays in sync

Every line is synthesised before recording, so its length is known. The scene `say`s a
line and acts while it plays; the next `say` waits for it to end. The picture is paced by
the voice, never the other way round.

The recorder's clock and the video's are different clocks: headless WebKit's frames
arrived ~0.5 s late, constantly. So `save` finds a one-frame magenta flash shown before
the trim point, and trims at the video's own time for it.

## Traps this tooling has already paid for

- **Chromium, not WebKit.** Headless WebKit on macOS cannot be larger than the screen: on
  a laptop display every frame came out at 0.9 scale in a grey border.
  `VIDEO_ENGINE=webkit` records in the macOS app's own engine on a display at least 1920
  wide.
- **Every stage layer must be `pointer-events: none`.** An invisible terminal, and the
  captions, ate the clicks meant for the app below them.
- **Never `scrollIntoView` inside the app.** It also scrolls the stage around the iframe.
  `box()` scrolls the element's own scroller instead.
- **A smooth scroll outlasts a fixed wait.** `box()` reads the rect until it holds still;
  at a fixed 250 ms a long page was still scrolling and the click landed below the
  window. For the same reason `union()` re-reads every rect after the last scroll.
- **A zoom covers the headline beside it.** The stage hides the headline while zoomed,
  and brings it back only once the window has shrunk again. In portrait, a target as
  wide as the window is not panned to.
- **Aim after the layout settles.** A zoomed camera plus a card that moves (a job card
  clearing itself, a list re-sorting) sends the click off-frame. `click()` pulls the
  camera back when the target is off-frame. Scenes wait for the settled state before
  framing it.
- **Answer fresh copies.** Returning the same mutated array let React Query see "no
  change". The sidebar said 4 sites beside a list of 5.
- **Answer every shape the screen reads.** A `null` where the UI expects an object or
  array blanks the whole app, and it records as a black window. Unanswered commands are
  listed at the end of each run.
- **Backend stamps are SQLite UTC (`YYYY-MM-DD HH:MM:SS`), not ISO.** The UI prints any
  other shape verbatim.
- **Only what renders in the webview can be recorded.** The browser opening a site,
  wp-admin and macOS password dialogs are narrated, not drawn. The terminal and the menu
  bar are drawn, with their real text.
- **Synthesis uses Microsoft's read-aloud service** (edge-tts, an unofficial client). That
  is fine for drafts. For a published video, the same voices are licensed through Azure
  Speech.
