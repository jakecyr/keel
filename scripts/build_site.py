#!/usr/bin/env python3
"""Build the static Pages site using the recorded benchmark evidence."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
from urllib.parse import quote

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "build" / "site"


def workflow_report(name):
    """Reconcile advertised totals with every registered, sealed trial record."""
    directory = ROOT / "benchmarks/results" / name
    plan = json.loads((directory / "plan.json").read_text())
    summary = json.loads((ROOT / "benchmarks/results" / f"{name}-summary.json").read_text())
    records = [json.loads(p.read_text()) for p in sorted(directory.glob("trial-*/record.json"))]
    for value in [plan, *records]:
        content = {k: v for k, v in value.items() if k != "integrity_sha256"}
        encoded = json.dumps(content, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()
        assert hashlib.sha256(encoded).hexdigest() == value["integrity_sha256"], "Changed workflow evidence"
    assert summary["status"] == "COMPLETE", "Cannot advertise an incomplete workflow study"
    assert summary["plan_sha256"] == plan["integrity_sha256"]
    assert len(records) == len(plan["trials"])
    assert {r["trial_id"] for r in records} == {t["trial_id"] for t in plan["trials"]}
    for variant, group in summary["variants"].items():
        selected = [r for r in records if r["variant"] == variant]
        assert all(r["plan_sha256"] == plan["integrity_sha256"] for r in selected)
        assert group["attempts"] == len(selected)
        assert group["accepted"] == sum(r["accepted"] for r in selected)
        assert group["correct_before_policy"] == sum(r["acceptance"]["passed"] for r in selected)
        assert group["total_tokens_including_failures"] == sum(r["input_tokens"] + r["output_tokens"] for r in selected)
    return summary


def workflow_reduction(summary, candidate):
    base = summary["variants"]["protocol"]["total_tokens_including_failures"]
    improved = summary["variants"][candidate]["total_tokens_including_failures"]
    assert base > 0 and 0 <= improved < base, "No measured token reduction"
    return 100 * (1 - improved / base)


def build(sync=False):
    local = json.loads((ROOT / "benchmarks/results/local.json").read_text())
    pilot = json.loads((ROOT / "benchmarks/results/pilot-trials.gate.json").read_text())
    workflow = workflow_report("efficiency-dev-02")
    validation = workflow_report("efficiency-validation-01")
    reduction = workflow_reduction(workflow, "compact_combined")
    validation_reduction = workflow_reduction(validation, "compact_guided")
    languages = []
    for key, name in (("keel", "Keel"), ("c", "C"), ("rust", "Rust")):
        raw = local["languages"][key]
        entry = {"name": name, "size": raw["minimal_binary_bytes_stripped"]}
        for field, source in (
            ("check", "edited_check_warm_os_cache_fresh_process"),
            ("build", "edited_build_warm_os_cache_fresh_process"),
            ("runtime", "runtime_native"),
        ):
            entry[field] = {p: raw[source][p + "_ms"] for p in ("p50", "p95")}
        languages.append(entry)
    maximum = max(lang["check"]["p50"] for lang in languages)
    rows = []
    for lang in languages:
        value = lang["check"]["p50"]
        css = "bar keel" if lang["name"] == "Keel" else "bar"
        rows.append(
            f'<tr><th scope="row">{lang["name"]}</th><td>'
            f'<div class="bar-track" aria-hidden="true"><div class="{css}" '
            f'style="width:{value / maximum * 100:.3f}%"></div></div>'
            f'</td><td class="chart-value">{value:.2f}</td></tr>'
        )
    pilot_rows = []
    for key, name in (
        ("existing_conventional", "C / conventional tools"),
        ("existing_improved", "C / improved JSON tools"),
        ("keel_text", "Keel / text editing"),
        ("keel_protocol", "Keel / compiler protocol"),
    ):
        condition = pilot["conditions"][key]
        tokens = condition["total_input_tokens"] + condition["total_output_tokens"]
        pilot_rows.append(
            f'<tr><th scope="row">{name}</th><td>{tokens:,}</td>'
            f'<td>{condition["accepted"]}/{condition["attempts"]}</td></tr>'
        )
    workflow_rows = []
    for key, name in (("protocol", "Keel / protocol baseline"),
                      ("full_context", "Keel / full context upfront"),
                      ("compact_context", "Keel / compact context upfront"),
                      ("compact_combined", "Keel / compact context + combined validation")):
        group = workflow["variants"][key]
        workflow_rows.append(f'<tr><th scope="row">{name}</th>'
                             f'<td>{group["total_tokens_including_failures"]:,}</td>'
                             f'<td>{group["accepted"]}/{group["attempts"]}</td></tr>')
    held_out = validation["variants"]["compact_guided"]
    baseline_held_out = validation["variants"]["protocol"]
    banner = (f'> **Measured progress: {reduction:.1f}% fewer agent tokens** with compact context and combined validation '
              '(six-task Keel workflow experiment). '
              f'Reserved-task validation: **{validation_reduction:.1f}% fewer tokens**, '
              f'**{held_out["accepted"]}/{held_out["attempts"]} accepted** versus '
              f'{baseline_held_out["accepted"]}/{baseline_held_out["attempts"]} for the Keel protocol baseline.\n>\n'
              f'> **Local compiler feedback:** CLI checks were **{languages[1]["check"]["p50"] / languages[0]["check"]["p50"]:.1f}× faster than C** '
              f'and **{languages[2]["check"]["p50"] / languages[0]["check"]["p50"]:.1f}× faster than Rust** '
              'on the recorded synthetic ~10,000-line fixture. '
              'A cross-language agent-token or dollar-cost advantage is **not established**. '
              '[Workflow evidence](benchmarks/results/efficiency.md) · [Compiler methodology](benchmarks/results/README.md)')
    readme_path = ROOT / "README.md"
    readme = readme_path.read_text()
    updated_readme, count = re.subn(
        r'(<!-- efficiency-banner:start -->\n).*?(\n<!-- efficiency-banner:end -->)',
        lambda match: match[1] + banner + match[2], readme, flags=re.DOTALL)
    assert count == 1, "Missing or duplicate README metrics banner"
    if sync:
        readme_path.write_text(updated_readme)
    else:
        assert updated_readme == readme, "Stale README metrics: run scripts/build_site.py --sync"
    icon = (ROOT / "docs/assets/keel-icon.svg").read_text()
    decorative_icon = re.sub(r' aria-labelledby="[^"]+"', '', icon)
    decorative_icon = re.sub(r'<(?:title|desc)[^>]*>.*?</(?:title|desc)>', '', decorative_icon)
    decorative_icon = decorative_icon.replace('role="img"', 'aria-hidden="true"')
    html = (ROOT / "site/index.html").read_text()
    for size in (36, 28):
        rendered_icon = decorative_icon.replace(
            'width="128" height="128"', f'width="{size}" height="{size}"'
        )
        html, count = re.subn(
            rf'<svg[^>]*width="{size}" height="{size}".*?</svg>',
            lambda match: rendered_icon.strip(), html, flags=re.DOTALL,
        )
        assert count == 1, f"Missing brand icon at size {size}"
    html = re.sub(
        r'(<link rel="icon" type="image/svg\+xml" href=")[^"]+("\s*>)',
        lambda match: match[1] + "data:image/svg+xml," + quote(icon, safe="") + match[2],
        html,
    )
    replacements = {
        r'(<strong id="workflow-reduction">)[^<]+': lambda match: match[1] + f'{reduction:.1f}%',
        r'(<tbody id="workflow-rows">).*?(</tbody>)': lambda match: match[1] + "\n".join(workflow_rows) + match[2],
        r'(<span id="validation-result">).*?(</span>)': lambda match: match[1] + f'{validation_reduction:.1f}% fewer reported tokens; {held_out["accepted"]}/{held_out["attempts"]} accepted versus {baseline_held_out["accepted"]}/{baseline_held_out["attempts"]} for the Keel protocol baseline.' + match[2],
        r'(<strong id="highlight-check">)[^<]+': lambda match: match[1] + f'{languages[0]["check"]["p50"]:.2f}',
        r'(<strong id="highlight-size">)[^<]+': lambda match: match[1] + f'{languages[0]["size"] / 1024:.1f}',
        r'(<tbody id="chart-rows">).*?(</tbody>)': lambda match: match[1] + "\n".join(rows) + match[2],
        r'(<tbody id="pilot-rows">).*?(</tbody>)': lambda match: match[1] + "\n".join(pilot_rows) + match[2],
        r'(<script id="benchmark-data" type="application/json">).*?(</script>)': lambda match: match[1] + json.dumps({"languages": languages}).replace("<", "\\u003c") + match[2],
    }
    for pattern, replacement in replacements.items():
        html, count = re.subn(pattern, replacement, html, flags=re.DOTALL)
        assert count == 1, f"Missing or duplicate generated region: {pattern}"
    ids = re.findall(r'\bid="([^"]+)"', html)
    assert len(ids) == len(set(ids)), "Duplicate HTML ID"
    for anchor in re.findall(r'href="#([^"]+)"', html):
        assert anchor in ids, f"Missing anchor: {anchor}"
    OUTPUT.mkdir(parents=True, exist_ok=True)
    (OUTPUT / "index.html").write_text(html)
    if sync:
        (ROOT / "site/index.html").write_text(html)
    for name in ("style.css", "app.js"):
        shutil.copyfile(ROOT / "site" / name, OUTPUT / name)
    # Only public site assets belong in the Pages artifact. Example runtime
    # directories and credentials are never part of this copy.
    assets = ROOT / "site" / "assets"
    if assets.is_dir():
        shutil.copytree(assets, OUTPUT / "assets", dirs_exist_ok=True)
    for asset in re.findall(r'(?:src|poster)="(assets/[^\"]+)"', html):
        assert (assets.parent / asset).is_file(), f"Missing public site asset: {asset}"
    (OUTPUT / ".nojekyll").touch()
    print(f"Built {OUTPUT} from recorded benchmarks; anchors verified.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sync", action="store_true", help="Refresh checked-in HTML and README metric snapshots")
    build(parser.parse_args().sync)
