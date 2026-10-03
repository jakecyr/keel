# Evolving arena — native Keel players that rewrite their abilities

Two players compete while background coding workers generate new Keel abilities.
The native Keel server owns the referee, state, worker lifecycle, HTTP routes and
static assets. Generated native Keel functions run every tick. JavaScript only
renders the match. Shell builds and launches; Python is only offline test tooling.

The implemented evolution hook is deliberately concrete: players invent adaptive
**power, reach, armor and speed** functions. They can pulse attack power on firing
ticks, grow armor when hurt, or trade reach for movement. This example does not
implement unrestricted world generation or arbitrary engine hot reloading.

## Run

From the repository root (Rust toolchain, a C compiler, and libcurl development
support are required; see [stdlib setup](../../docs/stdlib.md)):

```sh
./examples/evolving_arena/launch.sh
```

Open **http://127.0.0.1:8780**. The default is visibly labeled **SCRIPTED OFFLINE
DEMO**: deterministic checked-in proposals, no model requests, no credentials
read. It exercises the actual compile/test/activate pipeline; it is not an agent
benchmark. Stop with Ctrl-C. Set `ARENA_PORT=8781` before the command to change
the port. One server instance per example directory is supported.

For real code generation, put literal assignments in this example's ignored
`.env` (preserve any existing file):

```dotenv
OPENAI_API_KEY=your-key
OPENAI_MODEL=gpt-5-mini
```

Then explicitly start the live mode:

```sh
./examples/evolving_arena/launch.sh agents
```

The native worker calls the [OpenAI Responses API](https://developers.openai.com/api/docs/guides/structured-outputs)
with a strict `{name, body, memory}` string schema. `OPENAI_MODEL` is optional;
the default is `gpt-5-mini`. The key is read only by background workers, never
sent to the browser or generated code. No SDK or Python adapter is involved.
The HTTP call has a 60-second deadline, and the host terminates workers after
90 seconds. Errors, incomplete responses and refusals retain the previous code.

Live mode consumes API usage: up to one proposal per player per round, with
at most one outstanding worker per player and a hard cap of 10 worker attempts
per player per launch. Startup launches the first pair.
Rounds advance while a browser polls; a round lasts up to 150 ticks, with a
100ms minimum tick interval. Pausing stops gameplay and cancels in-flight generation workers; queued
validated abilities remain ready. Resuming can start a replacement worker and
counts toward the attempt cap. Closing the page stops ticks, but does not immediately
terminate existing workers. Stopping the server ends its directly owned worker
groups; an already-started compiler/test subprocess in a separate group can
finish afterward under its own deadlines. Pause cancellation has the same
descendant-process limitation. No live model calls
are made by offline tests or CI.

## What evolves

Each player supplies the body of this existing hook:

```keel
fn ability(state: read List<Int>) -> List<Int> {
    if list.at(state, 2) < 15 { return [3, 1, 5, 1] }
    return [4, 3, 1, 2]
}
```

The observation is `[round, tick, ownHp, opponentHp, ManhattanDistance, ownWins,
opponentWins, playerIndex]`. The result is `[power, reach, armor, speed]`:
each value must be 1–5, with a total budget of 10. Both players start at 30 HP.
They approach until within `reach * 6`, then simultaneously attack every third
tick. Damage is `max(1, power - opponentArmor + 1)`. Higher remaining HP wins
when either player reaches zero or tick 150 arrives. A tie awards no point.

Each coding worker receives its own current source, memory, observation,
previous round outcome, and latest validation feedback. Opponent source and
memory are excluded. The next round gives rejected candidates a new opportunity
with the prior feedback; there are no hidden retries within a request. The
checked-in [agent prompt](agent-prompt.txt) describes supported syntax and the
fixed referee. This is code generation plus compiler feedback, not an autonomous
coding CLI with repository access.

## Compile, test, activate

1. The host spawns separate native workers using `process.spawn` while serving
   frames. Workers generate a proposal and write only their inactive source slot.
   `keel edit` replaces exactly the existing ability body against a captured
   revision, preserving its pure signature and rejecting additional declarations.
2. The compiler runs independent budget assertions on both native and reference
   engines, across sampled ticks and health/distance scenarios, then builds a
   native executable. A failed worker publishes feedback and retains old code.
3. Successful candidates stay queued until the next round. The host switches
   slots without overwriting a running ability, resets HP, and starts the next
   generation with separate player memory.
4. Every tick, the host runs each active ability with only its observation-file
   read and stdout grants. The immutable referee validates the actual returned
   stats again. A trap, invalid output or illegal budget disables that ability
   for the rest of the round and restores `[3,3,2,2]`.

The UI shows active source, adaptive stats, HP, scores, compilation feedback and
activation events. Keel's `http.serve_app` serves only `public/`; runtime files,
source files and `.env` are not public assets. The server runtime grants no
network-connect permission; only the worker receives the OpenAI origin grant.

These are experimental runtime permissions and trusted compiler/runtime
boundaries, not an OS sandbox for hostile code. The compiler invoked by workers
is a trusted native tool. Sampled tests do not prove all states or termination.
Individual ability invocations have a 250ms process deadline; a bad
candidate that evades samples is terminated and disabled for the round. Two
invalid players can add roughly 500ms plus process/HTTP overhead to one frame. This is a local demonstration, not an internet-facing service.

## Verify offline

From the repository root:

```sh
cargo build --locked
cargo run --locked -- test examples/evolving_arena --engine both
cargo run --locked -- test examples/evolving_arena/worker-red.json --engine both
python3 -m unittest discover -s examples/evolving_arena -p 'test_*.py' -v
```

The integration tests build isolated native servers/workers, verify static-file
boundaries and pause, run both demo proposals through compilation and sampled
checks, verify activation only at a round boundary and independent player state,
reject extra declarations in a candidate body, and inject an illegal loadout
and an unseen-state infinite loop to verify bounded referee fallback. They do not copy
`.env`, use the API, or weaken acceptance assertions. `ARENA_BUILD_ONLY=1
./examples/evolving_arena/launch.sh` builds all initial binaries without starting
a server or making a model request.

`runtime/` holds ignored build artifacts and transient per-player JSON files.
State resets on launch. To inspect a running match, read `/state`; it reports
active proposal metadata, per-player feedback and a numeric `game` array:

| Indices | Meaning |
| --- | --- |
| 0–6 | tick, red x/y/HP, blue x/y/HP |
| 7–14 | red then blue power/reach/armor/speed |
| 15–18 | round, red wins, blue wins, last monotonic tick time |
| 19–24 | worker handles, active slots, worker start times (red then blue) |
| 25–29 | pending slots + 1, paused, per-player disabled flags |
| 30–31 | damage received this tick, red then blue |
| 32–33 | total worker attempts per player (maximum 10) |
