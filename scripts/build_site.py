#!/usr/bin/env python3
"""Build the static Pages site using the recorded benchmark evidence."""
import json
from pathlib import Path
import re
import shutil
from urllib.parse import quote

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "build" / "site"


def build():
    local = json.loads((ROOT / "benchmarks/results/local.json").read_text())
    pilot = json.loads((ROOT / "benchmarks/results/pilot-trials.gate.json").read_text())
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
    for name in ("style.css", "app.js"):
        shutil.copyfile(ROOT / "site" / name, OUTPUT / name)
    (OUTPUT / ".nojekyll").touch()
    print(f"Built {OUTPUT} from recorded benchmarks; anchors verified.")


if __name__ == "__main__":
    build()
