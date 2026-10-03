use crate::syntax::*;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

fn expression(e: &Expr, used: &mut BTreeSet<String>, calls: &mut BTreeSet<String>) {
    match &e.kind {
        ExprKind::Var(name) | ExprKind::Move(name) | ExprKind::Edit(name) => {
            used.insert(name.clone());
        }
        ExprKind::List(items) => {
            for e in items {
                expression(e, used, calls)
            }
        }
        ExprKind::Call(name, args) => {
            calls.insert(name.clone());
            for e in args {
                expression(e, used, calls)
            }
        }
        ExprKind::Unary(_, e) => expression(e, used, calls),
        ExprKind::Binary(_, a, b) => {
            expression(a, used, calls);
            expression(b, used, calls);
        }
        _ => {}
    }
}
fn block(
    body: &[Stmt],
    bindings: &mut BTreeMap<String, usize>,
    used: &mut BTreeSet<String>,
    calls: &mut BTreeSet<String>,
) {
    for stmt in body {
        match &stmt.kind {
            StmtKind::Bind { name, value, .. } => {
                bindings.insert(name.clone(), stmt.at);
                expression(value, used, calls);
            }
            StmtKind::Assign(_, e) | StmtKind::Assert(e) | StmtKind::Expr(e) => {
                expression(e, used, calls)
            }
            StmtKind::Return(Some(e)) => expression(e, used, calls),
            StmtKind::Return(None) => {}
            StmtKind::If(e, a, b) => {
                expression(e, used, calls);
                block(a, bindings, used, calls);
                block(b, bindings, used, calls)
            }
            StmtKind::While(e, body) => {
                expression(e, used, calls);
                block(body, bindings, used, calls)
            }
            StmtKind::For(name, e, body) => {
                bindings.insert(name.clone(), stmt.at);
                expression(e, used, calls);
                block(body, bindings, used, calls)
            }
            StmtKind::Match(e, arms) => {
                expression(e, used, calls);
                for arm in arms {
                    if let Some(name) = &arm.binding {
                        bindings.insert(name.clone(), stmt.at);
                    }
                    block(&arm.body, bindings, used, calls)
                }
            }
        }
    }
}
pub fn run(source: &str, program: &Program, deny: bool) -> Value {
    let mut warnings = Vec::new();
    for f in &program.functions {
        let mut bindings: BTreeMap<_, _> =
            f.params.iter().map(|p| (p.name.clone(), f.start)).collect();
        let mut used = BTreeSet::new();
        let mut calls = BTreeSet::new();
        for contract in f.requires.iter().chain(&f.ensures) {
            expression(contract, &mut used, &mut calls);
        }
        block(&f.body, &mut bindings, &mut used, &mut calls);
        for (name, offset) in bindings {
            if !name.starts_with('_') && !used.contains(&name) {
                let mut d = serde_json::to_value(Diagnostic::new(
                    source,
                    offset,
                    "unused_binding",
                    format!("'{name}' is never read; prefix deliberately unused names with '_'"),
                ))
                .unwrap();
                d["severity"] = json!("warning");
                warnings.push(d);
            }
        }
        let mut needed = BTreeSet::new();
        for call in calls {
            if call == "http.serve" {
                needed.insert("net.listen".to_string());
            } else if let Some(b) = crate::check::builtin(&call) {
                needed.extend(b.effects);
            } else if let Some(callee) = program.functions.iter().find(|f| f.name == call) {
                needed.extend(callee.effects.clone());
            }
        }
        for effect in &f.effects {
            if !needed.contains(effect) {
                let mut d = serde_json::to_value(Diagnostic::new(
                    source,
                    f.start,
                    "unused_effect",
                    format!("declared effect '{effect}' is not used by this implementation"),
                ))
                .unwrap();
                d["severity"] = json!("warning");
                warnings.push(d);
            }
        }
    }
    json!({"status":if deny && !warnings.is_empty(){"FAILED"}else{"LINTED"},"revision":crate::revision(source),"diagnostics":warnings,"deny_warnings":deny,"limitations":"name-based unused-binding lint; not a path-sensitive dataflow proof"})
}
