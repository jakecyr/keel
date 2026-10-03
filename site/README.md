# Keel website

A static GitHub Pages site, with no JavaScript dependencies or external fonts.
The homepage can also be opened directly from `site/index.html`.

To build and preview the same artifact deployed by GitHub Actions:

```sh
python3 scripts/build_site.py
python3 -m http.server 4173 --directory build/site
```

Open `http://localhost:4173`. The builder refreshes the homepage's benchmark data
from `benchmarks/results/local.json` and `pilot-trials.gate.json`; it checks all
internal anchor targets. The checked-in page retains a snapshot for direct-file
previews. Keep that snapshot aligned by copying `build/site/index.html` back to
`site/index.html` after changing benchmark evidence.

The context/edit/check walkthrough is illustrative, not a browser-based Keel
compiler. The chart uses the recorded measurements, including P50/P95 and the
minimal binary sizes. Copy buttons fall back to selecting text when clipboard
access is unavailable. Tabs support arrow keys, Home, and End. Motion respects
`prefers-reduced-motion`.

The Pages workflow builds on relevant pull requests and deploys relevant pushes
to `master`. GitHub Pages must use **GitHub Actions** as its publishing source.
It publishes only `build/site`, never the repository root. See GitHub's
[custom workflow documentation](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages).

Keep claims tied to the recorded evidence. Local compiler timings do not prove
agent token savings. The current pilot does not establish an agent-cost benefit,
and Keel remains experimental and feature-incomplete.
