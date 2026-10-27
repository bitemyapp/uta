// Anonymous research probe, isolated from Uta's account/session storage.
// Run with Deno; see docs/youtube-integration-research.md for the command.
import { Innertube, Log, Platform } from "npm:youtubei.js@18.3.0";

Log.setLevel(Log.Level.NONE);
const output = Deno.args[0];
if (!output) throw new Error("Pass an output JSON path");
const report = { version: "18.3.0", deno: Deno.version.deno, utc: new Date().toISOString(), samples: [] };
let requests = [];
const traceFetch = async (input, init) => {
  const url = new URL(typeof input === "string" || input instanceof URL ? input : input.url);
  const endpoint = url.pathname.includes("/youtubei/v1/") ? url.pathname.split("/").pop()
    : url.pathname.includes("/s/player/") ? "player-js" : "bootstrap-or-other";
  const start = performance.now();
  try {
    return await fetch(input, { ...init, signal: AbortSignal.timeout(20000) });
  } finally {
    requests.push({ endpoint, ms: Math.round(performance.now() - start) });
  }
};

// No network, filesystem, environment, or subprocess permissions in this worker.
// Only the extracted player program and challenge arguments enter it.
Platform.shim.eval = async (data) => {
  const code = `onmessage = (event) => {
    try { postMessage({ result: new Function(event.data)() }); }
    catch { postMessage({ failed: true }); }
  };`;
  const objectURL = URL.createObjectURL(new Blob([code], { type: "text/javascript" }));
  const worker = new Worker(objectURL, { type: "module", deno: { permissions: "none" } });
  try {
    return await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("Evaluator timeout")), 5000);
      worker.onmessage = ({ data }) => {
        clearTimeout(timer);
        if (data.failed) reject(new Error("Evaluator failed"));
        else resolve(data.result);
      };
      worker.onerror = (event) => { event.preventDefault(); clearTimeout(timer); reject(new Error("Worker failed")); };
      worker.postMessage(data.output);
    });
  } finally {
    worker.terminate();
    URL.revokeObjectURL(objectURL);
  }
};

async function run(name, index, action, summarize = () => ({})) {
  requests = [];
  const start = performance.now();
  let value;
  let row;
  try {
    value = await action();
    row = { case: name, round: index, ms: Math.round(performance.now() - start), ok: true, ...summarize(value) };
  } catch (error) {
    row = { case: name, round: index, ms: Math.round(performance.now() - start), ok: false, error_type: error.name,
      no_matching_formats: error.message === "No matching formats found" };
  }
  row.requests = requests;
  report.samples.push(row);
  await Deno.writeTextFile(output, JSON.stringify(report, null, 2) + "\n");
  console.log(JSON.stringify(row));
  return value;
}

const queries = ["Bill Evans Waltz for Debby", "Bach cello suite Yo Yo Ma", "Miles Davis So What",
  "Debussy Clair de Lune", "Ella Fitzgerald Misty"];
const browse = await run("session-no-player", 0, () => Innertube.create({ retrieve_player: false, fetch: traceFetch }));
const videoIDs = [];
if (browse) {
  for (let i = 0; i < queries.length; i++) {
    const result = await run("music-search-first-page", i,
      () => browse.music.search(queries[i], { type: "song" }),
      result => ({ rows: result.songs?.contents.length ?? 0, continuation: result.has_continuation }));
    if (result?.songs?.contents[0]?.id) videoIDs.push(result.songs.contents[0].id);
    if (result?.has_continuation) {
      await run("music-search-continuation", i, () => result.getContinuation(),
        result => ({ rows: result.contents?.contents.length ?? 0 }));
    }
  }
}

const player = await run("session-with-player", 0, () => Innertube.create({ retrieve_player: true, fetch: traceFetch }));
if (player && videoIDs.length) {
  report.public_video_ids = videoIDs;
  for (const mode of ["music", "youtube"]) {
    for (let i = 0; i < 3; i++) {
      const videoID = videoIDs[i % videoIDs.length];
      const info = await run(`${mode}-info`, i,
        () => mode === "music" ? player.music.getInfo(videoID) : player.getBasicInfo(videoID),
        result => ({ playability: result.playability_status?.status,
          adaptive_formats: result.streaming_data?.adaptive_formats?.length ?? 0,
          direct_formats: result.streaming_data?.adaptive_formats?.filter(f => f.url).length ?? 0,
          ciphered_formats: result.streaming_data?.adaptive_formats?.filter(f => f.signature_cipher || f.cipher).length ?? 0 }));
      if (!info) continue;
      const stream = await run(`${mode}-decipher`, i, async () => {
        const format = info.chooseFormat({ type: "audio", quality: "best" });
        return { url: await format.decipher(player.session.player), mime: format.mime_type, bitrate: format.bitrate };
      }, result => ({ mime: result.mime, bitrate: result.bitrate }));
      if (stream) {
        // A small byte-range probe distinguishes a deciphered URL from usable media.
        await run(`${mode}-media-range`, i, async () => {
          const response = await fetch(stream.url, { headers: { Range: "bytes=0-4095" }, signal: AbortSignal.timeout(15000) });
          const reader = response.body?.getReader();
          const first = await reader?.read();
          await reader?.cancel();
          return { status: response.status, content_type: response.headers.get("content-type"),
            first_chunk_bytes: first?.value?.length ?? 0 };
        }, result => result);
        await run(`${mode}-media-2MiB`, i, async () => {
          const response = await fetch(stream.url, { headers: { Range: "bytes=0-2097151" }, signal: AbortSignal.timeout(15000) });
          let bytes = 0;
          const reader = response.body?.getReader();
          while (reader && bytes < 2097152) {
            const part = await reader.read();
            if (part.done) break;
            bytes += part.value.length;
          }
          await reader?.cancel();
          return { status: response.status, bytes, content_type: response.headers.get("content-type") };
        }, result => result);
        await run(`${mode}-mpv`, i, async () => {
          const child = new Deno.Command("python3", { args: ["-c",
            "import json,runpy,sys; p=runpy.run_path('scripts/benchmark-youtube'); print(json.dumps(p['play_probe'](json.load(sys.stdin))))"],
            stdin: "piped", stdout: "piped", stderr: "null" }).spawn();
          const writer = child.stdin.getWriter();
          await writer.write(new TextEncoder().encode(JSON.stringify({ url: stream.url })));
          await writer.close();
          const result = await child.output();
          if (!result.success) throw new Error("Playback probe failed");
          return JSON.parse(new TextDecoder().decode(result.stdout));
        }, result => result);
      }
    }
  }
}
