//! Independent, bounded reference semantics. Never invokes a native backend or host effect.
use crate::syntax::{Expr, ExprKind, Mode, Program, Stmt, StmtKind};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq)]
enum Val {
    Int(i64),
    Bool(bool),
    Text(String),
    List(Vec<i64>),
    Option(Option<i64>),
    Result(Result<i64, String>),
    Unit,
}
type Cell = Rc<RefCell<Val>>;
type Env = BTreeMap<String, Cell>;
type EResult<T> = Result<T, Fault>;
#[derive(Debug)]
struct Fault {
    kind: &'static str,
    at: usize,
    state: &'static str,
}
impl Fault {
    fn failed(kind: &'static str, at: usize) -> Self {
        Self {
            kind,
            at,
            state: "FAILED",
        }
    }
    fn limit(kind: &'static str, at: usize) -> Self {
        Self {
            kind,
            at,
            state: "UNKNOWN",
        }
    }
    fn json(&self, value: Option<i64>) -> Value {
        json!({"kind":self.kind,"offset":self.at,"has_value":value.is_some(),"value":value.unwrap_or(0),"status":self.state})
    }
}
impl Val {
    fn integer(&self) -> EResult<i64> {
        match self {
            Self::Int(v) => Ok(*v),
            _ => Err(Fault::failed("reference_type_error", 0)),
        }
    }
    fn boolean(&self) -> EResult<bool> {
        match self {
            Self::Bool(v) => Ok(*v),
            _ => Err(Fault::failed("reference_type_error", 0)),
        }
    }
    fn text(&self) -> EResult<&str> {
        match self {
            Self::Text(v) => Ok(v),
            _ => Err(Fault::failed("reference_type_error", 0)),
        }
    }
    fn list(&self) -> EResult<&[i64]> {
        match self {
            Self::List(v) => Ok(v),
            _ => Err(Fault::failed("reference_type_error", 0)),
        }
    }
    fn bytes(&self) -> usize {
        match self {
            Self::Text(v) | Self::Result(Err(v)) => v.len(),
            Self::List(v) => v.len().saturating_mul(8),
            _ => 0,
        }
    }
}
fn cell(value: Val) -> Cell {
    Rc::new(RefCell::new(value))
}
struct Evaluator<'a> {
    program: &'a Program,
    deadline: Instant,
    steps: u64,
    depth: usize,
    expression_depth: usize,
    allocated: usize,
}
impl Evaluator<'_> {
    fn tick(&mut self, at: usize) -> EResult<()> {
        self.steps += 1;
        if self.steps > 1_000_000 {
            return Err(Fault::limit("reference_step_limit", at));
        }
        if Instant::now() >= self.deadline {
            return Err(Fault::limit("execution_limit", at));
        }
        Ok(())
    }
    fn account(&mut self, bytes: usize) -> EResult<()> {
        self.allocated = self.allocated.saturating_add(bytes);
        if self.allocated > 32 * 1024 * 1024 {
            return Err(Fault::limit("reference_allocation_limit", 0));
        }
        Ok(())
    }
    fn snapshot(&mut self, value: &Val) -> EResult<Val> {
        self.account(value.bytes())?;
        Ok(value.clone())
    }
    fn binding(env: &Env, name: &str, at: usize) -> EResult<Cell> {
        env.get(name)
            .cloned()
            .ok_or_else(|| Fault::failed("reference_unknown_binding", at))
    }
    fn expr(&mut self, expr: &Expr, env: &mut Env) -> EResult<Val> {
        if self.expression_depth >= 128 {
            return Err(Fault::limit("reference_recursion_limit", expr.at));
        }
        self.expression_depth += 1;
        let result = self.expr_inner(expr, env);
        self.expression_depth -= 1;
        result
    }
    fn expr_inner(&mut self, expr: &Expr, env: &mut Env) -> EResult<Val> {
        self.tick(expr.at)?;
        let at = expr.at;
        let result = match &expr.kind {
            ExprKind::Int(v) => Val::Int(*v),
            ExprKind::Bool(v) => Val::Bool(*v),
            ExprKind::Text(v) => {
                self.account(v.len())?;
                Val::Text(v.clone())
            }
            ExprKind::Var(name) | ExprKind::Edit(name) => {
                return self.snapshot(&Self::binding(env, name, at)?.borrow());
            }
            ExprKind::Move(name) => {
                std::mem::replace(&mut *Self::binding(env, name, at)?.borrow_mut(), Val::Unit)
            }
            ExprKind::List(values) => {
                let mut list = Vec::new();
                self.account(values.len().saturating_mul(8))?;
                for value in values {
                    list.push(self.expr(value, env)?.integer()?);
                }
                Val::List(list)
            }
            ExprKind::Unary(op, value) => {
                let value = self.expr(value, env)?;
                match op.as_str() {
                    "!" => Val::Bool(!value.boolean()?),
                    "-" => Val::Int(
                        value
                            .integer()?
                            .checked_neg()
                            .ok_or_else(|| Fault::failed("overflow", at))?,
                    ),
                    _ => return Err(Fault::failed("reference_operator", at)),
                }
            }
            ExprKind::Binary(op, left, right) => {
                let left = self.expr(left, env)?;
                if op == "&&" && !left.boolean()? {
                    return Ok(Val::Bool(false));
                }
                if op == "||" && left.boolean()? {
                    return Ok(Val::Bool(true));
                }
                let right = self.expr(right, env)?;
                match op.as_str() {
                    "==" => Val::Bool(left == right),
                    "!=" => Val::Bool(left != right),
                    "&&" | "||" => Val::Bool(right.boolean()?),
                    "<" => Val::Bool(left.integer()? < right.integer()?),
                    "<=" => Val::Bool(left.integer()? <= right.integer()?),
                    ">" => Val::Bool(left.integer()? > right.integer()?),
                    ">=" => Val::Bool(left.integer()? >= right.integer()?),
                    "+" | "-" | "*" | "/" | "%" => {
                        let (a, b) = (left.integer()?, right.integer()?);
                        if (op == "/" || op == "%") && b == 0 {
                            return Err(Fault::failed("division_by_zero", at));
                        }
                        let value = match op.as_str() {
                            "+" => a.checked_add(b),
                            "-" => a.checked_sub(b),
                            "*" => a.checked_mul(b),
                            "/" => a.checked_div(b),
                            _ => a.checked_rem(b),
                        };
                        Val::Int(value.ok_or_else(|| Fault::failed("overflow", at))?)
                    }
                    _ => return Err(Fault::failed("reference_operator", at)),
                }
            }
            ExprKind::Call(name, args) => return self.call(name, args, env, at),
        };
        Ok(result)
    }
    fn block(&mut self, body: &[Stmt], env: &mut Env) -> EResult<Option<Val>> {
        let outer: BTreeSet<_> = env.keys().cloned().collect();
        let result = self.statements(body, env);
        env.retain(|name, _| outer.contains(name));
        result
    }
    fn statements(&mut self, body: &[Stmt], env: &mut Env) -> EResult<Option<Val>> {
        for stmt in body {
            self.tick(stmt.at)?;
            let returned = match &stmt.kind {
                StmtKind::Bind { name, value, .. } => {
                    let value = self.expr(value, env)?;
                    env.insert(name.clone(), cell(value));
                    None
                }
                StmtKind::Assign(name, value) => {
                    let value = self.expr(value, env)?;
                    *Self::binding(env, name, stmt.at)?.borrow_mut() = value;
                    None
                }
                StmtKind::Return(value) => {
                    return Ok(Some(match value {
                        Some(v) => self.expr(v, env)?,
                        None => Val::Unit,
                    }));
                }
                StmtKind::Assert(value) => {
                    if !self.expr(value, env)?.boolean()? {
                        return Err(Fault::failed("assertion_failure", value.at));
                    }
                    None
                }
                StmtKind::Expr(value) => {
                    self.expr(value, env)?;
                    None
                }
                StmtKind::If(condition, yes, no) => {
                    let condition = self.expr(condition, env)?.boolean()?;
                    self.block(if condition { yes } else { no }, env)?
                }
                StmtKind::While(condition, body) => {
                    let mut returned = None;
                    while self.expr(condition, env)?.boolean()? {
                        returned = self.block(body, env)?;
                        if returned.is_some() {
                            break;
                        }
                    }
                    returned
                }
                StmtKind::For(name, values, body) => {
                    let values = self.expr(values, env)?;
                    let mut returned = None;
                    for value in values.list()? {
                        env.insert(name.clone(), cell(Val::Int(*value)));
                        returned = self.block(body, env)?;
                        env.remove(name);
                        if returned.is_some() {
                            break;
                        }
                    }
                    returned
                }
                StmtKind::Match(value, arms) => {
                    let value = self.expr(value, env)?;
                    let (variant, payload) = match value {
                        Val::Option(Some(v)) => ("Some", Val::Int(v)),
                        Val::Option(None) => ("None", Val::Unit),
                        Val::Result(Ok(v)) => ("Ok", Val::Int(v)),
                        Val::Result(Err(v)) => ("Err", Val::Text(v)),
                        _ => return Err(Fault::failed("reference_type_error", stmt.at)),
                    };
                    let arm = arms
                        .iter()
                        .find(|a| a.variant == variant)
                        .ok_or_else(|| Fault::failed("reference_nonexhaustive", stmt.at))?;
                    if let Some(name) = &arm.binding {
                        env.insert(name.clone(), cell(payload));
                    }
                    let returned = self.block(&arm.body, env)?;
                    if let Some(name) = &arm.binding {
                        env.remove(name);
                    }
                    returned
                }
            };
            if returned.is_some() {
                return Ok(returned);
            }
        }
        Ok(None)
    }
    fn call(&mut self, name: &str, args: &[Expr], env: &mut Env, at: usize) -> EResult<Val> {
        if name == "hole" {
            return Err(Fault {
                kind: "hole_reached",
                at,
                state: "BLOCKED",
            });
        }
        if name == "http.serve" || name == "io.println" {
            if name == "io.println" {
                for arg in args {
                    self.expr(arg, env)?;
                }
            } else if let Some(port) = args.first()
                && !(1..=65535).contains(&self.expr(port, env)?.integer()?)
            {
                return Err(Fault::failed("invalid_port", 0));
            }
            return Err(Fault {
                kind: if name == "http.serve" {
                    "permission_denied_net"
                } else {
                    "permission_denied_stdout"
                },
                at: 0,
                state: "BLOCKED",
            });
        }
        if let Some(function) = self.program.functions.iter().find(|f| f.name == name) {
            if self.depth >= 64 {
                return Err(Fault::limit("reference_recursion_limit", at));
            }
            let mut locals = Env::new();
            for (arg, param) in args.iter().zip(&function.params) {
                let binding = if matches!(param.mode, Mode::Read | Mode::Edit)
                    && let ExprKind::Var(name) | ExprKind::Edit(name) = &arg.kind
                {
                    Self::binding(env, name, arg.at)?
                } else {
                    cell(self.expr(arg, env)?)
                };
                locals.insert(param.name.clone(), binding);
            }
            self.depth += 1;
            let result = (|| {
                for condition in &function.requires {
                    if !self.expr(condition, &mut locals)?.boolean()? {
                        return Err(Fault::failed("precondition_failure", condition.at));
                    }
                }
                let value = self
                    .block(&function.body, &mut locals)?
                    .unwrap_or(Val::Unit);
                locals.insert("result".into(), cell(self.snapshot(&value)?));
                for condition in &function.ensures {
                    if !self.expr(condition, &mut locals)?.boolean()? {
                        return Err(Fault::failed("postcondition_failure", condition.at));
                    }
                }
                Ok(value)
            })();
            self.depth -= 1;
            return result;
        }
        if name == "list.push" || name == "list.set" {
            let Some(Expr {
                kind: ExprKind::Edit(name),
                ..
            }) = args.first()
            else {
                return Err(Fault::failed("reference_edit_required", at));
            };
            let target = Self::binding(env, name, at)?;
            let a = self.expr(&args[1], env)?.integer()?;
            let b = if args.len() > 2 {
                Some(self.expr(&args[2], env)?.integer()?)
            } else {
                None
            };
            let mut target = target.borrow_mut();
            let Val::List(values) = &mut *target else {
                return Err(Fault::failed("reference_type_error", at));
            };
            if let Some(value) = b {
                let index = usize::try_from(a)
                    .ok()
                    .filter(|i| *i < values.len())
                    .ok_or_else(|| Fault::failed("bounds", at))?;
                values[index] = value;
            } else {
                self.account(8)?;
                values.push(a);
            }
            return Ok(Val::Unit);
        }
        let mut values = Vec::new();
        for arg in args {
            values.push(self.expr(arg, env)?);
        }
        let value = match name {
            "text.clone" | "list.clone" => return self.snapshot(&values[0]),
            "text.concat" => {
                let a = values[0].text()?;
                let b = values[1].text()?;
                self.account(a.len().saturating_add(b.len()))?;
                Val::Text(format!("{a}{b}"))
            }
            "text.len" => Val::Int(values[0].text()?.len() as i64),
            "text.from_int" => Val::Text(values[0].integer()?.to_string()),
            "text.parse_int" => Val::Result(parse_integer(values[0].text()?)),
            "list.new" => Val::List(Vec::new()),
            "list.len" => Val::Int(values[0].list()?.len() as i64),
            "list.contains" => Val::Bool(values[0].list()?.contains(&values[1].integer()?)),
            "list.at" | "list.get" => {
                let index = values[1].integer()?;
                let result = usize::try_from(index)
                    .ok()
                    .and_then(|i| values[0].list().ok()?.get(i).copied());
                if name == "list.at" {
                    Val::Int(result.ok_or_else(|| Fault::failed("bounds", at))?)
                } else {
                    Val::Option(result)
                }
            }
            "option.some" => Val::Option(Some(values[0].integer()?)),
            "option.none" => Val::Option(None),
            "result.ok" => Val::Result(Ok(values[0].integer()?)),
            "result.err" => Val::Result(Err(values[0].text()?.into())),
            "http.response" => {
                let status = values[0].integer()?;
                let body = values[1].text()?;
                if !(100..=599).contains(&status) {
                    return Err(Fault::failed("invalid_http_status", 0));
                }
                let reason = match status {
                    200 => "OK",
                    404 => "Not Found",
                    405 => "Method Not Allowed",
                    _ => "Response",
                };
                self.account(body.len().saturating_add(256))?;
                Val::Text(format!(
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                ))
            }
            "http.status" => {
                let text = values[0].text()?.as_bytes();
                Val::Int(
                    if text.len() >= 12
                        && text.starts_with(b"HTTP/1.1 ")
                        && text[9..12].iter().all(u8::is_ascii_digit)
                    {
                        text[9..12]
                            .iter()
                            .fold(0, |n, c| n * 10 + i64::from(c - b'0'))
                    } else {
                        0
                    },
                )
            }
            "http.body" => Val::Text(
                values[0]
                    .text()?
                    .split_once("\r\n\r\n")
                    .map_or("", |(_, body)| body)
                    .into(),
            ),
            _ => {
                return Err(Fault {
                    kind: "reference_unsupported_function",
                    at,
                    state: "BLOCKED",
                });
            }
        };
        self.account(value.bytes())?;
        Ok(value)
    }
}

fn parse_integer(text: &str) -> Result<i64, String> {
    if text.is_empty() {
        return Err("empty integer".into());
    }
    let digits = text.strip_prefix('-').unwrap_or(text);
    if digits.is_empty() {
        return Err("invalid integer".into());
    }
    // Validate incrementally so an overflow before a later invalid character agrees with the language.
    let mut magnitude = 0_i128;
    let limit = if text.starts_with('-') {
        i128::from(i64::MAX) + 1
    } else {
        i128::from(i64::MAX)
    };
    for digit in digits.bytes() {
        if !digit.is_ascii_digit() {
            return Err("invalid integer".into());
        }
        magnitude = magnitude * 10 + i128::from(digit - b'0');
        if magnitude > limit {
            return Err("integer out of range".into());
        }
    }
    Ok(if text.starts_with('-') {
        -magnitude
    } else {
        magnitude
    } as i64)
}

/// Execute one already type-checked test without native compilation.
pub fn run_case(
    program: &Program,
    index: usize,
    value: Option<i64>,
    timeout_ms: u64,
) -> Result<(), Value> {
    let Some(test) = program.tests.get(index) else {
        return Err(json!({"kind":"reference_test_index","status":"FAILED","offset":0}));
    };
    let mut evaluator = Evaluator {
        program,
        deadline: Instant::now() + Duration::from_millis(timeout_ms),
        steps: 0,
        depth: 0,
        expression_depth: 0,
        allocated: 0,
    };
    let mut env = Env::new();
    if let Some((name, min, max)) = &test.generator {
        let Some(value) = value.filter(|n| n >= min && n <= max) else {
            return Err(
                json!({"kind":"reference_generator_value","status":"FAILED","offset":test.at}),
            );
        };
        env.insert(name.clone(), cell(Val::Int(value)));
    }
    evaluator
        .block(&test.body, &mut env)
        .map(|_| ())
        .map_err(|e| e.json(value))
}

fn generated(seed: &mut u64, index: usize, min: i64, max: i64) -> i64 {
    if index == 0 {
        return min;
    }
    if index == 1 {
        return max;
    }
    if (2..5).contains(&index) {
        let edge = [0, -1, 1][index - 2];
        if edge >= min && edge <= max {
            return edge;
        }
    }
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    let width = (i128::from(max) - i128::from(min) + 1) as u128;
    (i128::from(min) + (u128::from(*seed) % width) as i128) as i64
}

pub fn run_tests(
    source: &str,
    program: &Program,
    options: &crate::TestOptions,
) -> Result<Value, String> {
    let selected: Vec<_> = program
        .tests
        .iter()
        .enumerate()
        .filter(|(_, test)| {
            options.filter.as_ref().is_none_or(|filter| {
                if program.tests.iter().any(|t| &t.name == filter) {
                    &test.name == filter
                } else {
                    test.name.contains(filter)
                }
            })
        })
        .collect();
    if options.replay.is_some() && (selected.len() != 1 || selected[0].1.generator.is_none()) {
        return Err(
            "--value replay must select exactly one property; use --filter with its full name"
                .into(),
        );
    }
    let deadline = Instant::now() + Duration::from_millis(options.budget_ms);
    let mut tests = Vec::new();
    for (index, test) in selected {
        if let (Some(value), Some((_, min, max))) = (options.replay, &test.generator)
            && (value < *min || value > *max)
        {
            return Err(format!(
                "replay value {value} is outside [{min}, {max}] for '{}'",
                test.name
            ));
        }
        let cases = if test.generator.is_none() || options.replay.is_some() {
            1
        } else {
            options.cases
        };
        let worker_deadline =
            deadline.min(Instant::now() + Duration::from_millis(options.timeout_ms));
        let mut seed = options.seed.max(1);
        let mut failure = None;
        for case in 0..cases {
            if Instant::now() >= worker_deadline {
                failure = Some(
                    json!({"kind":"execution_limit","status":"UNKNOWN","offset":test.at,"has_value":false,"value":0}),
                );
                break;
            }
            let value = test.generator.as_ref().map(|(_, min, max)| {
                options
                    .replay
                    .unwrap_or_else(|| generated(&mut seed, case, *min, *max))
            });
            let timeout = worker_deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .max(1) as u64;
            if let Err(error) = run_case(program, index, value, timeout) {
                failure = Some(error);
                break;
            }
        }
        let mut attempts = 0;
        let mut shrunk = false;
        if options.shrink
            && options.replay.is_none()
            && let Some(error) = &failure
            && error["status"] == "FAILED"
            && let Some((_, min, max)) = &test.generator
            && let Some(original) = error["value"].as_i64()
        {
            let mut best = original;
            let kind = error["kind"].clone();
            let offset = error["offset"].clone();
            let mut seen = BTreeSet::from([best]);
            while attempts < 32 && Instant::now() < deadline {
                let mut improved = false;
                for candidate in [
                    0,
                    best / 2,
                    if best > 0 {
                        best - 1
                    } else {
                        best.saturating_add(1)
                    },
                    *min,
                    *max,
                ] {
                    if attempts >= 32 || Instant::now() >= deadline {
                        break;
                    }
                    if candidate < *min
                        || candidate > *max
                        || candidate.unsigned_abs() >= best.unsigned_abs()
                        || !seen.insert(candidate)
                    {
                        continue;
                    }
                    attempts += 1;
                    let timeout = options.timeout_ms.min(
                        deadline
                            .saturating_duration_since(Instant::now())
                            .as_millis()
                            .max(1) as u64,
                    );
                    if let Err(error) = run_case(program, index, Some(candidate), timeout)
                        && error["status"] == "FAILED"
                        && error["kind"] == kind
                        && error["offset"] == offset
                    {
                        failure = Some(error);
                        best = candidate;
                        shrunk = true;
                        improved = true;
                        break;
                    }
                }
                if !improved {
                    break;
                }
            }
        }
        let status = failure
            .as_ref()
            .map_or("TESTED", |f| f["status"].as_str().unwrap_or("FAILED"));
        let mut result = json!({"name":test.name,"status":status,"cases":cases,"seed":options.seed,"case_count_is_budget":status!="TESTED"});
        if let Some(mut failure) = failure {
            let at = failure["offset"].as_u64().unwrap_or(0) as usize;
            let location = crate::syntax::Diagnostic::new(source, at, "", "");
            failure["line"] = json!(location.line);
            failure["column"] = json!(location.column);
            failure["shrink_attempts"] = json!(attempts);
            failure["shrunk"] = json!(shrunk);
            if failure["has_value"] == true {
                failure["replay"] = json!({"test":test.name,"value":failure["value"],"revision":crate::revision(source)});
            }
            result["failure"] = failure;
        }
        tests.push(result);
    }
    let status = if tests.is_empty() {
        "UNKNOWN"
    } else if tests.iter().any(|t| t["status"] == "FAILED") {
        "FAILED"
    } else if tests.iter().any(|t| t["status"] == "UNKNOWN") {
        "UNKNOWN"
    } else if tests.iter().any(|t| t["status"] == "BLOCKED") {
        "BLOCKED"
    } else {
        "TESTED"
    };
    Ok(
        json!({"status":status,"revision":crate::revision(source),"tests":tests,"engine":"reference","assurance":"TESTED means recorded cases only; independent evaluator, no native execution","limits":{"steps_per_case":1_000_000,"recursion":64,"cumulative_owned_payload_bytes_per_case":32*1024*1024,"heap_limit_enforced":false,"suite_ms":options.budget_ms,"worker_ms":options.timeout_ms}}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options() -> crate::TestOptions {
        crate::TestOptions {
            cases: 100,
            seed: 42,
            timeout_ms: 2000,
            filter: None,
            replay: None,
            shrink: true,
            memory_mib: 256,
            budget_ms: 30000,
        }
    }
    fn evaluate(source: &str) -> Value {
        let (program, _) = crate::checked(source).unwrap();
        run_tests(source, &program, &options()).unwrap()
    }
    #[test]
    fn reference_examples_and_owned_aliases() {
        for source in [
            include_str!("../examples/web_server.keel"),
            include_str!("../examples/ownership.keel"),
            include_str!("../examples/collections.keel"),
        ] {
            assert_eq!(evaluate(source)["status"], "TESTED");
        }
        assert_eq!(
            evaluate(
                "fn replace(xs: edit List<Int>) { xs = [3] list.push(edit xs, 4) } fn forward(xs: edit List<Int>) { replace(edit xs) } test \"alias\" { var xs = [1] forward(edit xs) assert xs == [3,4] }"
            )["status"],
            "TESTED"
        );
    }
    #[test]
    fn arithmetic_contracts_holes_and_short_circuit() {
        let source = "fn positive(x: Int) -> Int requires x > 0 ensures result > 0 { return x } test \"overflow\" { let x = 9223372036854775807 + 1 } test \"division\" { let x = 1 / 0 } test \"requires\" { positive(0) } test \"hole\" { let x: Int = hole(\"x\") } test \"short circuit\" { assert true || 1 / 0 == 0 }";
        let result = evaluate(source);
        for (index, kind) in [
            "overflow",
            "division_by_zero",
            "precondition_failure",
            "hole_reached",
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(result["tests"][index]["failure"]["kind"], *kind);
        }
        assert_eq!(result["tests"][3]["status"], "BLOCKED");
        assert_eq!(result["tests"][4]["status"], "TESTED");
    }
    #[test]
    fn independent_native_parity_for_examples_and_runtime_failures() {
        for source in [
            include_str!("../examples/collections.keel"),
            include_str!("../examples/counterexample.keel"),
            include_str!("../examples/holes.keel"),
            "test \"overflow\" { let x = -9223372036854775808 / -1 } test \"bounds\" { let x = list.at([1], -1) }",
        ] {
            let (program, analysis) = crate::checked(source).unwrap();
            let reference = run_tests(source, &program, &options()).unwrap();
            let native = crate::run_tests(source, &program, &analysis, &options()).unwrap();
            assert_eq!(reference["status"], native["status"]);
            for (a, b) in reference["tests"]
                .as_array()
                .unwrap()
                .iter()
                .zip(native["tests"].as_array().unwrap())
            {
                assert_eq!(a["status"], b["status"]);
                for field in ["kind", "offset", "value"] {
                    assert_eq!(
                        a["failure"][field], b["failure"][field],
                        "{field}: {a} != {b}"
                    );
                }
            }
        }
    }
    #[test]
    fn parsing_recoverable_errors_and_runtime_contracts_match_native() {
        let source = r#"
        fn bad() -> Int ensures result > 0 { return 0 }
        test "integer boundaries" {
            assert text.parse_int("-9223372036854775808") == result.ok(-9223372036854775808)
            assert text.parse_int("9223372036854775807") == result.ok(9223372036854775807)
            assert text.parse_int("9223372036854775808") == result.err("integer out of range")
            assert text.parse_int("-9223372036854775809") == result.err("integer out of range")
            assert text.parse_int("") == result.err("empty integer")
            assert text.parse_int("+") == result.err("invalid integer")
            assert text.parse_int(" 1") == result.err("invalid integer")
            assert text.parse_int("١") == result.err("invalid integer")
            assert text.parse_int("-0") == result.ok(0)
        }
        test "postcondition" { bad() }
        test "list set bounds" { var values = [1] list.set(edit values, 2, 4) }
        test "move restore loop" {
            var value = text.clone("a") var index = 0
            while index < 10 { let old = take value value = text.concat(old, "b") index = index + 1 }
            assert text.len(value) == 11
        }
        "#;
        let (program, analysis) = crate::checked(source).unwrap();
        let reference = run_tests(source, &program, &options()).unwrap();
        let native = crate::run_tests(source, &program, &analysis, &options()).unwrap();
        for (a, b) in reference["tests"]
            .as_array()
            .unwrap()
            .iter()
            .zip(native["tests"].as_array().unwrap())
        {
            assert_eq!(a["status"], b["status"], "{a} != {b}");
            assert_eq!(a["failure"]["kind"], b["failure"]["kind"]);
            assert_eq!(a["failure"]["offset"], b["failure"]["offset"]);
        }
    }

    #[test]
    fn reference_limits_and_external_effects_never_report_tested() {
        let allocated = evaluate(
            "test \"growth\" { var text = text.clone(\"a\") while true { text = text.concat(text, text) } }",
        );
        assert_eq!(allocated["status"], "UNKNOWN");
        assert_eq!(
            allocated["tests"][0]["failure"]["kind"],
            "reference_allocation_limit"
        );
        // Parsing directly exercises the evaluator's explicit host-effect rejection.
        // The normal CLI rejects an effectful test earlier during static checking.
        let program =
            crate::syntax::parse("test \"effect\" { io.println(\"never print\") }").unwrap();
        let error = run_case(&program, 0, None, 1000).unwrap_err();
        assert_eq!(error["status"], "BLOCKED");
        assert_eq!(error["kind"], "permission_denied_stdout");
    }

    #[test]
    fn bounded_loops_recursion_and_generation() {
        for source in [
            "test \"loop\" { while true {} }",
            "fn recurse() -> Int { return recurse() } test \"recursion\" { recurse() }",
        ] {
            assert_eq!(evaluate(source)["status"], "UNKNOWN");
        }
        let mut seed = 42;
        assert_eq!(generated(&mut seed, 0, i64::MIN, i64::MAX), i64::MIN);
        assert_eq!(generated(&mut seed, 1, i64::MIN, i64::MAX), i64::MAX);
        assert_eq!(
            evaluate(
                "property \"full range\" (n in gen.int(min: -9223372036854775808, max: 9223372036854775807)) { assert n >= -9223372036854775808 }"
            )["status"],
            "TESTED"
        );
    }
}
