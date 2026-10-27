// Anonymous public-content experiment; third-party package installed outside Uta.
import { readFile, writeFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { spawn } from "node:child_process";
import { loadavg } from "node:os";

const [modulePath, output, experiment] = process.argv.slice(2);
if (!modulePath || !output) throw new Error("Pass package entry point and output JSON path");
if (experiment && experiment !== "omit-next") throw new Error("Unknown experiment");
const importStart = performance.now();
const { Ytdl, Client, Quality, chooseFormat } = await import(pathToFileURL(modulePath).href);
const report = { version: "5.0.2", node: process.version, utc: new Date().toISOString(),
  import_ms: Math.round(performance.now() - importStart), loadavg: loadavg(), experiment, samples: [] };
const { public_video_ids: videoIDs } = JSON.parse(await readFile("docs/research/youtube-benchmark-2026-10-27.json", "utf8"));
report.public_video_ids = videoIDs.slice(0, 3);
let requests = [];
const traceFetch = async (input, init) => {
  const url = new URL(typeof input === "string" || input instanceof URL ? input : input.url);
  const endpoint = url.hostname.endsWith("googlevideo.com") ? "media"
    : url.pathname.includes("/youtubei/v1/") ? url.pathname.split("/").pop()
    : url.pathname.includes("/s/player/") ? "player-js" : "bootstrap-or-other";
  const start = performance.now();
  // Research-only stand-in for an upstream option to omit related metadata.
  // This is not a supported package option or a proposed production interceptor.
  if (experiment === "omit-next" && endpoint === "next") {
    requests.push({ endpoint, ms: 0, stubbed: true });
    return Response.json({});
  }
  try { return await fetch(input, init); }
  finally { requests.push({ endpoint, ms: Math.round(performance.now() - start) }); }
};

async function run(name, index, action, summary = () => ({})) {
  requests = [];
  const start = performance.now();
  let value, row;
  try {
    value = await action();
    row = { case: name, round: index, ms: Math.round(performance.now() - start), ok: true, ...summary(value) };
  } catch (error) {
    row = { case: name, round: index, ms: Math.round(performance.now() - start), ok: false,
      error_type: error.name, code: /^[A-Z_]+$/.test(error.code ?? "") ? error.code : undefined };
  }
  row.requests = requests;
  row.rss_mib = Math.round(process.memoryUsage().rss / 1048576);
  report.samples.push(row);
  await writeFile(output, JSON.stringify(report, null, 2) + "\n");
  console.log(JSON.stringify(row));
  return value;
}

function playback(url) {
  return new Promise((resolve, reject) => {
    const child = spawn("python3", ["-c",
      "import json,runpy,sys; p=runpy.run_path('scripts/benchmark-youtube'); print(json.dumps(p['play_probe'](json.load(sys.stdin))))"],
      { stdio: ["pipe", "pipe", "ignore"] });
    let data = "";
    child.stdout.on("data", chunk => { data += chunk; });
    child.on("error", reject);
    child.stdin.on("error", reject);
    child.on("close", code => {
      if (code !== 0) return reject(new Error("Playback helper failed"));
      try { resolve(JSON.parse(data)); } catch { reject(new Error("Invalid helper response")); }
    });
    child.stdin.end(JSON.stringify({ url }));
  });
}

for (const mode of experiment ? ["default"] : ["default", "mweb"]) {
  const yt = new Ytdl({ fetch: traceFetch, timeoutMs: 15000, ...(mode === "mweb" ? { clients: [Client.Mweb] } : {}) });
  for (let index = 0; index < (mode === "default" ? 3 : 1); index++) {
    const info = await run(`${mode}-info`, index,
      () => yt.getInfo(videoIDs[index], { signal: AbortSignal.timeout(45000) }),
      result => ({ client: result.client, formats: result.formats.length }));
    if (!info) continue;
    const format = await run(`${mode}-choose`, index,
      () => chooseFormat(info.formats, { quality: Quality.HighestAudio, filter: { hasAudio: true, hasVideo: false } }),
      result => ({ itag: result.itag, codec: result.audioCodec, bitrate: result.audioBitrate,
        direct_url: !!result.url, client: result.client }));
    if (!format) continue;
    await run(`${mode}-2MiB`, index, async () => {
      const start = performance.now();
      let bytes = 0, firstByteMs;
      const stream = yt.download(info, { format, range: { start: 0, end: 2097151 }, signal: AbortSignal.timeout(20000) });
      try {
        for await (const part of stream) {
          firstByteMs ??= Math.round(performance.now() - start);
          bytes += part.length;
          if (bytes >= 2097152) break;
        }
      } finally { stream.destroy(); }
      return { bytes, first_byte_ms: firstByteMs, media_ok: bytes >= 2097152 };
    }, result => result);
    if (format.url) await run(`${mode}-mpv`, index, () => playback(format.url), result => result);
  }
}
