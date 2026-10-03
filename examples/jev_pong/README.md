# Jev Pong: two independent decision-making players

A local browser game whose HTTP server, game rules, state, process scheduling,
Jev requests, and response parsing are native Keel. JavaScript renders the court,
polls the API, and records a short replay. The shell launcher only builds and
starts the programs. Python is used **only for offline integration tests**.
Keel remains experimental; this is a showcase, not a performance benchmark.

## Play offline

From the repository root:

```sh
./examples/jev_pong/run.sh
```

Open **http://127.0.0.1:8766** and click **Start match**. This default mode uses
simple deterministic tracking bots written in Keel, with no API calls or
credential reads. Each player holds its action between decisions. Use Pause,
the instant replay slider, and Save replay to inspect a rally. Replay browsing
leaves the match running; pause first to stop gameplay or live requests. The
browser keeps its latest 1,800 sampled frames, and downloads them as JSON.

Requirements: Rust/Cargo, a C compiler, libcurl headers and `pkg-config`, and a
modern browser. See [the standard library guide](../../docs/stdlib.md). The
launcher runs from its own directory, so relative file grants are consistent.
It builds the native game and two worker entrypoints, without a global install.

## Play with Jev

```sh
./examples/jev_pong/run.sh jev
```

Each native worker uses `JEV_API_KEY` from its environment when nonempty;
otherwise it reads the existing **`examples/jev/.env`** with Keel's literal
`dotenv.get` parser. Do not copy a key into source or frontend files. The existing
file is never modified. Keys are not included in URLs, command arguments, state,
replays, or diagnostics. Only native decision workers have the file/environment
and TypeSafe network grants. API state contains no credentials.

The match starts paused. Clicking **Start match** in Jev mode sends real requests
to `https://api.typesafe.ai/v1/systemone`, using `jev-latest` and a Choice question
with `up`, `down`, and `stay`. This can consume API usage. By default, each player
may make **30 calls per server session** (60 total), including failed attempts.
There are no retries of a failed request; a later scheduled decision is a new
call. Restarting the server resets this cap and the match. Closing a browser tab
does not cancel an already submitted provider request. Pause terminates local
pending workers; it cannot undo a request already received by the provider.

Configure the cadence and budget in milliseconds using environment values:

```sh
PONG_INTERVAL=900 PONG_TIMEOUT=3000 PONG_TTL=1800 PONG_MAX_DECISIONS=10 \
  ./examples/jev_pong/run.sh jev
```

`PONG_PORT` changes the listening port (default 8766). `PONG_INTERVAL` is the
minimum time between a player's request starts (default 600 ms). Each player
has at most one pending request. `PONG_TIMEOUT` limits its local lifetime
(default 3,000 ms); the HTTP adapter also has its own 10-second transfer bound.
`PONG_TTL` expires an accepted action after 1,500 ms by default. All timing/cap
settings must be positive integers; the port must be a valid TCP port.

Transport errors, missing credentials, non-200 responses, malformed/unsupported
choices, nonzero worker exits, and expired actions fall back to **stay**. No
heuristic bot silently replaces Jev in live mode. The cards show actual choices,
status, requests attempted, and measured time until the server observes worker
completion. This latency includes polling overhead; it is not provider-only
inference time. The UI does not invent explanations for a Choice response.

## Architecture

- `game.keel`: checked integer physics, paddle bounds, wall/paddle collisions,
  scoring, deterministic serves, and the offline controller. Coordinates use a
  1000×600 court; one simulation tick is approximately 1/30 second.
- `main.keel` / `native.keel`: native `http.serve_app` serves `web/` and the
  `/api/state` and `/api/control` endpoints. Match state and the last eight
  decisions **per player** are kept in `build/state.json` using atomic writes.
- `decisions.keel`: builds the Jev request and validates its three legal choices.
  Each player's observation identifies its own/opponent paddle and scores, ball
  position/velocity, held action, decision cadence, and its own bounded history.
- `left_worker/main.keel` and `right_worker/main.keel`: native workers share
  the same implementation, using isolated input/output files. The server spawns,
  polls and terminates them through scoped process grants. Before each spawn it
  clears that player's result, preventing a failed worker from reusing old data.
- `web/`: HTML, CSS and vanilla JavaScript. Rendering interpolates observed
  positions only; all authoritative decisions, collisions and scores are Keel.

The browser polls every 50 ms. On each request the native server advances fixed
33 ms ticks using its monotonic clock, capped at five catch-up ticks. Multiple
viewers share one match. Background-tab throttling or no viewers slows/stops
simulation; this example does not have an autonomous background game loop.
Jev workers run independently while HTTP requests continue serving the game.
The reference engine tests pure behavior; it does not perform live I/O.

The public asset grant is only `web/`. Keys, worker input/output, and state live
outside that root. This is a local development example, not a hosted service or
an OS sandbox for untrusted source.

## Verify without API usage

From the repository root:

```sh
cargo run --locked -- test examples/jev_pong --engine both
cargo run --locked -- lint examples/jev_pong --deny-warnings
cargo run --locked -- fmt examples/jev_pong --check
python3 -m unittest discover -s examples/jev_pong -p 'test_*.py' -v
node --check examples/jev_pong/web/app.js
```

The Keel suite covers collisions, bounds, scoring, independent observations and
histories, JSON escaping and choice decoding. Python tests compile and run the
real native server on temporary loopback ports; temporary mock executables
supply successful, illegal, malformed, failed and delayed decisions without
credentials or external calls. They check continued physics, per-player request
caps, cancellation, fallback, static assets and HTTP controls. A real native
worker is also checked with missing credentials and no network grant. These
are sampled tests, not proof or an agent-efficiency evaluation. Live Jev
latency, strategy quality and billing are not established by these tests.
