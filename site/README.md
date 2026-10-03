# Keel website

A static GitHub Pages site, with no JavaScript dependencies or external fonts.
The homepage can also be opened directly from `site/index.html`.

To build and preview the same artifact deployed by GitHub Actions:

```sh
python3 scripts/build_site.py
python3 -m http.server 4173 --directory build/site
```

Open `http://localhost:4173`. The builder refreshes the homepage's benchmark data
from `benchmarks/results/local.json`, `pilot-trials.gate.json`, and the registered
workflow experiment summaries/records; it checks all
internal anchor targets. The checked-in page retains a snapshot for direct-file
previews. Run `python3 scripts/build_site.py --sync` after changing benchmark evidence to
refresh both the checked-in homepage and README metric banner. Ordinary builds
reject a stale README banner. Workflow totals are reconciled with every sealed
registered trial before being advertised.

The context/edit/check walkthrough is illustrative, not a browser-based Keel
compiler. The chart uses the recorded measurements, including P50/P95 and the
minimal binary sizes. Copy buttons fall back to selecting text when clipboard
access is unavailable. Tabs support arrow keys, Home, and End. Motion respects
`prefers-reduced-motion`.

The Examples section contains local gameplay recordings for the evolving arena
and Jev Pong. Public MP4 videos and JPG posters live in `site/assets/examples/`;
the builder copies `site/assets/` into the Pages artifact and rejects missing
referenced media. Videos use native playback controls, load metadata only,
and do not autoplay. The text beside each recording describes its visual
content and links to the example's run instructions and source. Keep recordings
and descriptions aligned with actual live behavior; offline scripted footage
must be labeled as such. Never copy example `.env` files or runtime directories
into the public assets.

The current silent recordings are 1440×1400. The arena records two live OpenAI
proposals passing sampled checks and activating in native Keel gameplay. Pong
replays a saved live Jev session: 20 consecutive paddle returns, zero misses,
and 20 calls (10 per player) over 74.489 seconds. Replay capture makes no API
calls. These observations describe those recordings, not general success rates
or performance guarantees.

The Pages workflow builds on relevant pull requests and deploys relevant pushes
to `master`. GitHub Pages must use **GitHub Actions** as its publishing source.
It publishes only `build/site`, never the repository root. See GitHub's
[custom workflow documentation](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages).

Keep claims tied to the recorded evidence. Local compiler timings do not prove
agent token savings. The original C-comparison pilot does not establish an agent-cost benefit.
The newer Keel workflow experiments establish reductions only within their small
registered Keel samples. Keep that comparison explicit; do not relabel workflow
improvements as an advantage over C or Rust. Keel remains experimental.
