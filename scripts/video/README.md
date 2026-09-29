# video — narrated tutorial recordings of the real UI

**Dev tooling only. Nothing here ships.**

Records rexenv's own screens (the shipped React components, not a mockup)
driven by a script, against a scripted backend, with a text-to-speech
voiceover. No real site, service or folder is touched, and nothing clicks on
the machine's own screen: the app runs in a headless browser inside a stage
page that draws the window frame, a cursor, the camera and the captions.

| File | What it is |
|---|---|
| `stage.html` / `stage.ts` | The 1920×1080 frame: background, the app window at its real 1180×760 size, cursor, click ripple, camera zoom, captions, spotlight, title cards, the sync flash. Driven through `window.stage`. |
| `demo.html` / `demo-main.ts` | Boots the app (`src/main.tsx`) with `mockIPC`, so every IPC call goes to the demo backend. |
| `demo-backend.ts` | The scripted backend: demo sites and services, and the provision job replayed with the phase list, labels and log lines the Rust job emits (`commands/site_provision.rs`). **When a recorded flow changes in the backend, change it here too** — a tutorial that shows steps the app never takes teaches the wrong thing. An unanswered command is answered `null` and listed at the end of the run, with any page error — a screen that crashes on a missing fixture records as a blank. |
| `voice.mjs` | Narration: synthesis (edge-tts, cached), the aligned track, SRT, preview page. |
| `lib.mjs` | Recorder helpers: glide/click/type (the real mouse lands under the drawn cursor, camera included), camera, captions, `say`/`voiceDone`, save + sync + trim. |
| `record-*.mjs` | One scene per video, its narration lines at the top. |

## Run

```
# once, in this folder:
npm install                                   # Playwright 1.61.1
npx playwright install chromium               # skipped if already in the cache
python3 -m venv .venv && .venv/bin/pip install edge-tts

# terminal 1 — from the repo root, the same dev server the wk-checks use:
npx vite --port 5199 --strictPort

# terminal 2 — in this folder:
npm run wp-install
```

`VIDEO_BASE_URL` overrides `http://localhost:5199`; `VIDEO_VOICE` the voice
(default `en-US-AndrewNeural`; `.venv/bin/edge-tts --list-voices` lists them,
Bangla included: `bn-BD-NabanitaNeural`, `bn-BD-PradeepNeural`).

## Output (`out/`)

| File | Use |
|---|---|
| `<name>.webm` | The video, 1920×1080, 25 fps, silent. Chrome, Firefox, VLC, YouTube; not QuickTime. |
| `<name>.voice.m4a` / `.wav` | The voiceover, exactly the video's length — place it at 0:00 under the video. |
| `<name>.srt` | Subtitles of the narration (YouTube's subtitle upload). |
| `<name>.preview.html` | Open in Chrome: plays the video with the voice in step, to review before editing. |
| `<name>.cues.json` | When each line starts; `node -e 'import("./voice.mjs").then(v=>v.rebuild("<name>"))'` rebuilds the audio, SRT and preview from it without re-recording. |

## How the voice stays in sync

Every line is synthesised before recording, so its length is known; the scene
`say`s a line and acts while it plays, and the next `say` waits for it to end —
the picture is paced by the voice, never the other way round. The recorder's
clock and the video's are different clocks (headless WebKit's frames arrived
~0.5 s late, constantly), so `save` finds a one-frame magenta flash shown before
the trim point and trims at the video's own time for it.

## Limits

- **Engine: Chromium, not WebKit.** Headless WebKit on macOS cannot be larger
  than the screen: on a laptop display every frame came out at 0.9 scale in a
  grey border. `VIDEO_ENGINE=webkit` records in the macOS app's own engine on a
  display at least 1920 wide.
- Only what renders inside the app window can be recorded — not the browser
  opening the site, wp-admin, the terminal, macOS password prompts or the tray.
- Native `<select>` popups don't render in a headless recording; a scene points
  at a select rather than opening it.
- Synthesis sends the narration text to Microsoft's read-aloud service
  (edge-tts, an unofficial client). Fine for drafts; for a published video,
  the same voices are licensed through Azure Speech.
- Joining voice and video into one file needs a full ffmpeg
  (`ffmpeg -i x.webm -i x.voice.m4a -c:v libx264 -crf 18 -c:a copy x.mp4`);
  the ffmpeg Playwright downloads has no audio encoder.
