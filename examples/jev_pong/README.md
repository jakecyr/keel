# Jev Pong: Jev selects the target, Keel moves the paddle

A local browser game whose HTTP server, physics, state, process scheduling,
Jev requests and response parsing are native Keel. JavaScript renders the court,
polls the API and records a short replay. The shell launcher only builds and
starts programs. Python is used **only for offline integration tests**.
Keel remains experimental; this showcase is not a performance benchmark.

## Run

From the repository root:

```sh
./examples/jev_pong/run.sh offline
```

Open **http://127.0.0.1:8766**, then click **Start match**. Offline mode selects
interception targets with a deterministic geometry controller: no API calls or
credential reads. Requirements: Rust/Cargo, a C compiler, libcurl headers and
`pkg-config`, and a modern browser. See [the standard library](../../docs/stdlib.md).
The launcher builds the native server and two workers without a global install.

For real Jev decisions:

```sh
PONG_MAX_DECISIONS=12 PONG_MAX_RETURNS=20 ./examples/jev_pong/run.sh jev
```

Each native worker uses nonempty `JEV_API_KEY` from its environment, otherwise
reads **`examples/jev/.env`** with Keel's literal `dotenv.get` parser. The existing
file is never modified. Keys stay out of URLs, command arguments, frontend,
state, replays and diagnostics. Only decision workers receive credential and
TypeSafe network grants.

The match starts paused. Clicking Start in Jev mode makes real requests to
`https://api.typesafe.ai/v1/systemone` using `jev-latest`, which can consume API
usage. The default caps are **12 calls per player** and **20 individual paddle
returns** (stricter than 20 complete back-and-forth round trips). Failed calls
count. At the return limit, or when the next incoming flight would exceed a
player's request budget, the server pauses, cancels pending workers and refuses
further Start requests. Restarting the server begins a new budget and match.

`PONG_PORT` changes the port (default 8766). `PONG_TIMEOUT` limits each pending
worker's lifetime (default 3,000 ms); the HTTP adapter also has a ten-second transfer
bound. Settings must be positive integers; the port must be valid. There are no
automatic retries for a failed flight. Pause cancels local workers, but cannot
undo a request already received by the provider. Closing a tab does not cancel
an already submitted provider request.

## Target decisions

Each player gets **one Jev choice per incoming ball flight**, not repeated
up/down requests on a timer. Jev chooses one of eleven paddle-center targets:
55, 104, 153, 202, 251, 300, 349, 398, 447, 496, 545. A native actuator moves toward that
chosen coordinate at nine units per tick and stops within four units; the
selected target stays fixed for the flight. This avoids holding a directional
command long enough to overshoot the ball.

The observation identifies own/opponent paddles and scores, ball position and
velocity, own decision history, legal targets, ticks until impact, and
**predicted_impact_y**. Keel computes that prediction from straight-line geometry
and wall reflections. It is explicitly supplied sensor information; Jev selects
the target. The UI labels this division of work and does not invent model
reasoning. Offline mode is separately labeled and uses no model.

Missing keys, transport errors, non-200 responses, malformed or illegal choices,
nonzero worker exits and timeouts hold the paddle at its current position.
There is no heuristic replacement for a failed live decision. Player cards show
selected targets, motor direction, bounded histories and observed request
latency. Latency includes the server's completion polling overhead, not only
provider inference time. Counters show total returns, current rally and longest
consecutive rally; a score resets the current rally.

## Implementation and replay

- `game.keel`: checked integer physics, collisions/scoring, reflected-impact
  geometry and the target actuator. Coordinates use a 1000×600 court.
- `main.keel` / `native.keel`: native `http.serve_app` serves `web/` and
  `/api/state` / `/api/control`; atomic `build/state.json` stores the match and
  last eight decisions per player. Separate workers are spawned, polled and
  terminated with scoped process grants.
- `decisions.keel`: Jev request construction and legal target decoding.
- `left_worker/main.keel` and `right_worker/main.keel`: native worker entrypoints
  with isolated input/output files. Results are cleared before each spawn, so
  a failed worker cannot apply an old decision.
- `web/`: vanilla HTML/CSS/JavaScript. Rendering interpolates observed positions;
  authoritative physics, scores, counters and decisions stay in Keel.

The browser polls every 50 ms. Requests advance fixed 33 ms simulation ticks from
the monotonic clock, capped at five catch-up ticks. Multiple viewers share a
match. Background-tab throttling or absent viewers slows/stops simulation; this
example does not run an autonomous background simulation. Jev workers continue
independently while requests serve the game.

Use Pause before replay browsing if you want to stop gameplay and live calls.
The slider otherwise leaves the match running. The browser retains 1,800 sampled
frames and Save replay downloads JSON with selected targets and counters.
Only `web/` is exposed as static content. This local development example is not
a hosted service or an OS sandbox for untrusted source.

## Offline verification

```sh
cargo run --locked -- test examples/jev_pong --engine both
cargo run --locked -- lint examples/jev_pong --deny-warnings
cargo run --locked -- fmt examples/jev_pong --check
python3 -m unittest discover -s examples/jev_pong -p 'test_*.py' -v
node --check examples/jev_pong/web/app.js
```

Pure tests cover collisions, bounds, scoring, independent observations/histories,
JSON escaping, target decoding, reflected geometry, actuator stopping, rally
counters and a sampled sustained offline rally. Python tests compile the real
native server and use temporary mock workers for success, malformed/illegal
choices, failure and delay; they verify physics continues, request and return
limits pause the session, and pause cancels workers. A native worker is also
checked against an isolated empty-key fixture with no network grant. Tests do
not access repository credentials or make external calls. Passing tests is
sampled evidence, not proof or an agent-efficiency evaluation.
