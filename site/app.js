"use strict";
const workflow = {
  context: {
    code: "$ keel agent context . --symbol square --json\n\nSupported language reference\nAvailable commands + built-in API discovery\nCurrent source revision\nsquare: implementation + signature\nDependencies + callers + contracts + holes",
    note: "The CLI supplies the context. Your agent chooses how to use it. Implementation snippets can be bounded; the full response has no total token-budget guarantee.",
  },
  edit: {
    code: '$ keel edit . --request edit.json --json\n\n{\n  "base_revision": "COPY_FROM_CONTEXT",\n  "target": "fn:square",\n  "operation": "replace_body",\n  "source": "{ return value * value }",\n  "run": "affected_checks_and_tests"\n}',
    note: "Use the real revision from context. Stale or invalid edits leave source unchanged. Manifest acceptance files are protected by this tool; ordinary file edits still work.",
  },
  check: {
    code: "$ keel fmt . --check\n$ keel check . --json\n$ keel lint . --json\n$ keel test . --engine both --json\n\nCHECKED  types, ownership, effects\nTESTED   recorded native + reference cases\n\nReached hole? BLOCKED. Timeout? UNKNOWN.",
    note: "The edit request conservatively checks the entire candidate and runs all tests. TESTED is sampled evidence. Runtime contracts are ENFORCED, never formally PROVEN.",
  },
};
function wireTabs(container, onSelect) {
  const tabs = [...container.querySelectorAll('[role="tab"]')];
  const select = (tab) => {
    tabs.forEach((item) => {
      const active = item === tab;
      item.setAttribute("aria-selected", String(active));
      item.tabIndex = active ? 0 : -1;
    });
    onSelect(tab);
  };
  tabs.forEach((tab, index) => {
    tab.addEventListener("click", () => select(tab));
    tab.addEventListener("keydown", (event) => {
      let next;
      if (event.key === "Home") next = 0;
      else if (event.key === "End") next = tabs.length - 1;
      else if (["ArrowRight", "ArrowDown"].includes(event.key))
        next = (index + 1) % tabs.length;
      else if (["ArrowLeft", "ArrowUp"].includes(event.key))
        next = (index - 1 + tabs.length) % tabs.length;
      if (next !== undefined) {
        event.preventDefault();
        select(tabs[next]);
        tabs[next].focus();
      }
    });
  });
}
wireTabs(document.querySelector(".steps"), (tab) => {
  const step = workflow[tab.dataset.step];
  document.querySelector("#workflow-code").textContent = step.code;
  document.querySelector("#workflow-note").textContent = step.note;
  document
    .querySelector("#workflow-panel")
    .setAttribute("aria-labelledby", tab.id);
});
const measurements = JSON.parse(
  document.querySelector("#benchmark-data").textContent,
);
const metricSelect = document.querySelector("#metric");
const metricLabels = {
  check: {
    title: "CLI check",
    note: "Fresh compiler processes with warm OS caches, after a function-body edit. C provides fewer static guarantees; Rust supports a much broader language.",
  },
  build: {
    title: "Edited build",
    note: "A full build after a function-body edit, with warm OS caches and a fresh compiler process. This does not measure declaration-level incremental compilation.",
  },
  runtime: {
    title: "Native execution",
    note: "A bounded integer-recurrence microbenchmark including process startup. The programs check an independently calculated result; this is not general application throughput.",
  },
  size: {
    title: "Minimal stripped executable",
    note: "A minimal program after stripping. This is a single binary-size observation per language, not a typical application size or a repeated timing measurement.",
  },
};
function updateChart() {
  const key = metricSelect.value;
  const size = key === "size";
  const percentile = document.querySelector(
    'input[name="percentile"]:checked',
  ).value;
  const values = measurements.languages.map((lang) =>
    size ? lang.size / 1024 : lang[key][percentile],
  );
  const max = Math.max(...values);
  document.querySelector("#chart-title").textContent = metricLabels[key].title;
  document.querySelector("#chart-description").textContent =
    metricLabels[key].note;
  document.querySelector("#chart-unit").textContent = size
    ? "KiB · lower is smaller"
    : "Milliseconds · lower is better";
  document.querySelector("#value-heading").textContent = size
    ? "Size (KiB)"
    : `${percentile === "p50" ? "Median" : "P95"} (ms)`;
  document.querySelector("#percentiles").disabled = size;
  document.querySelectorAll("#chart-rows tr").forEach((row, index) => {
    row.querySelector(".bar").style.width = `${(values[index] / max) * 100}%`;
    row.querySelector(".chart-value").textContent = values[index].toFixed(
      size ? 1 : 2,
    );
  });
}
metricSelect.addEventListener("change", updateChart);
document
  .querySelectorAll('input[name="percentile"]')
  .forEach((input) => input.addEventListener("change", updateChart));
const downloadInstall = document.querySelector("#install-code").textContent;
const installMethods = {
  macos: {
    code: downloadInstall,
    note: "Requires macOS 15+ and a C compiler. Run `xcode-select --install` if needed. Add the PATH line to ~/.zshrc for new terminals.",
  },
  linux: {
    code: downloadInstall,
    note: "Requires x86_64 with glibc 2.35+ or ARM64 with glibc 2.39+. On Debian/Ubuntu, install a C compiler with `sudo apt-get install build-essential`. Add the PATH line to ~/.bashrc or ~/.zshrc. Alpine/musl is not supported.",
  },
  source: {
    code: 'git clone https://github.com/jakecyr/keel.git\ncd keel\ncargo install --path . --locked\nexport PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"\nkeel doctor',
    note: "Source builds need Rust/Cargo and a C compiler. Add the Cargo PATH line to your shell profile. See the installation guide for supported platforms and reinstalling.",
  },
};
wireTabs(document.querySelector(".install-tabs"), (tab) => {
  const method = installMethods[tab.dataset.install];
  document.querySelector("#install-code").textContent = method.code;
  document.querySelector("#install-note").textContent = method.note;
  document
    .querySelector("#install-panel")
    .setAttribute("aria-labelledby", tab.id);
});
document.querySelectorAll("[data-copy]").forEach((button) =>
  button.addEventListener("click", async () => {
    const code = document.getElementById(button.dataset.copy);
    try {
      await navigator.clipboard.writeText(code.textContent);
      button.textContent = "Copied";
      document.querySelector("#copy-status").textContent =
        `${button.getAttribute("aria-label").replace(/^Copy /, "")} copied.`;
      setTimeout(() => {
        button.textContent = "Copy";
      }, 2000);
    } catch {
      const range = document.createRange();
      range.selectNodeContents(code);
      const selection = window.getSelection();
      selection.removeAllRanges();
      selection.addRange(range);
      document.querySelector("#copy-status").textContent =
        "Clipboard unavailable. The text is selected; use your usual copy shortcut.";
    }
  }),
);
if (
  "IntersectionObserver" in window &&
  !window.matchMedia("(prefers-reduced-motion: reduce)").matches
) {
  const observer = new IntersectionObserver(
    (entries) =>
      entries.forEach((entry) => {
        if (entry.isIntersecting) {
          entry.target.classList.add("reveal");
          observer.unobserve(entry.target);
        }
      }),
    { threshold: 0.12 },
  );
  document
    .querySelectorAll(".section-head,.features article,.example,.setup-step")
    .forEach((element) => observer.observe(element));
}
