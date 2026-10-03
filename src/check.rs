use crate::syntax::*;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct Signature {
    pub params: Vec<(Type, Mode)>,
    pub result: Type,
    pub effects: Vec<String>,
}
pub fn builtin(name: &str) -> Option<Signature> {
    use Mode::*;
    use Type::*;
    let (params, result, effects) = match name {
        "result.text_ok" | "result.text_err" => (vec![(Text, Take)], ResultTextText, vec![]),
        "json.parse" => (vec![(Text, Read)], ResultTextText, vec![]),
        "json.get" | "json.text" | "dotenv.get" | "http.query" | "http.header" => {
            (vec![(Text, Read), (Text, Read)], ResultTextText, vec![])
        }
        "json.set" => (vec![(Text, Read); 3], ResultTextText, vec![]),
        "json.array_len" => (vec![(Text, Read)], ResultIntText, vec![]),
        "http.path" => (vec![(Text, Read)], Text, vec![]),
        "json.int" => (vec![(Text, Read), (Text, Read)], ResultIntText, vec![]),
        "json.quote" => (vec![(Text, Read)], Text, vec![]),
        "csv.get" => (
            vec![(Text, Read), (Int, Value), (Int, Value)],
            ResultTextText,
            vec![],
        ),
        "xml.text" => (vec![(Text, Read), (Text, Read)], ResultTextText, vec![]),
        "sse.data" => (vec![(Text, Read), (Int, Value)], ResultTextText, vec![]),
        "fs.read_text" => (vec![(Text, Read)], ResultTextText, vec!["fs.read".into()]),
        "fs.write_text" => (
            vec![(Text, Read), (Text, Read)],
            ResultIntText,
            vec!["fs.write".into()],
        ),
        "process.run" => (
            vec![(Text, Read), (Text, Read)],
            ResultTextText,
            vec!["process.exec".into()],
        ),
        "process.run_timeout" => (
            vec![(Text, Read), (Text, Read), (Int, Value)],
            ResultTextText,
            vec!["process.exec".into()],
        ),
        "process.spawn" => (
            vec![(Text, Read), (Text, Read)],
            ResultIntText,
            vec!["process.exec".into()],
        ),
        "process.poll" | "process.terminate" => (
            vec![(Int, Value)],
            ResultIntText,
            vec!["process.exec".into()],
        ),
        "clock.millis" => (vec![], Int, vec!["clock.read".into()]),
        "env.get" => (vec![(Text, Read)], ResultTextText, vec!["env.read".into()]),
        "http.get" => (
            vec![(Text, Read)],
            ResultTextText,
            vec!["net.connect".into()],
        ),
        "http.post_json" => (
            vec![(Text, Read), (Text, Read), (Text, Read)],
            ResultTextText,
            vec!["net.connect".into()],
        ),
        "http.post_json_timeout" => (
            vec![(Text, Read), (Text, Read), (Text, Read), (Int, Value)],
            ResultTextText,
            vec!["net.connect".into()],
        ),
        "http.json_response" => (vec![(Int, Value), (Text, Read)], ResultTextText, vec![]),
        "tcp.exchange" | "udp.exchange" => (
            vec![(Text, Read), (Int, Value), (Text, Read)],
            ResultTextText,
            vec!["net.connect".into()],
        ),
        "websocket.exchange" => (
            vec![(Text, Read), (Text, Read)],
            ResultTextText,
            vec!["net.connect".into()],
        ),
        "text.clone" => (vec![(Text, Read)], Text, vec![]),
        "text.concat" => (vec![(Text, Read), (Text, Read)], Text, vec![]),
        "text.len" => (vec![(Text, Read)], Int, vec![]),
        "text.from_int" => (vec![(Int, Value)], Text, vec![]),
        "text.parse_int" => (vec![(Text, Read)], ResultIntText, vec![]),
        "list.new" => (vec![], ListInt, vec![]),
        "list.clone" => (vec![(ListInt, Read)], ListInt, vec![]),
        "list.len" => (vec![(ListInt, Read)], Int, vec![]),
        "list.get" => (vec![(ListInt, Read), (Int, Value)], OptionInt, vec![]),
        "list.at" => (vec![(ListInt, Read), (Int, Value)], Int, vec![]),
        "list.contains" => (vec![(ListInt, Read), (Int, Value)], Bool, vec![]),
        "list.push" => (vec![(ListInt, Edit), (Int, Value)], Unit, vec![]),
        "list.set" => (
            vec![(ListInt, Edit), (Int, Value), (Int, Value)],
            Unit,
            vec![],
        ),
        "option.some" => (vec![(Int, Value)], OptionInt, vec![]),
        "option.none" => (vec![], OptionInt, vec![]),
        "result.ok" => (vec![(Int, Value)], ResultIntText, vec![]),
        "result.err" => (vec![(Text, Take)], ResultIntText, vec![]),
        "io.println" => (vec![(Text, Read)], Unit, vec!["io.stdout".into()]),
        "http.response" => (vec![(Int, Value), (Text, Read)], Text, vec![]),
        "http.status" => (vec![(Text, Read)], Int, vec![]),
        "http.body" => (vec![(Text, Read)], Text, vec![]),
        _ => return None,
    };
    Some(Signature {
        params,
        result,
        effects,
    })
}
#[derive(Clone, Debug, Serialize)]
pub struct Hole {
    pub id: String,
    pub function: String,
    pub expected: Type,
    pub offset: usize,
    pub bindings: BTreeMap<String, Type>,
    pub allowed_effects: Vec<String>,
}
#[derive(Default)]
pub struct Analysis {
    pub holes: Vec<Hole>,
    pub calls: BTreeMap<String, BTreeSet<String>>,
}
#[derive(Clone)]
struct Binding {
    ty: Type,
    mutable: bool,
    borrowed: bool,
    moved: bool,
}
type Env = BTreeMap<String, Binding>;
pub fn check(source: &str, program: &Program) -> DResult<Analysis> {
    let mut signatures = BTreeMap::new();
    for f in &program.functions {
        if [
            "true", "false", "result", "hole", "take", "let", "var", "return", "if", "else",
            "while", "assert", "for", "in", "match", "edit", "Some", "None", "Ok", "Err",
        ]
        .contains(&f.name.as_str())
        {
            return Err(Diagnostic::new(
                source,
                f.start,
                "name",
                "reserved function name",
            ));
        }
        if signatures
            .insert(
                f.name.clone(),
                Signature {
                    params: f.params.iter().map(|p| (p.ty, p.mode)).collect(),
                    result: f.result,
                    effects: f.effects.clone(),
                },
            )
            .is_some()
        {
            return Err(Diagnostic::new(
                source,
                f.start,
                "duplicate_name",
                format!("duplicate function '{}'", f.name),
            ));
        }
        for effect in &f.effects {
            if ![
                "io.stdout",
                "net.listen",
                "net.connect",
                "fs.read",
                "fs.write",
                "process.exec",
                "clock.read",
                "env.read",
            ]
            .contains(&effect.as_str())
            {
                return Err(Diagnostic::new(
                    source,
                    f.start,
                    "effect",
                    format!("unknown effect '{effect}'"),
                ));
            }
        }
        if f.name == "main" && (!f.params.is_empty() || f.result != Type::Unit) {
            return Err(Diagnostic::new(
                source,
                f.start,
                "entrypoint",
                "main must have no parameters and return Unit",
            ));
        }
    }
    let mut c = Checker {
        source,
        signatures,
        analysis: Analysis::default(),
        function: String::new(),
        effects: Vec::new(),
        result: Type::Unit,
        in_contract: false,
        in_test: false,
        reads: BTreeSet::new(),
        edits: BTreeSet::new(),
    };
    for f in &program.functions {
        c.function = f.name.clone();
        c.effects = f.effects.clone();
        c.result = f.result;
        c.in_test = false;
        let mut env = Env::new();
        for p in &f.params {
            if p.ty == Type::Unit
                || (p.ty.owned() && p.mode == Mode::Value)
                || (!p.ty.owned() && p.mode != Mode::Value)
            {
                return c.err(f.start, "parameter_mode", "owned parameters require read, edit, or take; copyable types use value mode; Unit cannot be stored");
            }
            c.bind(
                &mut env,
                &p.name,
                Binding {
                    ty: p.ty,
                    mutable: p.mode == Mode::Edit,
                    borrowed: matches!(p.mode, Mode::Read | Mode::Edit),
                    moved: false,
                },
                f.start,
            )?;
        }
        c.in_contract = true;
        for e in &f.requires {
            c.expect(e, &mut env, Type::Bool, false)?;
        }
        c.in_contract = false;
        let returned = c.block(&f.body, &mut env)?;
        if f.result != Type::Unit && !returned {
            return c.err(
                f.start,
                "missing_return",
                format!("function '{}' does not return on every path", f.name),
            );
        }
        c.in_contract = true;
        env.insert(
            "result".into(),
            Binding {
                ty: f.result,
                mutable: false,
                borrowed: true,
                moved: false,
            },
        );
        for e in &f.ensures {
            c.expect(e, &mut env, Type::Bool, false)?;
        }
        c.in_contract = false;
    }
    c.in_test = true;
    let mut names = BTreeSet::new();
    for test in &program.tests {
        if !names.insert(&test.name) {
            return c.err(test.at, "duplicate_name", "duplicate test name");
        }
        c.function = format!("test:{}", test.name);
        c.effects.clear();
        c.result = Type::Unit;
        let mut env = Env::new();
        if let Some((name, _, _)) = &test.generator {
            c.bind(
                &mut env,
                name,
                Binding {
                    ty: Type::Int,
                    mutable: false,
                    borrowed: false,
                    moved: false,
                },
                test.at,
            )?;
        }
        c.block(&test.body, &mut env)?;
    }
    Ok(c.analysis)
}
struct Checker<'a> {
    source: &'a str,
    signatures: BTreeMap<String, Signature>,
    analysis: Analysis,
    function: String,
    effects: Vec<String>,
    result: Type,
    in_contract: bool,
    in_test: bool,
    reads: BTreeSet<String>,
    edits: BTreeSet<String>,
}
impl Checker<'_> {
    fn err<T>(&self, at: usize, kind: &str, message: impl Into<String>) -> DResult<T> {
        Err(Diagnostic::new(self.source, at, kind, message))
    }
    fn bind(&self, env: &mut Env, name: &str, value: Binding, at: usize) -> DResult<()> {
        if name == "result" || name == "true" || name == "false" || env.contains_key(name) {
            return self.err(
                at,
                "duplicate_name",
                format!("'{name}' is reserved or already bound; v0 forbids shadowing"),
            );
        }
        env.insert(name.into(), value);
        Ok(())
    }
    fn expect(&mut self, e: &Expr, env: &mut Env, ty: Type, consuming: bool) -> DResult<()> {
        let actual = self.expr(e, env, Some(ty), consuming)?;
        if actual != ty {
            return self.err(
                e.at,
                "type_mismatch",
                format!("expected {ty:?}, found {actual:?}"),
            );
        }
        Ok(())
    }
    fn effects(&self, at: usize, required: &[String]) -> DResult<()> {
        for effect in required {
            if self.in_contract || !self.effects.contains(effect) {
                return self.err(at, "effect_not_allowed", format!("'{effect}' is not permitted in {}; declare it in the caller's effects boundary", self.function));
            }
        }
        Ok(())
    }
    fn expr(
        &mut self,
        e: &Expr,
        env: &mut Env,
        expected: Option<Type>,
        consuming: bool,
    ) -> DResult<Type> {
        use ExprKind::*;
        Ok(match &e.kind {
            Int(_) => Type::Int,
            Bool(_) => Type::Bool,
            Text(_) => Type::Text,
            List(values) => {
                for value in values {
                    self.expect(value, env, Type::Int, false)?;
                }
                Type::ListInt
            }
            Edit(_) => {
                return self.err(
                    e.at,
                    "invalid_edit",
                    "edit is only valid at an edit parameter call site",
                );
            }
            Var(name) | Move(name) => {
                if self.edits.contains(name)
                    || (matches!(e.kind, Move(_)) && self.reads.contains(name))
                {
                    return self.err(
                        e.at,
                        "borrow_conflict",
                        format!("'{name}' is already borrowed"),
                    );
                }
                let b = match env.get_mut(name) {
                    Some(b) => b,
                    None => {
                        return self.err(
                            e.at,
                            "unknown_name",
                            format!("unknown variable '{name}'"),
                        );
                    }
                };
                if b.moved {
                    return self.err(e.at, "use_after_move", format!("'{name}' was moved; explicitly clone before transferring it if both owners need it"));
                }
                if matches!(e.kind, Move(_)) {
                    if self.in_contract {
                        return self.err(
                            e.at,
                            "contract_ownership",
                            "contracts cannot transfer ownership",
                        );
                    }
                    if !b.ty.owned() || b.borrowed {
                        return self.err(
                            e.at,
                            "invalid_move",
                            "take requires an owned, non-borrowed value",
                        );
                    }
                    b.moved = true;
                } else if consuming && b.ty.owned() {
                    return self.err(e.at, "ownership", format!("'{name}' is not implicitly copied; use 'take {name}' or an explicit clone"));
                }
                b.ty
            }
            Unary(op, inner) => {
                let ty = if op == "!" { Type::Bool } else { Type::Int };
                self.expect(inner, env, ty, false)?;
                ty
            }
            Binary(op, left, right) => {
                let lhs = self.expr(left, env, None, false)?;
                let previous = env.clone();
                let saved_reads = self.reads.clone();
                if lhs.owned()
                    && let Var(name) = &left.kind
                {
                    self.reads.insert(name.clone());
                }
                self.expect(right, env, lhs, false)?;
                self.reads = saved_reads;
                if lhs.owned()
                    && let Var(name) = &left.kind
                    && env.get(name).is_some_and(|b| b.moved)
                {
                    return self.err(
                        right.at,
                        "borrow_conflict",
                        format!("'{name}' is borrowed by the left operand"),
                    );
                }
                if op == "&&" || op == "||" {
                    Self::merge(env, &previous);
                }
                match op.as_str() {
                    "==" | "!=" if lhs != Type::Unit => Type::Bool,
                    "<" | "<=" | ">" | ">=" if lhs == Type::Int => Type::Bool,
                    "&&" | "||" if lhs == Type::Bool => Type::Bool,
                    "+" | "-" | "*" | "/" | "%" if lhs == Type::Int => Type::Int,
                    _ => {
                        return self.err(
                            e.at,
                            "operator_type",
                            format!("operator '{op}' does not accept {lhs:?}"),
                        );
                    }
                }
            }
            Call(name, args) if name == "hole" => {
                if self.in_contract {
                    return self.err(e.at, "contract_hole", "contracts cannot contain holes");
                }
                let id = if let [Expr { kind: Text(id), .. }] = args.as_slice() {
                    id.clone()
                } else {
                    return self.err(e.at, "hole", "hole requires one string literal identifier");
                };
                let expected = match expected {
                    Some(t) => t,
                    None => {
                        return self.err(
                            e.at,
                            "hole_type",
                            "add an explicit type annotation around this hole",
                        );
                    }
                };
                self.analysis.holes.push(Hole {
                    id,
                    function: self.function.clone(),
                    expected,
                    offset: e.at,
                    bindings: env
                        .iter()
                        .filter(|(_, b)| !b.moved)
                        .map(|(n, b)| (n.clone(), b.ty))
                        .collect(),
                    allowed_effects: self.effects.clone(),
                });
                expected
            }
            Call(name, args) if name == "parallel.map" => {
                if args.len() != 2 {
                    return self.err(
                        e.at,
                        "arity",
                        "parallel.map expects a List<Int> and a pure fn(Int) -> Int",
                    );
                }
                self.expect(&args[0], env, Type::ListInt, false)?;
                let Var(handler) = &args[1].kind else {
                    return self.err(e.at, "handler", "worker must be a named function");
                };
                if !self.signatures.get(handler).is_some_and(|s| {
                    s.params == vec![(Type::Int, Mode::Value)]
                        && s.result == Type::Int
                        && s.effects.is_empty()
                }) {
                    return self.err(
                        e.at,
                        "handler",
                        "parallel worker must be a pure fn(Int) -> Int",
                    );
                }
                self.analysis
                    .calls
                    .entry(self.function.clone())
                    .or_default()
                    .insert(handler.clone());
                Type::ListInt
            }
            Call(name, args) if name == "http.serve_app" || name == "http.serve_api" => {
                self.effects(e.at, &["net.listen".into(), "fs.read".into()])?;
                if args.len() != 3 {
                    return self.err(
                        e.at,
                        "arity",
                        format!("{name} expects a port, static root, and named handler"),
                    );
                }
                self.expect(&args[0], env, Type::Int, false)?;
                self.expect(&args[1], env, Type::Text, false)?;
                let Var(handler) = &args[2].kind else {
                    return self.err(e.at, "handler", "handler must be a named function");
                };
                let Some(sig) = self.signatures.get(handler).cloned() else {
                    return self.err(args[2].at, "handler", "unknown HTTP application handler");
                };
                let count = if name == "http.serve_api" { 4 } else { 3 };
                if sig.params != vec![(Type::Text, Mode::Read); count] || sig.result != Type::Text {
                    let signature = if count == 4 {
                        "API handler must have signature fn(method: read Text, target: read Text, headers: read Text, body: read Text) -> Text"
                    } else {
                        "application handler must have signature fn(method: read Text, path: read Text, body: read Text) -> Text"
                    };
                    return self.err(args[2].at, "handler", signature);
                }
                self.effects(e.at, &sig.effects)?;
                self.analysis
                    .calls
                    .entry(self.function.clone())
                    .or_default()
                    .insert(handler.clone());
                Type::Unit
            }
            Call(name, args) if name == "http.serve" => {
                self.effects(e.at, &["net.listen".into()])?;
                if args.len() != 2 {
                    return self.err(
                        e.at,
                        "arity",
                        "http.serve expects a port and a pure handler function",
                    );
                }
                self.expect(&args[0], env, Type::Int, false)?;
                let handler = if let Var(n) = &args[1].kind {
                    n
                } else {
                    return self.err(e.at, "handler", "handler must be a named function");
                };
                let sig = self.signatures.get(handler);
                if !sig.is_some_and(|s| {
                    s.params == vec![(Type::Text, Mode::Read)]
                        && s.result == Type::Text
                        && s.effects.is_empty()
                }) {
                    return self.err(args[1].at, "handler", "HTTP handler must have signature fn handler(path: read Text) -> Text with no external effects");
                }
                self.analysis
                    .calls
                    .entry(self.function.clone())
                    .or_default()
                    .insert(handler.clone());
                Type::Unit
            }
            Call(name, args) => {
                let sig = self.signatures.get(name).cloned().or_else(|| builtin(name)).ok_or_else(|| Diagnostic::new(self.source, e.at, "unknown_function", format!("unknown function '{name}'; dependencies are never installed implicitly")))?;
                self.effects(e.at, &sig.effects)?;
                if args.len() != sig.params.len() {
                    return self.err(
                        e.at,
                        "arity",
                        format!(
                            "{name} expects {} arguments, found {}",
                            sig.params.len(),
                            args.len()
                        ),
                    );
                }
                if self.in_contract
                    && sig
                        .params
                        .iter()
                        .any(|(_, m)| matches!(m, Mode::Take | Mode::Edit))
                {
                    return self.err(
                        e.at,
                        "contract_ownership",
                        "contracts cannot call consuming or editing functions",
                    );
                }
                // A read argument stays borrowed through the entire call, including later argument evaluation.
                let saved_reads = self.reads.clone();
                let saved_edits = self.edits.clone();
                for (arg, (ty, mode)) in args.iter().zip(&sig.params) {
                    if *mode == Mode::Edit {
                        let Edit(name) = &arg.kind else {
                            return self.err(
                                arg.at,
                                "invalid_edit",
                                "edit parameters require 'edit variable'",
                            );
                        };
                        if self.reads.contains(name) || self.edits.contains(name) {
                            return self.err(
                                arg.at,
                                "borrow_conflict",
                                format!("'{name}' is already borrowed"),
                            );
                        }
                        let b = env.get(name).ok_or_else(|| {
                            Diagnostic::new(
                                self.source,
                                arg.at,
                                "unknown_name",
                                format!("unknown variable '{name}'"),
                            )
                        })?;
                        if b.moved {
                            return self.err(
                                arg.at,
                                "use_after_move",
                                format!("'{name}' was moved"),
                            );
                        }
                        if !b.mutable {
                            return self.err(
                                arg.at,
                                "immutable",
                                "edit requires a mutable variable or edit parameter",
                            );
                        }
                        if b.ty != *ty {
                            return self.err(
                                arg.at,
                                "type_mismatch",
                                format!("expected {ty:?}, found {:?}", b.ty),
                            );
                        }
                        self.edits.insert(name.clone());
                    } else {
                        self.expect(arg, env, *ty, *mode == Mode::Take)?;
                        if *mode == Mode::Read
                            && let Var(name) = &arg.kind
                        {
                            self.reads.insert(name.clone());
                        }
                    }
                }
                self.reads = saved_reads;
                self.edits = saved_edits;
                self.analysis
                    .calls
                    .entry(self.function.clone())
                    .or_default()
                    .insert(name.clone());
                sig.result
            }
        })
    }
    fn merge(env: &mut Env, other: &Env) {
        for (name, b) in env {
            if let Some(o) = other.get(name) {
                b.moved |= o.moved;
            }
        }
    }
    fn block(&mut self, body: &[Stmt], env: &mut Env) -> DResult<bool> {
        let outer: BTreeSet<String> = env.keys().cloned().collect();
        let mut returned = false;
        for stmt in body {
            if returned {
                return self.err(stmt.at, "unreachable", "unreachable statement after return");
            }
            match &stmt.kind {
                StmtKind::Bind {
                    mutable,
                    name,
                    annotation,
                    value,
                } => {
                    let ty = self.expr(value, env, *annotation, true)?;
                    if let Some(t) = annotation
                        && *t != ty
                    {
                        return self.err(
                            stmt.at,
                            "type_mismatch",
                            format!("expected {t:?}, found {ty:?}"),
                        );
                    }
                    if ty == Type::Unit {
                        return self.err(stmt.at, "type_mismatch", "Unit cannot be stored");
                    }
                    self.bind(
                        env,
                        name,
                        Binding {
                            ty,
                            mutable: *mutable,
                            borrowed: false,
                            moved: false,
                        },
                        stmt.at,
                    )?;
                }
                StmtKind::Assign(name, value) => {
                    if self.reads.contains(name) || self.edits.contains(name) {
                        return self.err(
                            stmt.at,
                            "borrow_conflict",
                            format!("'{name}' is borrowed"),
                        );
                    }
                    let b = env.get(name).cloned().ok_or_else(|| {
                        Diagnostic::new(
                            self.source,
                            stmt.at,
                            "unknown_name",
                            format!("unknown variable '{name}'"),
                        )
                    })?;
                    if !b.mutable {
                        return self.err(
                            stmt.at,
                            "immutable",
                            format!("'{name}' is immutable; declare var to allow assignment"),
                        );
                    }
                    self.expect(value, env, b.ty, true)?;
                    env.get_mut(name).unwrap().moved = false;
                }
                StmtKind::Return(value) => {
                    if self.in_test {
                        return self.err(
                            stmt.at,
                            "test_return",
                            "tests cannot return early; all assertions must execute",
                        );
                    }
                    if let Some(e) = value {
                        if let ExprKind::Var(name) = &e.kind {
                            if env.get(name).is_some_and(|b| b.ty.owned() && !b.borrowed) {
                                self.expect(
                                    &Expr {
                                        at: e.at,
                                        kind: ExprKind::Move(name.clone()),
                                    },
                                    env,
                                    self.result,
                                    true,
                                )?;
                            } else {
                                self.expect(e, env, self.result, true)?;
                            }
                        } else {
                            self.expect(e, env, self.result, true)?;
                        }
                    } else if self.result != Type::Unit {
                        return self.err(stmt.at, "type_mismatch", "return requires a value");
                    }
                    returned = true;
                }
                StmtKind::Assert(e) => {
                    self.expect(e, env, Type::Bool, false)?;
                }
                StmtKind::Expr(e) => {
                    self.expr(e, env, Some(Type::Unit), false)?;
                }
                StmtKind::If(condition, yes, no) => {
                    self.expect(condition, env, Type::Bool, false)?;
                    let mut a = env.clone();
                    let mut b = env.clone();
                    let ra = self.block(yes, &mut a)?;
                    let rb = self.block(no, &mut b)?;
                    *env = if ra && !rb {
                        b
                    } else if rb && !ra {
                        a
                    } else {
                        Self::merge(&mut a, &b);
                        a
                    };
                    returned = ra && rb;
                }
                StmtKind::While(condition, body) => {
                    let before = env.clone();
                    self.expect(condition, env, Type::Bool, false)?;
                    let mut inside = env.clone();
                    self.block(body, &mut inside)?;
                    for (name, b) in &before {
                        if !b.moved
                            && (inside.get(name).unwrap().moved || env.get(name).unwrap().moved)
                        {
                            return self.err(stmt.at, "loop_move", format!("loop may repeatedly use moved '{name}'; restore ownership on every iteration"));
                        }
                    }
                    Self::merge(env, &inside);
                }
                StmtKind::For(name, values, body) => {
                    self.expect(values, env, Type::ListInt, false)?;
                    let saved_reads = self.reads.clone();
                    if let ExprKind::Var(name) = &values.kind {
                        self.reads.insert(name.clone());
                    }
                    let before = env.clone();
                    let mut inside = env.clone();
                    self.bind(
                        &mut inside,
                        name,
                        Binding {
                            ty: Type::Int,
                            mutable: false,
                            borrowed: false,
                            moved: false,
                        },
                        stmt.at,
                    )?;
                    self.block(body, &mut inside)?;
                    for (name, b) in &before {
                        if !b.moved && inside[name].moved {
                            return self.err(
                                stmt.at,
                                "loop_move",
                                format!("loop may repeatedly use moved '{name}'"),
                            );
                        }
                    }
                    self.reads = saved_reads;
                    Self::merge(env, &inside);
                }
                StmtKind::Match(value, arms) => {
                    let ty = self.expr(value, env, None, false)?;
                    let variants: &[(&str, Option<Type>)] = match ty {
                        Type::OptionInt => &[("Some", Some(Type::Int)), ("None", None)],
                        Type::ResultTextText => {
                            &[("Ok", Some(Type::Text)), ("Err", Some(Type::Text))]
                        }
                        Type::ResultIntText => {
                            &[("Ok", Some(Type::Int)), ("Err", Some(Type::Text))]
                        }
                        _ => {
                            return self.err(
                                stmt.at,
                                "match_type",
                                "match requires Option<Int>, Result<Int, Text>, or Result<Text, Text>",
                            );
                        }
                    };
                    let saved_reads = self.reads.clone();
                    if ty.owned()
                        && let ExprKind::Var(name) = &value.kind
                    {
                        self.reads.insert(name.clone());
                    }
                    let mut seen = BTreeSet::new();
                    let mut continuing: Option<Env> = None;
                    for arm in arms {
                        let Some((_, payload)) = variants.iter().find(|(v, _)| *v == arm.variant)
                        else {
                            return self.err(
                                stmt.at,
                                "match_variant",
                                "variant does not belong to matched type",
                            );
                        };
                        if !seen.insert(arm.variant.clone()) {
                            return self.err(stmt.at, "match_variant", "duplicate match arm");
                        }
                        if payload.is_some() != arm.binding.is_some() {
                            return self.err(
                                stmt.at,
                                "match_variant",
                                "payload variants require one binding; None has no payload",
                            );
                        }
                        let mut inside = env.clone();
                        if let (Some(name), Some(payload)) = (&arm.binding, payload) {
                            self.bind(
                                &mut inside,
                                name,
                                Binding {
                                    ty: *payload,
                                    mutable: false,
                                    borrowed: payload.owned(),
                                    moved: false,
                                },
                                stmt.at,
                            )?;
                        }
                        let branch_returns = self.block(&arm.body, &mut inside)?;
                        inside.retain(|name, _| env.contains_key(name));
                        if !branch_returns {
                            if let Some(previous) = &mut continuing {
                                Self::merge(previous, &inside);
                            } else {
                                continuing = Some(inside);
                            }
                        }
                    }
                    if seen.len() != variants.len() {
                        return self.err(
                            stmt.at,
                            "non_exhaustive_match",
                            "match must handle every variant",
                        );
                    }
                    self.reads = saved_reads;
                    returned = continuing.is_none();
                    if let Some(inside) = continuing {
                        *env = inside;
                    }
                }
            }
        }
        env.retain(|name, _| outer.contains(name));
        Ok(returned)
    }
}
