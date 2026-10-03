"""Public exploratory tasks plus evaluator-only fixtures; never copy this module to agents.

Validation tasks are reserved from tuning, but public in this repository, not secret.
Expected results and mutation tests are authored independently of agent output.
"""
import json


def task(name, category, requirement, source, public_tests, oracle, correct,
         *, symbol, apis=(), support=None, split="development", kind="code"):
    support = support or {}
    files = {"solution.keel": source, **support, "public_tests.keel": public_tests}
    if kind == "tests":
        files["generated_tests.keel"] = "// Add regression tests here.\n"
    manifest = {"schema": 1, "name": name, "entry": "solution.keel",
                "sources": list(support), "tests": ["public_tests.keel"]}
    if kind == "tests":
        manifest["tests"].append("generated_tests.keel")
    files["keel.json"] = json.dumps(manifest, indent=2) + "\n"
    return {"id": name, "category": category, "split": split, "kind": kind,
            "requirement": requirement, "symbol": symbol, "apis": list(apis),
            "files": files, "editable": ["generated_tests.keel" if kind == "tests" else "solution.keel"],
            "oracle": oracle, "correct": correct}


TASKS = [
    task("boundary", "repair", "Repair is_fresh(now, deadline): true exactly when now < deadline, for every signed 64-bit timestamp. Preserve the interface.",
         "pub fn is_fresh(now: Int, deadline: Int) -> Bool { return now <= deadline }\n",
         'test "public" { assert is_fresh(1, 2) assert !is_fresh(3, 2) }\n',
         'test "boundary oracle" { assert !is_fresh(4, 4) assert is_fresh(-7, -6) assert !is_fresh(-6, -7) assert !is_fresh(9223372036854775807, 9223372036854775807) }\n',
         "pub fn is_fresh(now: Int, deadline: Int) -> Bool { return now < deadline }\n", symbol="is_fresh"),
    task("parse_default", "implementation", "Implement parse_default(raw, fallback): return the signed integer parsed from raw, or fallback for invalid/out-of-range text. Use the documented text.parse_int behavior. Preserve the interface.",
         "pub fn parse_default(raw: read Text, fallback: Int) -> Int { return fallback }\n",
         'test "public" { assert parse_default("bad", 7) == 7 }\n',
         'test "parse oracle" { assert parse_default("42", 3) == 42 assert parse_default("-08", 3) == -8 assert parse_default("", 3) == 3 assert parse_default("+1", 3) == 3 assert parse_default(" 1", 3) == 3 assert parse_default("9223372036854775808", 3) == 3 assert parse_default("-9223372036854775808", 3) == -9223372036854775808 }\n',
         "pub fn parse_default(raw: read Text, fallback: Int) -> Int { match text.parse_int(raw) { Ok(value) => { return value } Err(message) => { return fallback } } }\n",
         symbol="parse_default", apis=("text.parse_int",)),
    task("unique", "collections", "Implement unique(values): return a new list with duplicates removed, retaining first-occurrence order. Leave the borrowed input unchanged. Preserve the interface.",
         "pub fn unique(values: read List<Int>) -> List<Int> { return list.clone(values) }\n",
         'test "public" { assert unique([]) == [] assert unique([1]) == [1] }\n',
         'test "unique oracle" { let input = [3, 1, 3, -2, 1, 0, -2] assert unique(input) == [3, 1, -2, 0] assert input == [3, 1, 3, -2, 1, 0, -2] assert unique([7, 7, 7]) == [7] assert unique([2, 1]) == [2, 1] }\n',
         "pub fn unique(values: read List<Int>) -> List<Int> { var output = [] for value in values { if !list.contains(output, value) { list.push(edit output, value) } } return output }\n",
         symbol="unique", apis=("list.clone", "list.contains", "list.push")),
    task("copy_append", "ownership", "Implement copy_append(values): return a distinct copy of values with its original length appended. Do not mutate or consume the borrowed input. Preserve the interface.",
         "pub fn copy_append(values: read List<Int>) -> List<Int> { return list.clone(values) }\n",
         'test "public" { let input = [4] assert list.len(copy_append(input)) >= 1 assert input == [4] }\n',
         'test "ownership oracle" { var input = [4, 8] var output = copy_append(input) assert output == [4, 8, 2] list.set(edit output, 0, 99) assert input == [4, 8] list.push(edit input, 5) assert output == [99, 8, 2] assert copy_append([]) == [0] }\n',
         "pub fn copy_append(values: read List<Int>) -> List<Int> { let size = list.len(values) var output = list.clone(values) list.push(edit output, size) return output }\n",
         symbol="copy_append", apis=("list.clone", "list.len", "list.push", "list.set")),
    task("checkout", "multiple_files", "Repair checkout(price, quantity, member): compute subtotal with the existing line_total helper, then apply the existing discount helper once to that subtotal. Inputs: 0 <= price <= 10000, 0 <= quantity <= 100. Preserve helpers and interface.",
         "pub fn checkout(price: Int, quantity: Int, member: Bool) -> Int { return line_total(discount(price, member), quantity) }\n",
         'test "public" { assert checkout(100, 2, false) == 200 assert checkout(100, 1, true) == 90 }\n',
         'test "checkout oracle" { assert checkout(9, 2, true) == 17 assert checkout(19, 3, true) == 52 assert checkout(0, 100, true) == 0 assert checkout(10000, 100, false) == 1000000 assert checkout(10000, 100, true) == 900000 }\n',
         "pub fn checkout(price: Int, quantity: Int, member: Bool) -> Int { return discount(line_total(price, quantity), member) }\n",
         symbol="checkout", support={"pricing.keel": "fn line_total(price: Int, quantity: Int) -> Int { return price * quantity }\nfn discount(total: Int, member: Bool) -> Int { if member { return total - total / 10 } return total }\n"}),
    task("clamp_tests", "test_generation", "Write deterministic regression tests for clamp(value, low, high) in generated_tests.keel. Callers guarantee low <= high. Return low below low, high above high, otherwise value, for signed 64-bit integers. Tests must pass correct implementations and detect boundary/branch bugs. Do not change implementation or existing tests; add test blocks only.",
         "pub fn clamp(value: Int, low: Int, high: Int) -> Int { if value < low { return low } if value > high { return high } return value }\n",
         'test "public" { assert clamp(4, 0, 10) == 4 }\n', "",
         'test "regressions" { assert clamp(-1, 0, 10) == 0 assert clamp(11, 0, 10) == 10 assert clamp(4, 0, 10) == 4 assert clamp(-4, -3, -1) == -3 }\n',
         symbol="clamp", kind="tests"),
    task("positive_sum", "collections", "Implement positive_sum(values): sum only strictly positive entries, without changing input. There are at most 100 entries, each between -10000 and 10000. Preserve the interface.",
         "pub fn positive_sum(values: read List<Int>) -> Int { return 0 }\n",
         'test "public" { assert positive_sum([]) == 0 }\n',
         'test "sum oracle" { assert positive_sum([-3, 2, 0, 7, -1]) == 9 assert positive_sum([-1, -2]) == 0 assert positive_sum([1, 1, 1]) == 3 assert positive_sum([10000, -10000, 10000]) == 20000 }\n',
         "pub fn positive_sum(values: read List<Int>) -> Int { var total = 0 for value in values { if value > 0 { total = total + value } } return total }\n",
         symbol="positive_sum", split="validation"),
    task("fresh_tests", "test_generation", "Write deterministic regression tests in generated_tests.keel for is_fresh(now, deadline), true precisely when now < deadline across signed 64-bit timestamps. Tests must pass correct implementations and detect boundary/ordering bugs. Add test blocks only, without changing the implementation or existing tests.",
         "pub fn is_fresh(now: Int, deadline: Int) -> Bool { return now < deadline }\n",
         'test "public" { assert is_fresh(1, 2) }\n', "",
         'test "regressions" { assert !is_fresh(1, 1) assert !is_fresh(2, 1) assert is_fresh(-3, -2) assert !is_fresh(-2, -3) }\n',
         symbol="is_fresh", kind="tests", split="validation"),
]

# Alternative correct implementations discourage source/implementation-specific tests.
for entry in TASKS:
    if entry["id"] == "clamp_tests":
        entry["correct_implementations"] = [entry["files"]["solution.keel"],
            "pub fn clamp(value: Int, low: Int, high: Int) -> Int { if value >= low && value <= high { return value } if value < low { return low } return high }\n"]
        entry["mutants"] = {
            "wrong_lower": entry["files"]["solution.keel"].replace("return low", "return high"),
            "wrong_upper": entry["files"]["solution.keel"].replace("return high", "return low"),
            "always_low": "pub fn clamp(value: Int, low: Int, high: Int) -> Int { return low }\n",
            "zero_lower": entry["files"]["solution.keel"].replace("return low", "return 0"),
        }
    elif entry["id"] == "fresh_tests":
        entry["correct_implementations"] = [entry["files"]["solution.keel"],
            "pub fn is_fresh(now: Int, deadline: Int) -> Bool { return !(now >= deadline) }\n"]
        entry["mutants"] = {"inclusive": entry["files"]["solution.keel"].replace(" < ", " <= "),
                            "reversed": entry["files"]["solution.keel"].replace(" < ", " > "),
                            "always_true": "pub fn is_fresh(now: Int, deadline: Int) -> Bool { return true }\n"}

SUITE = {"schema": 1, "scope": "Public exploratory Keel workflow tasks; no cross-language economic claim", "tasks": TASKS}
