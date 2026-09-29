// Narration: each line is synthesised once (Microsoft neural voices through
// edge-tts, cached by voice + text), timed by the recorder as the scene plays,
// then laid onto one track that starts at the video's first frame.
//
// Everything here is local file work except the synthesis itself, which sends
// the narration text to Microsoft's read-aloud service.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

export const VOICE = process.env.VIDEO_VOICE ?? "en-US-AndrewNeural";
const RATE = 24000; // every clip is converted to 24 kHz mono 16-bit, so they splice as raw samples
const EDGE_TTS = path.join(import.meta.dirname, ".venv", "bin", "edge-tts");

/** lines: { key: { text, say? } } — `text` is the subtitle, `say` what is spoken
 *  when the two differ (".rex" reads as "dot rex", "rexenv" as "rex env"). */
export function synthesize(lines, cacheDir) {
  if (!existsSync(EDGE_TTS)) {
    throw new Error("edge-tts is not installed — run: python3 -m venv .venv && .venv/bin/pip install edge-tts");
  }
  mkdirSync(cacheDir, { recursive: true });
  const out = {};
  for (const [key, line] of Object.entries(lines)) {
    const spoken = line.say ?? line.text;
    const id = createHash("sha1").update(`${VOICE}\n${spoken}`).digest("hex").slice(0, 16);
    const mp3 = path.join(cacheDir, `${id}.mp3`);
    const wav = path.join(cacheDir, `${id}.wav`);
    if (!existsSync(wav)) {
      execFileSync(EDGE_TTS, ["--voice", VOICE, "--text", spoken, "--write-media", mp3]);
      execFileSync("afconvert", ["-f", "WAVE", "-d", `LEI16@${RATE}`, "-c", "1", mp3, wav]);
    }
    const samples = readWav(wav);
    out[key] = { ...line, wav, dur: samples.length / 2 / RATE };
  }
  return out;
}

/** The 16-bit sample bytes of a WAV (chunks walked, not assumed at byte 44 —
 *  afconvert writes an extra FLLR chunk before `data`). */
function readWav(file) {
  const b = readFileSync(file);
  let at = 12;
  while (at < b.length) {
    const id = b.toString("ascii", at, at + 4);
    const size = b.readUInt32LE(at + 4);
    if (id === "data") return b.subarray(at + 8, at + 8 + size);
    at += 8 + size + (size % 2);
  }
  throw new Error(`no data chunk in ${file}`);
}

function wavFile(pcm) {
  const h = Buffer.alloc(44);
  h.write("RIFF", 0);
  h.writeUInt32LE(36 + pcm.length, 4);
  h.write("WAVEfmt ", 8);
  h.writeUInt32LE(16, 16);
  h.writeUInt16LE(1, 20); // PCM
  h.writeUInt16LE(1, 22); // mono
  h.writeUInt32LE(RATE, 24);
  h.writeUInt32LE(RATE * 2, 28);
  h.writeUInt16LE(2, 32);
  h.writeUInt16LE(16, 34);
  h.write("data", 36);
  h.writeUInt32LE(pcm.length, 40);
  return Buffer.concat([h, pcm]);
}

const stamp = (t, sep) => {
  const ms = Math.max(0, Math.round(t * 1000));
  const p = (n, w = 2) => String(n).padStart(w, "0");
  return `${p(Math.floor(ms / 3600000))}:${p(Math.floor(ms / 60000) % 60)}:${p(Math.floor(ms / 1000) % 60)}${sep}${p(ms % 1000, 3)}`;
};

/** Rebuild <name>'s track, SRT and preview from the cues a recording saved. */
export function rebuild(name) {
  const dir = path.join(import.meta.dirname, "out");
  const { videoSeconds, cues } = JSON.parse(readFileSync(path.join(dir, `${name}.cues.json`), "utf8"));
  return writeNarration(cues, { dir, name, videoSeconds });
}

/** cues: [{ at (s, video time), dur, wav, text }] → <name>.voice.m4a (the whole
 *  video's length, silence between lines), <name>.srt, and preview.html. */
export function writeNarration(cues, { dir, name, videoSeconds }) {
  const track = Buffer.alloc(Math.ceil(videoSeconds * RATE) * 2);
  for (const c of cues) {
    const pcm = readWav(c.wav);
    const start = Math.max(0, Math.round(c.at * RATE) * 2);
    pcm.copy(track, start, 0, Math.min(pcm.length, track.length - start));
  }
  const wav = path.join(dir, `${name}.voice.wav`);
  const m4a = path.join(dir, `${name}.voice.m4a`);
  writeFileSync(wav, wavFile(track));
  // Up to 48 kHz (video's rate): AAC refuses 128 kbps for a 24 kHz mono source.
  execFileSync("afconvert", ["-f", "m4af", "-d", "aac@48000", "-b", "128000", wav, m4a]);

  const srt = cues
    .map((c, i) => `${i + 1}\n${stamp(c.at, ",")} --> ${stamp(c.at + c.dur, ",")}\n${c.text}\n`)
    .join("\n");
  writeFileSync(path.join(dir, `${name}.srt`), srt);

  // A review page: the video with the voice track kept in step, so the result
  // can be watched before it goes into an editor.
  writeFileSync(
    path.join(dir, `${name}.preview.html`),
    `<!doctype html><meta charset="utf-8"><title>${name} — preview</title>
<style>body{margin:0;background:#111;display:grid;place-items:center;height:100vh}video{max-width:96vw;max-height:92vh}</style>
<video id="v" src="${name}.webm" controls></video><audio id="a" src="${name}.voice.m4a" preload="auto"></audio>
<script>
const v = document.getElementById("v"), a = document.getElementById("a");
const sync = () => { if (Math.abs(a.currentTime - v.currentTime) > 0.08) a.currentTime = v.currentTime; };
v.addEventListener("play", () => { sync(); a.play(); });
v.addEventListener("pause", () => a.pause());
v.addEventListener("seeked", sync);
v.addEventListener("ratechange", () => (a.playbackRate = v.playbackRate));
v.addEventListener("volumechange", () => { a.volume = v.volume; a.muted = v.muted; });
setInterval(() => { if (!v.paused) sync(); }, 1000);
</script>
`,
  );
  return { m4a, wav };
}
