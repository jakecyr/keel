# Keel workflow efficiency experiments

These are exploratory **Keel-versus-Keel workflow** measurements, not a new
cross-language economic result. The [original pilot](README.md) remains unchanged.
All live trials below used the pinned `gpt-6-astra` argument, 180 seconds and
32,000 reported input + output tokens per trial. Cached input is included once;
failed and over-budget attempts remain in totals. Actual dollar costs and enforced
condition isolation remain UNKNOWN.

## Development round 01: find avoidable work

Two tasks, one repetition, two variants (four live trials):

| Workflow | Reported tokens | Correct before budget policy | Accepted |
| --- | ---: | ---: | ---: |
| Protocol | 182,261 | 2/2 | 0/2 |
| Compact context + combined validation | 97,413 | 2/2 | 1/2 |

The compact combined workflow used 46.6% fewer total reported tokens on this sample.
Both collection attempts still bound a local named `result`, received the reserved-name
diagnostic, and retried. This prompted a shared primer/documentation clarification,
not a change to language semantics or acceptance assertions. See [issue #2](https://github.com/jakecyr/keel/issues/2).

[Registered plan](efficiency-dev-01-plan.json) · [records and frozen harness](efficiency-dev-01/)

## Development round 02: compare context and workflow variants

Six task families, one repetition, four variants (24 live trials):

| Workflow | Reported tokens | Tool calls | Correct before budget policy | Accepted |
| --- | ---: | ---: | ---: | ---: |
| Protocol | 491,897 | 32 | 6/6 | 0/6 |
| Full context upfront | 347,643 | 14 | 6/6 | 0/6 |
| Compact context upfront | 328,051 | 16 | 6/6 | 0/6 |
| Compact context + combined validation | 241,915 | 9 | 6/6 | 3/6 |

The combined workflow used **50.8% fewer total reported tokens** than protocol on
this development sample. It met the budget for boundary repair, ownership/copy
independence, and the change across source files. Collection implementation, parsing,
and test generation remained over budget. All 24 artifacts passed independent
acceptance. Each generated test suite passed two correct implementations and killed
all four registered clamp mutants with assertion failures in both engines.

There were no failed commands in this round; the reserved `result` mistake did not
recur. Agents still sometimes fetched a full reference after receiving compact API
signatures, motivating a separately registered syntax-guidance experiment.

**Tokens per accepted change cannot be compared against the baseline:** its
acceptance denominator is zero. The 50.8% figure concerns total reported tokens,
not cost per accepted change, a release gate, or general agent efficiency.

[Registered plan](efficiency-dev-02-plan.json) · [records and frozen harness](efficiency-dev-02/)

## Registration guard

[Development plan 03](efficiency-dev-03-plan.json) was not executed: a concurrent
compiler rebuild changed its registered binary hash. The runner refused to launch
any agent, so there are no attempts or inference usage for this plan. Plan 04 pins a
separate local compiler copy instead. Each executed run freezes its binary internally;
compiler hashes differ between some rounds, so do not treat cross-round differences
as a controlled estimate of a single prompt change.

## Development round 04: supply exact collection syntax

Two collection/ownership tasks, two repetitions, two variants (eight live trials):

| Workflow | Reported tokens | Tool calls | Correct before budget policy | Accepted |
| --- | ---: | ---: | ---: | ---: |
| Compact context + combined validation | 179,852 | 7 | 4/4 | 1/4 |
| Same workflow + focused syntax guidance | 126,093 | 4 | 4/4 | 4/4 |

Adding a short loop/borrowing example increased the mean prompt from 2,995 to
3,380 bytes but reduced total reported tokens by **29.9%**. All guided trials
finished with one tool call; three of four unguided trials retrieved extra reference
material. Total tokens per accepted change were 179,852 versus 31,523.25, including
failed budget attempts. That acceptance-adjusted reduction is 82.5% for this small
registered sample, not a dollar-cost claim. There were no paired acceptance regressions.
The report therefore selected the guided workflow as a candidate for reserved-task
validation. No task assertions, expected outputs or budgets were changed.

[Registered plan](efficiency-dev-04-plan.json) · [records and frozen harness](efficiency-dev-04/)

## Reserved-task validation: frozen workflow, different tasks

Two reserved tasks (positive-only list summation and freshness regression tests),
two repetitions, three variants (12 live trials). The candidate was frozen before
these model runs, and no tuning followed the validation results.

| Workflow | Reported tokens | Tool calls | Correct before budget policy | Accepted |
| --- | ---: | ---: | ---: | ---: |
| Protocol | 335,357 | 21 | 4/4 | 0/4 |
| Compact context + combined validation | 163,238 | 7 | 4/4 | 0/4 |
| Same workflow + focused syntax guidance | 128,009 | 5 | 4/4 | 2/4 |

The guided workflow used **61.8% fewer total reported tokens** than protocol and
21.6% fewer than compact combined on this reserved sample. Both summation trials
met the budget. Generated regression tests detected every mutant and passed both
correct implementations, but still exceeded the 32,000-token cap: 32,828 and
32,695 tokens. The budget was not relaxed. The naming/collection guidance does not
apply to the scalar test-generation task, so differences between those two compact
variants on that task are noise, not evidence for that guidance.

All 12 results passed independent correctness checks. There was no correctness
regression, but a cost-per-accepted-change comparison against protocol is still
undefined because the baseline accepted zero trials. This validates a directional
workflow improvement on two public reserved tasks, not superiority over another
language, a broad population result, or an economic release claim.

[Registered plan](efficiency-validation-01-plan.json) · [summary](efficiency-validation-01-summary.json) · [records and frozen harness](efficiency-validation-01/)

Across the four executed rounds, all **48 live trials** and their reported usage
are retained, including every over-budget attempt. The unexecuted plan 03 consumed
no inference. The original 12-trial cross-language pilot is retained separately.

## Reproduce and extend

Use [the experiment instructions](../README.md#iterate-on-keel-agent-efficiency).
Reports and trial records are immutable. Every executed run stores its exact prompts,
raw events, source artifacts, independent oracle evidence, compiler hash, environment,
and harness source snapshot. The snapshot captures the scorer used at execution;
a later hardening change requires a new plan, not rewritten historical evidence.

The current scorer requires the exact `assertion_failure` diagnostic in both engines
for mutation credit. Round 02 was also inspected against this stricter condition:
all recorded mutant kills were actual assertion failures. Existing evidence was
preserved unchanged.

These are public small tasks in a mutable development working tree. The model backend
identity and all inherited provider defaults are not independently attested. Prompt
bytes and tool-output bytes are not token measurements, and the CLI does not expose
per-inference token attribution. Token caps are applied after completion. Broader,
repeated, independently held-out comparisons with a strong C baseline, enforced tool
conditions and billing evidence are still needed before claiming an economic advantage.
