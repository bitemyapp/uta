# Frame budget

Uta targets Aizome's 4 ms budget for typical interactions. Song rows are
virtualized; network, keyring, audio, and state writes run off the UI thread.

Measure the native view with 200 songs and 120 playback-position updates,
including layout and rasterization of damaged pixels:

```sh
nix develop --command cargo run --release --example frame_budget
```

The first 20 frames warm the font and raster caches. The example reports
p50 and p99 latency. Full-window painting, font initialization, and opening a
new view have different costs from steady playback updates; the measurement
does not include network or audio startup latency.

On the development machine (x86_64 Linux, the pinned Nix development shell,
release build, 25 October 2026), this measured **66 µs p50 / 226 µs p99**.
Both are below 4 ms. This is a local measurement, not a guarantee for every
GPU, output scale, font configuration, or machine.
