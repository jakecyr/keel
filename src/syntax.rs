use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Diagnostic {
    pub kind: String,
    pub message: String,
    pub offset: usize,
    pub line: usize,
    pub column: usize,
}
impl Diagnostic {
    pub fn new(source: &str, offset: usize, kind: &str, message: impl Into<String>) -> Self {
        let prefix = &source[..offset.min(source.len())];
        Self {
            kind: kind.into(),
            message: message.into(),
            offset,
            line: prefix.bytes().filter(|b| *b == b'\n').count() + 1,
            column: prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1,
        }
    }
}
pub type DResult<T> = Result<T, Diagnostic>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Type {
    Int,
    Bool,
    Text,
    ListInt,
    OptionInt,
    ResultIntText,
    Unit,
}
impl Type {
    pub fn owned(self) -> bool {
        matches!(self, Self::Text | Self::ListInt | Self::ResultIntText)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Mode {
    Value,
    Read,
    Take,
    Edit,
}
#[derive(Clone, Debug, Serialize)]
pub struct Param {
    pub name: String,
    pub ty: Type,
    pub mode: Mode,
}
#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub at: usize,
}
#[derive(Clone, Debug)]
pub enum ExprKind {
    Int(i64),
    Bool(bool),
    Text(String),
    Var(String),
    Move(String),
    Edit(String),
    List(Vec<Expr>),
    Call(String, Vec<Expr>),
    Unary(String, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
}
#[derive(Clone, Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub at: usize,
}
#[derive(Clone, Debug)]
pub enum StmtKind {
    Bind {
        mutable: bool,
        name: String,
        annotation: Option<Type>,
        value: Expr,
    },
    Assign(String, Expr),
    Return(Option<Expr>),
    Assert(Expr),
    Expr(Expr),
    If(Expr, Vec<Stmt>, Vec<Stmt>),
    While(Expr, Vec<Stmt>),
    For(String, Expr, Vec<Stmt>),
    Match(Expr, Vec<MatchArm>),
}
#[derive(Clone, Debug)]
pub struct MatchArm {
    pub variant: String,
    pub binding: Option<String>,
    pub body: Vec<Stmt>,
}
#[derive(Clone, Debug)]
pub struct Function {
    pub name: String,
    pub public: bool,
    pub params: Vec<Param>,
    pub result: Type,
    pub effects: Vec<String>,
    pub requires: Vec<Expr>,
    pub ensures: Vec<Expr>,
    pub body: Vec<Stmt>,
    pub start: usize,
    pub body_start: usize,
    pub end: usize,
}
#[derive(Clone, Debug)]
pub struct Test {
    pub name: String,
    pub generator: Option<(String, i64, i64)>,
    pub body: Vec<Stmt>,
    pub at: usize,
}
#[derive(Clone, Debug)]
pub struct Program {
    pub functions: Vec<Function>,
    pub tests: Vec<Test>,
}
#[derive(Clone, Debug, PartialEq)]
enum TokenKind {
    Id(String),
    Num(u64),
    Str(String),
    Symbol(String),
    Eof,
}
#[derive(Clone, Debug)]
struct Token {
    kind: TokenKind,
    start: usize,
    end: usize,
}

fn lex(source: &str) -> DResult<Vec<Token>> {
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if source[i..].starts_with("//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let start = i;
        let kind = if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            TokenKind::Id(source[start..i].into())
        } else if bytes[i].is_ascii_digit() {
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            TokenKind::Num(source[start..i].parse().map_err(|_| {
                Diagnostic::new(
                    source,
                    start,
                    "integer_literal",
                    "integer literal exceeds Int range",
                )
            })?)
        } else if bytes[i] == b'"' {
            i += 1;
            let mut value = String::new();
            let mut closed = false;
            while i < bytes.len() {
                let c = source[i..].chars().next().unwrap();
                i += c.len_utf8();
                if c == '"' {
                    closed = true;
                    break;
                }
                if c == '\\' {
                    let e = *bytes.get(i).ok_or_else(|| {
                        Diagnostic::new(source, start, "syntax", "unterminated escape")
                    })?;
                    i += 1;
                    value.push(match e {
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'"' => '"',
                        b'\\' => '\\',
                        _ => {
                            return Err(Diagnostic::new(
                                source,
                                i - 1,
                                "syntax",
                                "unsupported string escape",
                            ));
                        }
                    });
                } else {
                    value.push(c);
                }
            }
            if !closed {
                return Err(Diagnostic::new(
                    source,
                    start,
                    "syntax",
                    "unterminated string",
                ));
            }
            TokenKind::Str(value)
        } else {
            let pair = ["->", "=>", "==", "!=", "<=", ">=", "&&", "||"]
                .into_iter()
                .find(|s| source[i..].starts_with(s));
            if let Some(p) = pair {
                i += 2;
                TokenKind::Symbol(p.into())
            } else if b"{}[]():,;.+-*/%<>=!".contains(&bytes[i]) {
                i += 1;
                TokenKind::Symbol(source[start..i].into())
            } else {
                return Err(Diagnostic::new(source, i, "syntax", "unexpected character"));
            }
        };
        out.push(Token {
            kind,
            start,
            end: i,
        });
    }
    out.push(Token {
        kind: TokenKind::Eof,
        start: i,
        end: i,
    });
    Ok(out)
}

pub fn parse(source: &str) -> DResult<Program> {
    if source.len() > 4 * 1024 * 1024 {
        return Err(Diagnostic::new(
            source,
            0,
            "resource_limit",
            "source exceeds 4 MiB limit",
        ));
    }
    Parser {
        source,
        tokens: lex(source)?,
        pos: 0,
        depth: 0,
    }
    .program()
}
struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    pos: usize,
    depth: usize,
}
impl Parser<'_> {
    fn current(&self) -> &Token {
        &self.tokens[self.pos]
    }
    fn at(&self, value: &str) -> bool {
        matches!(&self.current().kind, TokenKind::Id(s) | TokenKind::Symbol(s) if s == value)
    }
    fn eat(&mut self, value: &str) -> bool {
        if self.at(value) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn error<T>(&self, message: impl Into<String>) -> DResult<T> {
        Err(Diagnostic::new(
            self.source,
            self.current().start,
            "syntax",
            message,
        ))
    }
    fn expect(&mut self, value: &str) -> DResult<()> {
        if self.eat(value) {
            Ok(())
        } else {
            self.error(format!("expected '{value}'"))
        }
    }
    fn ident(&mut self) -> DResult<String> {
        if let TokenKind::Id(s) = self.current().kind.clone() {
            self.pos += 1;
            Ok(s)
        } else {
            self.error("expected identifier")
        }
    }
    fn string(&mut self) -> DResult<String> {
        if let TokenKind::Str(s) = self.current().kind.clone() {
            self.pos += 1;
            Ok(s)
        } else {
            self.error("expected quoted string")
        }
    }
    fn ty(&mut self) -> DResult<Type> {
        match self.ident()?.as_str() {
            "Int" => Ok(Type::Int),
            "Bool" => Ok(Type::Bool),
            "Text" => Ok(Type::Text),
            "Unit" => Ok(Type::Unit),
            "List" => {
                self.expect("<")?;
                self.expect("Int")?;
                self.expect(">")?;
                Ok(Type::ListInt)
            }
            "Option" => {
                self.expect("<")?;
                self.expect("Int")?;
                self.expect(">")?;
                Ok(Type::OptionInt)
            }
            "Result" => {
                self.expect("<")?;
                self.expect("Int")?;
                self.expect(",")?;
                self.expect("Text")?;
                self.expect(">")?;
                Ok(Type::ResultIntText)
            }
            _ => self.error(
                "supported types: Int, Bool, Text, Unit, List<Int>, Option<Int>, Result<Int, Text>",
            ),
        }
    }
    fn path(&mut self) -> DResult<String> {
        let mut s = self.ident()?;
        while self.eat(".") {
            s.push('.');
            s.push_str(&self.ident()?);
        }
        Ok(s)
    }
    fn integer(&mut self) -> DResult<i64> {
        let negative = self.eat("-");
        if let TokenKind::Num(n) = self.current().kind {
            self.pos += 1;
            let value = if negative { -(n as i128) } else { n as i128 };
            i64::try_from(value).map_err(|_| {
                Diagnostic::new(
                    self.source,
                    self.tokens[self.pos - 1].start,
                    "integer_literal",
                    "integer literal exceeds Int range",
                )
            })
        } else {
            self.error("expected integer bound")
        }
    }
    fn program(&mut self) -> DResult<Program> {
        let mut functions = Vec::new();
        let mut tests = Vec::new();
        while self.current().kind != TokenKind::Eof {
            let start = self.current().start;
            let public = self.eat("pub");
            if self.eat("fn") {
                let name = self.ident()?;
                self.expect("(")?;
                let mut params = Vec::new();
                if !self.at(")") {
                    loop {
                        let name = self.ident()?;
                        self.expect(":")?;
                        let mode = if self.eat("read") {
                            Mode::Read
                        } else if self.eat("take") {
                            Mode::Take
                        } else if self.eat("edit") {
                            Mode::Edit
                        } else {
                            Mode::Value
                        };
                        let ty = self.ty()?;
                        params.push(Param { name, mode, ty });
                        if !self.eat(",") {
                            break;
                        }
                    }
                }
                self.expect(")")?;
                let result = if self.eat("->") {
                    self.ty()?
                } else {
                    Type::Unit
                };
                let mut effects = Vec::new();
                let mut requires = Vec::new();
                let mut ensures = Vec::new();
                loop {
                    if self.eat("effects") {
                        self.expect("{")?;
                        if !self.at("}") {
                            loop {
                                effects.push(self.path()?);
                                if !self.eat(",") {
                                    break;
                                }
                            }
                        }
                        self.expect("}")?;
                    } else if self.eat("requires") {
                        requires.push(self.expr(0)?);
                    } else if self.eat("ensures") {
                        ensures.push(self.expr(0)?);
                    } else {
                        break;
                    }
                }
                let body_start = self.current().start;
                let body = self.block()?;
                let end = self.tokens[self.pos - 1].end;
                functions.push(Function {
                    name,
                    public,
                    params,
                    result,
                    effects,
                    requires,
                    ensures,
                    body,
                    start,
                    body_start,
                    end,
                });
            } else if !public && (self.at("test") || self.at("property")) {
                let property = self.eat("property");
                if !property {
                    self.expect("test")?;
                }
                let name = self.string()?;
                let generator = if property {
                    self.expect("(")?;
                    let name = self.ident()?;
                    self.expect("in")?;
                    if self.path()? != "gen.int" {
                        return self.error("v0 properties require gen.int");
                    }
                    self.expect("(")?;
                    self.expect("min")?;
                    self.expect(":")?;
                    let min = self.integer()?;
                    self.expect(",")?;
                    self.expect("max")?;
                    self.expect(":")?;
                    let max = self.integer()?;
                    self.expect(")")?;
                    self.expect(")")?;
                    if min > max {
                        return self.error("generator minimum exceeds maximum");
                    }
                    Some((name, min, max))
                } else {
                    None
                };
                tests.push(Test {
                    name,
                    generator,
                    body: self.block()?,
                    at: start,
                });
            } else {
                return self.error("expected fn, test, or property");
            }
        }
        Ok(Program { functions, tests })
    }
    fn block(&mut self) -> DResult<Vec<Stmt>> {
        self.enter()?;
        let result = self.block_inner();
        self.depth -= 1;
        result
    }
    fn enter(&mut self) -> DResult<()> {
        if self.depth >= 64 {
            return Err(Diagnostic::new(
                self.source,
                self.current().start,
                "resource_limit",
                "syntax nesting exceeds 64 levels",
            ));
        }
        self.depth += 1;
        Ok(())
    }
    fn block_inner(&mut self) -> DResult<Vec<Stmt>> {
        self.expect("{")?;
        let mut out = Vec::new();
        while !self.eat("}") {
            if self.current().kind == TokenKind::Eof {
                return self.error("unclosed block");
            }
            let at = self.current().start;
            let kind = if self.at("let") || self.at("var") {
                let mutable = self.eat("var");
                if !mutable {
                    self.expect("let")?;
                }
                let name = self.ident()?;
                let annotation = if self.eat(":") {
                    Some(self.ty()?)
                } else {
                    None
                };
                self.expect("=")?;
                StmtKind::Bind {
                    mutable,
                    name,
                    annotation,
                    value: self.expr(0)?,
                }
            } else if self.eat("return") {
                StmtKind::Return(if self.at("}") || self.at(";") {
                    None
                } else {
                    Some(self.expr(0)?)
                })
            } else if self.eat("assert") {
                StmtKind::Assert(self.expr(0)?)
            } else if self.eat("if") {
                let condition = self.expr(0)?;
                let yes = self.block()?;
                let no = if self.eat("else") {
                    self.block()?
                } else {
                    Vec::new()
                };
                StmtKind::If(condition, yes, no)
            } else if self.eat("while") {
                let condition = self.expr(0)?;
                StmtKind::While(condition, self.block()?)
            } else if self.eat("for") {
                let name = self.ident()?;
                self.expect("in")?;
                let values = self.expr(0)?;
                StmtKind::For(name, values, self.block()?)
            } else if self.eat("match") {
                let value = self.expr(0)?;
                self.expect("{")?;
                let mut arms = Vec::new();
                while !self.eat("}") {
                    let variant = self.ident()?;
                    let binding = if self.eat("(") {
                        let name = self.ident()?;
                        self.expect(")")?;
                        Some(name)
                    } else {
                        None
                    };
                    self.expect("=>")?;
                    arms.push(MatchArm {
                        variant,
                        binding,
                        body: self.block()?,
                    });
                    self.eat(",");
                }
                StmtKind::Match(value, arms)
            } else {
                let expr = self.expr(0)?;
                if self.eat("=") {
                    if let ExprKind::Var(name) = expr.kind {
                        StmtKind::Assign(name, self.expr(0)?)
                    } else {
                        return self.error("assignment needs a variable");
                    }
                } else {
                    StmtKind::Expr(expr)
                }
            };
            self.eat(";");
            out.push(Stmt { kind, at });
        }
        Ok(out)
    }
    fn expr(&mut self, min: u8) -> DResult<Expr> {
        self.enter()?;
        let result = self.expr_inner(min);
        self.depth -= 1;
        result
    }
    fn expr_inner(&mut self, min: u8) -> DResult<Expr> {
        let at = self.current().start;
        let mut lhs = if self.eat("-") {
            if matches!(self.current().kind, TokenKind::Num(9223372036854775808)) {
                self.pos += 1;
                Expr {
                    at,
                    kind: ExprKind::Int(i64::MIN),
                }
            } else {
                Expr {
                    at,
                    kind: ExprKind::Unary("-".into(), Box::new(self.expr(7)?)),
                }
            }
        } else if self.eat("!") {
            Expr {
                at,
                kind: ExprKind::Unary("!".into(), Box::new(self.expr(7)?)),
            }
        } else if self.eat("[") {
            let mut values = Vec::new();
            if !self.at("]") {
                loop {
                    values.push(self.expr(0)?);
                    if !self.eat(",") {
                        break;
                    }
                }
            }
            self.expect("]")?;
            Expr {
                at,
                kind: ExprKind::List(values),
            }
        } else if self.eat("edit") {
            Expr {
                at,
                kind: ExprKind::Edit(self.ident()?),
            }
        } else if self.eat("take") {
            Expr {
                at,
                kind: ExprKind::Move(self.ident()?),
            }
        } else if self.eat("(") {
            let e = self.expr(0)?;
            self.expect(")")?;
            e
        } else {
            match self.current().kind.clone() {
                TokenKind::Num(n) => {
                    self.pos += 1;
                    Expr {
                        at,
                        kind: ExprKind::Int(i64::try_from(n).map_err(|_| {
                            Diagnostic::new(
                                self.source,
                                at,
                                "integer_literal",
                                "integer literal exceeds Int range",
                            )
                        })?),
                    }
                }
                TokenKind::Str(s) => {
                    self.pos += 1;
                    Expr {
                        at,
                        kind: ExprKind::Text(s),
                    }
                }
                TokenKind::Id(s) if s == "true" || s == "false" => {
                    self.pos += 1;
                    Expr {
                        at,
                        kind: ExprKind::Bool(s == "true"),
                    }
                }
                TokenKind::Id(_) => {
                    let name = self.path()?;
                    let kind = if self.eat("(") {
                        let mut args = Vec::new();
                        if !self.at(")") {
                            loop {
                                args.push(self.expr(0)?);
                                if !self.eat(",") {
                                    break;
                                }
                            }
                        }
                        self.expect(")")?;
                        ExprKind::Call(name, args)
                    } else {
                        ExprKind::Var(name)
                    };
                    Expr { at, kind }
                }
                _ => return self.error("expected expression"),
            }
        };
        let mut operators = 0;
        while let TokenKind::Symbol(symbol) = &self.current().kind {
            let op = symbol.clone();
            let prec = match op.as_str() {
                "||" => 1,
                "&&" => 2,
                "==" | "!=" => 3,
                "<" | "<=" | ">" | ">=" => 4,
                "+" | "-" => 5,
                "*" | "/" | "%" => 6,
                _ => break,
            };
            if prec < min {
                break;
            }
            operators += 1;
            if operators > 64 {
                return Err(Diagnostic::new(
                    self.source,
                    at,
                    "resource_limit",
                    "expression exceeds 64 operators; split into local bindings",
                ));
            }
            self.pos += 1;
            let rhs = self.expr(prec + 1)?;
            lhs = Expr {
                at,
                kind: ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)),
            };
        }
        Ok(lhs)
    }
}
