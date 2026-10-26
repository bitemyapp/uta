# Performance

## Frame budget

Uta targets Aizome's 4 ms budget for typical interactions. Song rows are
virtualized; network, keyring, audio, and state writes run off the UI thread.

Measure the native view with 200 songs and 120 playback-position updates,
including layout and rasterization of damaged pixels; pass a count to measure
a longer list (library lists hold up to 1,000 songs):

```sh
nix develop --command cargo run --release --example frame_budget
nix develop --command cargo run --release --example frame_budget -- 1000
```

The first 20 frames warm the font and raster caches. The example reports
p50 and p99 latency. Full-window painting, font initialization, and opening a
new view have different costs from steady playback updates; the measurement
does not include network or audio startup latency.

On the development machine (x86_64 Linux, the pinned Nix development shell,
release build, 25 October 2026), this measured **71 µs p50 / 103 µs p99**
with 200 songs and **130 µs p50 / 218 µs p99** with 1,000. Each view copies
the visible song list, so the cost grows with its length; both are far below
4 ms. This is a local measurement, not a guarantee for every GPU, output
scale, font configuration, or machine.

## Starting a song

Measure how long songs take to be heard, against public YouTube without an
account, at volume 0:

```sh
nix develop --command cargo run --release --example start_latency
```

The example times two kinds of start: a song chosen from a list, which is
looked up when it's chosen, and the next song in the queue, which is looked up
while the one before it plays.

On the development machine (26 October 2026, load average about 12), a song
chosen from a list was heard after **2.5–3.6 s**. That is one to three seconds
of YouTube lookup, then about 0.35 s for mpv. The next song in the queue was
heard after **0.45 s**, with an occasional second under load. mpv already
plays the audio as it arrives, so the lookup is the wait.
