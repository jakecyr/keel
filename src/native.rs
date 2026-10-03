use crate::check::{Analysis, builtin};
use crate::syntax::*;
use std::collections::BTreeMap;
use std::fmt::Write;

fn ctype(ty: Type) -> &'static str {
    match ty {
        Type::Int => "int64_t",
        Type::Bool => "bool",
        Type::Text => "KText",
        Type::ListInt => "KList",
        Type::OptionInt => "KOption",
        Type::ResultIntText => "KResult",
        Type::ResultTextText => "KTextResult",
        Type::Unit => "void",
    }
}
pub fn cstring(text: &str) -> String {
    let mut out = String::from("\"");
    for b in text.bytes() {
        write!(out, "\\{b:03o}").unwrap();
    }
    out.push('"');
    out
}
fn number(value: i64) -> String {
    if value == i64::MIN {
        "INT64_MIN".into()
    } else if value < 0 {
        format!("(-INT64_C({}))", -value)
    } else {
        format!("INT64_C({value})")
    }
}
#[derive(Clone)]
struct Value {
    name: String,
    ty: Type,
    owned: bool,
}
struct Emitter<'a> {
    program: &'a Program,
    analysis: &'a Analysis,
    declarations: String,
    code: String,
    env: BTreeMap<String, Value>,
    drops: Vec<String>,
    temporary: Vec<String>,
    next: usize,
}
impl<'a> Emitter<'a> {
    fn new(program: &'a Program, analysis: &'a Analysis) -> Self {
        Self {
            program,
            analysis,
            declarations: String::new(),
            code: String::new(),
            env: BTreeMap::new(),
            drops: Vec::new(),
            temporary: Vec::new(),
            next: 0,
        }
    }
    fn line(&mut self, s: impl AsRef<str>) {
        writeln!(self.code, "    {}", s.as_ref()).unwrap();
    }
    fn slot(&mut self, ty: Type, temporary: bool) -> Value {
        let name = format!("v{}", self.next);
        self.next += 1;
        writeln!(self.declarations, "    {} {name} = {{0}};", ctype(ty)).unwrap();
        if ty.owned() {
            self.drops.push(name.clone());
            if temporary {
                self.temporary.push(name.clone());
            }
        }
        Value {
            name,
            ty,
            owned: ty.owned(),
        }
    }
    fn assign_temp(&mut self, ty: Type, value: String) -> Value {
        if ty == Type::Unit {
            self.line(format!("{value};"));
            return Value {
                name: String::new(),
                ty,
                owned: false,
            };
        }
        let result = self.slot(ty, true);
        self.line(format!("{} = {value};", result.name));
        result
    }
    fn transfer(v: &Value) -> String {
        if v.ty.owned() && v.owned {
            format!("k_move(&{})", v.name)
        } else {
            v.name.clone()
        }
    }
    fn drop_temps(&mut self, from: usize) {
        for name in &self.temporary[from..] {
            writeln!(self.code, "    k_drop(&{name});").unwrap();
        }
    }
    fn expr(&mut self, e: &Expr) -> Value {
        match &e.kind {
            ExprKind::Int(n) => self.assign_temp(Type::Int, number(*n)),
            ExprKind::Bool(b) => self.assign_temp(Type::Bool, b.to_string()),
            ExprKind::Text(s) => self.assign_temp(
                Type::Text,
                format!("(KText){{{}, {}, false}}", cstring(s), s.len()),
            ),
            ExprKind::List(values) => {
                let list = self.assign_temp(Type::ListInt, "k_list_new()".into());
                for value in values {
                    let v = self.expr(value);
                    self.line(format!("k_list_push(&{}, {});", list.name, v.name));
                }
                list
            }
            ExprKind::Edit(name) => self.env[name].clone(),
            ExprKind::Var(name) => {
                let mut v = self.env[name].clone();
                v.owned = false;
                v
            }
            ExprKind::Move(name) => {
                let v = self.env[name].clone();
                self.assign_temp(v.ty, format!("k_move(&{})", v.name))
            }
            ExprKind::Unary(op, inner) => {
                let v = self.expr(inner);
                if op == "!" {
                    self.assign_temp(Type::Bool, format!("!{}", v.name))
                } else {
                    self.assign_temp(Type::Int, format!("k_sub(0,{}, {})", v.name, e.at))
                }
            }
            ExprKind::Binary(op, left, right) => {
                let a = self.expr(left);
                if op == "&&" || op == "||" {
                    let result = self.slot(Type::Bool, false);
                    self.line(format!("{} = {};", result.name, a.name));
                    self.line(format!(
                        "if ({}{}) {{",
                        if op == "||" { "!" } else { "" },
                        result.name
                    ));
                    let b = self.expr(right);
                    self.line(format!("{} = {};", result.name, b.name));
                    self.line("}");
                    return result;
                }
                let b = self.expr(right);
                if matches!(
                    a.ty,
                    Type::Text
                        | Type::ListInt
                        | Type::OptionInt
                        | Type::ResultIntText
                        | Type::ResultTextText
                ) {
                    let equal = match a.ty {
                        Type::Text => "k_equal",
                        Type::ListInt => "k_list_equal",
                        Type::OptionInt => "k_option_equal",
                        Type::ResultIntText => "k_result_equal",
                        Type::ResultTextText => "k_text_result_equal",
                        _ => unreachable!(),
                    };
                    return self.assign_temp(
                        Type::Bool,
                        format!(
                            "{}{equal}({}, {})",
                            if op == "!=" { "!" } else { "" },
                            a.name,
                            b.name
                        ),
                    );
                }
                if let Some(fun) = match op.as_str() {
                    "+" => Some("k_add"),
                    "-" => Some("k_sub"),
                    "*" => Some("k_mul"),
                    "/" => Some("k_div"),
                    "%" => Some("k_rem"),
                    _ => None,
                } {
                    self.assign_temp(
                        Type::Int,
                        format!("{fun}({}, {}, {})", a.name, b.name, e.at),
                    )
                } else {
                    self.assign_temp(Type::Bool, format!("{} {op} {}", a.name, b.name))
                }
            }
            ExprKind::Call(name, args) if name == "hole" => {
                let ty = self
                    .analysis
                    .holes
                    .iter()
                    .find(|h| h.offset == e.at)
                    .unwrap()
                    .expected;
                self.line(format!("k_fail(\"hole_reached\",{});", e.at));
                if ty == Type::Unit {
                    Value {
                        name: String::new(),
                        ty,
                        owned: false,
                    }
                } else {
                    self.slot(ty, true)
                }
            }
            ExprKind::Call(name, args) if name == "parallel.map" => {
                let values = self.expr(&args[0]);
                let ExprKind::Var(handler) = &args[1].kind else {
                    unreachable!()
                };
                self.assign_temp(
                    Type::ListInt,
                    format!("k_parallel_map({}, f_{handler})", values.name),
                )
            }
            ExprKind::Call(name, args) if name == "http.serve_app" => {
                let port = self.expr(&args[0]);
                let root = self.expr(&args[1]);
                let ExprKind::Var(handler) = &args[2].kind else {
                    unreachable!()
                };
                self.assign_temp(
                    Type::Unit,
                    format!("k_serve_app({}, {}, f_{handler})", port.name, root.name),
                )
            }
            ExprKind::Call(name, args) if name == "http.serve" => {
                let port = self.expr(&args[0]);
                let ExprKind::Var(handler) = &args[1].kind else {
                    unreachable!()
                };
                self.assign_temp(Type::Unit, format!("k_serve({}, f_{handler})", port.name))
            }
            ExprKind::Call(name, args) => {
                let (params, result, target) = if let Some(b) = builtin(name) {
                    let target = match name.as_str() {
                        "result.text_ok" => "k_text_ok",
                        "result.text_err" => "k_text_err",
                        "dotenv.get" => "k_dotenv_get",
                        "json.parse" => "k_json_parse",
                        "json.get" => "k_json_get",
                        "json.text" => "k_json_text",
                        "json.int" => "k_json_int",
                        "json.quote" => "k_json_quote",
                        "csv.get" => "k_csv_get",
                        "xml.text" => "k_xml_text",
                        "sse.data" => "k_sse_data",
                        "fs.read_text" => "k_read_text",
                        "fs.write_text" => "k_write_text",
                        "process.run" => "k_process_run",
                        "process.run_timeout" => "k_process_run_timeout",
                        "process.spawn" => "k_process_spawn",
                        "process.poll" => "k_process_poll",
                        "process.terminate" => "k_process_terminate",
                        "clock.millis" => "k_clock_millis",
                        "env.get" => "k_env_get",
                        "http.get" => "k_http_get",
                        "http.post_json" => "k_http_post_json",
                        "http.post_json_timeout" => "k_http_post_json_timeout",
                        "http.json_response" => "k_json_response",
                        "tcp.exchange" => "k_tcp_exchange",
                        "udp.exchange" => "k_udp_exchange",
                        "websocket.exchange" => "k_websocket_exchange",
                        "text.clone" => "k_clone",
                        "text.concat" => "k_concat",
                        "text.len" => "k_len",
                        "text.from_int" => "k_from_int",
                        "text.parse_int" => "k_parse_int",
                        "list.new" => "k_list_new",
                        "list.clone" => "k_list_clone",
                        "list.len" => "k_list_len",
                        "list.get" => "k_list_get",
                        "list.at" => "k_list_at",
                        "list.contains" => "k_list_contains",
                        "list.push" => "k_list_push",
                        "list.set" => "k_list_set",
                        "option.some" => "k_some",
                        "option.none" => "k_none",
                        "result.ok" => "k_ok",
                        "result.err" => "k_err",
                        "io.println" => "k_println",
                        "http.response" => "k_response",
                        "http.status" => "k_status",
                        "http.body" => "k_body",
                        _ => unreachable!(),
                    };
                    (b.params, b.result, target.to_string())
                } else {
                    let f = self
                        .program
                        .functions
                        .iter()
                        .find(|f| f.name == *name)
                        .unwrap();
                    (
                        f.params.iter().map(|p| (p.ty, p.mode)).collect(),
                        f.result,
                        format!("f_{name}"),
                    )
                };
                let mut values = Vec::new();
                for (arg, (_, mode)) in args.iter().zip(params) {
                    let v = self.expr(arg);
                    // Materialize transfers before evaluating the next argument: C argument order is unspecified.
                    if mode == Mode::Take {
                        let passed = self.slot(v.ty, true);
                        self.line(format!("{} = {};", passed.name, Self::transfer(&v)));
                        values.push((passed, mode));
                    } else {
                        values.push((v, mode));
                    }
                }
                let mut arguments = values
                    .iter()
                    .map(|(v, t)| {
                        if *t == Mode::Take {
                            Self::transfer(v)
                        } else if *t == Mode::Edit {
                            format!("&{}", v.name)
                        } else {
                            v.name.clone()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                if name == "list.at" || name == "list.set" {
                    write!(arguments, ", {}", e.at).unwrap();
                }
                self.assign_temp(result, format!("{target}({arguments})"))
            }
        }
    }
    fn assertion(&mut self, e: &Expr, kind: &str) {
        let begin = self.temporary.len();
        let v = self.expr(e);
        self.line(format!("if (!{}) k_fail(\"{kind}\",{});", v.name, e.at));
        self.drop_temps(begin);
    }
    fn block(&mut self, body: &[Stmt]) {
        let outer = self.env.clone();
        let mut locals = Vec::new();
        for stmt in body {
            let begin = self.temporary.len();
            match &stmt.kind {
                StmtKind::Bind { name, value, .. } => {
                    let v = self.expr(value);
                    let local = self.slot(v.ty, false);
                    self.line(format!("{} = {};", local.name, Self::transfer(&v)));
                    if local.ty.owned() {
                        locals.push(local.name.clone());
                    }
                    self.env.insert(name.clone(), local);
                }
                StmtKind::Assign(name, value) => {
                    let v = self.expr(value);
                    let local = self.env[name].clone();
                    if local.ty.owned() {
                        self.line(format!("k_drop(&{});", local.name));
                    }
                    self.line(format!("{} = {};", local.name, Self::transfer(&v)));
                }
                StmtKind::Return(value) => {
                    if let Some(e) = value {
                        let mut v = self.expr(e);
                        if let ExprKind::Var(name) = &e.kind {
                            v = self.env[name].clone();
                        }
                        if v.ty != Type::Unit {
                            self.line(format!("k_result = {};", Self::transfer(&v)));
                        }
                    }
                    self.line("goto k_cleanup;");
                }
                StmtKind::Assert(e) => self.assertion(e, "assertion_failure"),
                StmtKind::Expr(e) => {
                    self.expr(e);
                }
                StmtKind::If(condition, yes, no) => {
                    let v = self.expr(condition);
                    self.drop_temps(begin);
                    self.line(format!("if ({}) {{", v.name));
                    self.block(yes);
                    self.line("} else {");
                    self.block(no);
                    self.line("}");
                }
                StmtKind::While(condition, body) => {
                    self.line("while (true) {");
                    let v = self.expr(condition);
                    self.drop_temps(begin);
                    self.line(format!("if (!{}) break;", v.name));
                    self.block(body);
                    self.line("}");
                }
                StmtKind::For(name, values, body) => {
                    let list = self.expr(values);
                    let index = self.slot(Type::Int, false);
                    let item = self.slot(Type::Int, false);
                    self.line(format!(
                        "for ({} = 0; {} < k_list_len({}); {}++) {{",
                        index.name, index.name, list.name, index.name
                    ));
                    self.line(format!(
                        "{} = k_list_at({}, {}, {});",
                        item.name, list.name, index.name, stmt.at
                    ));
                    self.env.insert(name.clone(), item);
                    self.block(body);
                    self.env.remove(name);
                    self.line("}");
                }
                StmtKind::Match(value, arms) => {
                    let v = self.expr(value);
                    for (i, arm) in arms.iter().enumerate() {
                        let positive = matches!(arm.variant.as_str(), "Some" | "Ok");
                        let field = if v.ty == Type::OptionInt {
                            "some"
                        } else {
                            "ok"
                        };
                        self.line(format!(
                            "{}if ({}{}.{} ) {{",
                            if i == 0 { "" } else { "else " },
                            if positive { "" } else { "!" },
                            v.name,
                            field
                        ));
                        if let Some(binding) = &arm.binding {
                            self.env.insert(
                                binding.clone(),
                                Value {
                                    name: format!(
                                        "{}.{}",
                                        v.name,
                                        if arm.variant == "Err" {
                                            "error"
                                        } else {
                                            "value"
                                        }
                                    ),
                                    ty: if arm.variant == "Err" || v.ty == Type::ResultTextText {
                                        Type::Text
                                    } else {
                                        Type::Int
                                    },
                                    owned: false,
                                },
                            );
                        }
                        self.block(&arm.body);
                        if let Some(binding) = &arm.binding {
                            self.env.remove(binding);
                        }
                        self.line("}");
                    }
                }
            }
            self.drop_temps(begin);
        }
        for local in locals {
            self.line(format!("k_drop(&{local});"));
        }
        self.env = outer;
    }
    fn finish(mut self, signature: &str, result: Type, ensures: &[Expr]) -> String {
        self.line("k_cleanup:;");
        if result != Type::Unit {
            self.env.insert(
                "result".into(),
                Value {
                    name: "k_result".into(),
                    ty: result,
                    owned: false,
                },
            );
        }
        for e in ensures {
            self.assertion(e, "postcondition_failure");
        }
        for name in self.drops.clone() {
            self.line(format!("k_drop(&{name});"));
        }
        if result == Type::Unit {
            self.line("return;");
        } else {
            self.line("return k_result;");
        }
        format!(
            "{signature} {{\n{}{}{}\n}}\n",
            if result != Type::Unit {
                format!("    {} k_result = {{0}};\n", ctype(result))
            } else {
                String::new()
            },
            self.declarations,
            self.code
        )
    }
}
fn signature(f: &Function) -> String {
    format!(
        "static {} f_{}({})",
        ctype(f.result),
        f.name,
        if f.params.is_empty() {
            "void".into()
        } else {
            f.params
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    format!(
                        "{} {}p{i}",
                        ctype(p.ty),
                        if p.mode == Mode::Edit { "*" } else { "" }
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        }
    )
}
pub fn emit(program: &Program, analysis: &Analysis, tests: bool) -> String {
    let mut out = include_str!("runtime.c").to_string();
    out.push_str(include_str!("stdlib.c"));
    out.push_str(include_str!("process_runtime.c"));
    out.push_str(include_str!("http_app.c"));
    if uses_builtin(analysis, "http.get")
        || uses_builtin(analysis, "http.post_json")
        || uses_builtin(analysis, "http.post_json_timeout")
        || uses_builtin(analysis, "websocket.exchange")
    {
        out.insert_str(0, "#define KEEL_CURL 1\n");
    }
    if uses_builtin(analysis, "xml.text") {
        out.insert_str(0, "#define KEEL_XML 1\n");
    }
    for f in &program.functions {
        writeln!(out, "{};", signature(f)).unwrap();
    }
    for f in &program.functions {
        let mut e = Emitter::new(program, analysis);
        for (i, p) in f.params.iter().enumerate() {
            let name = if p.mode == Mode::Edit {
                format!("(*p{i})")
            } else {
                format!("p{i}")
            };
            if p.ty.owned() && p.mode == Mode::Take {
                e.drops.push(name.clone());
            }
            e.env.insert(
                p.name.clone(),
                Value {
                    name,
                    ty: p.ty,
                    owned: p.mode == Mode::Take,
                },
            );
        }
        for contract in &f.requires {
            e.assertion(contract, "precondition_failure");
        }
        e.block(&f.body);
        out.push_str(&e.finish(&signature(f), f.result, &f.ensures));
    }
    if tests {
        for (i, test) in program.tests.iter().enumerate() {
            let mut e = Emitter::new(program, analysis);
            if let Some((name, _, _)) = &test.generator {
                e.env.insert(
                    name.clone(),
                    Value {
                        name: "k_case_value".into(),
                        ty: Type::Int,
                        owned: false,
                    },
                );
            }
            e.block(&test.body);
            out.push_str(&e.finish(&format!("static void test_{i}(void)"), Type::Unit, &[]));
        }
        out.push_str("int main(int argc, char **argv) {\n k_std_init();\n if(argc!=5) return 64;\n size_t selected=(size_t)strtoull(argv[1],NULL,10);\n uint64_t seed=strtoull(argv[2],NULL,10); if(!seed) seed=1;\n size_t cases=(size_t)strtoull(argv[3],NULL,10);\n switch(selected) {\n");
        for (i, test) in program.tests.iter().enumerate() {
            writeln!(out, "case {i}:").unwrap();
            if let Some((_, min, max)) = &test.generator {
                writeln!(out,"k_has_value=true; if(strcmp(argv[4],\"auto\")) {{ k_case_value=strtoll(argv[4],NULL,10); test_{i}(); }} else {{ for(size_t i=0;i<cases;i++) {{ k_case_value=k_generate(&seed,i,{},{}); test_{i}(); }} }} break;",number(*min),number(*max)).unwrap();
            } else {
                writeln!(out, "test_{i}(); break;").unwrap();
            }
        }
        out.push_str("default: return 64;\n } return 0;\n}\n");
    } else {
        out.push_str("int main(int argc, char **argv) {\n k_std_init();\n for(int i=1;i<argc;i++) { if(!strncmp(argv[i],\"--allow-net=\",12)) k_net_permission=argv[i]+12; else if(!strcmp(argv[i],\"--allow-stdout\")) k_stdout_permission=true; else if(k_std_permission(argv[i])) {} else { fprintf(stderr,\"unknown runtime option: %s\\n\",argv[i]); return 64; } }\n");
        if program.functions.iter().any(|f| f.name == "main") {
            out.push_str("f_main(); return 0;\n}\n");
        } else {
            out.push_str("return 0;\n}\n");
        }
    }
    out
}

// Search the checked call graph, including test bodies recorded by the checker.
pub fn uses_builtin(analysis: &Analysis, name: &str) -> bool {
    analysis.calls.values().any(|calls| calls.contains(name))
}
