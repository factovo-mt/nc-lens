use std::collections::HashMap;

use crate::{BlockSkipUsage, Diagnostic, MacroSyntax, RunResult};
#[path = "osp.rs"]
pub(crate) mod osp;
pub(crate) use osp::{COMMON_BASE as OSP_COMMON_BASE, LOCAL_BASE as OSP_LOCAL_BASE};

pub type Error = String;
pub type Result<T> = std::result::Result<T, Error>;

pub enum Expr {
    Number(f64),
    Var(Box<Expr>),
    Negative(Box<Expr>),
    Binary(Op, Box<Expr>, Box<Expr>),
    Function(String, Box<Expr>, Option<Box<Expr>>),
}

#[derive(Clone, Copy)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Xor,
}

pub struct Word {
    pub addr: u8,
    pub value: Expr,
    pub literal_point: Option<bool>,
}

pub enum Body {
    Words(Vec<Word>),
    Assign(Expr, Expr),
    If(Expr, Box<Body>),
    Goto(Expr),
    While(Expr, u32),
    End(u32),
    OspCall {
        target: u32,
        name: String,
        modal: bool,
        args: Vec<(u32, Expr)>,
    },
    OspReturn,
    OspModalCancel,
}

pub struct Block {
    pub line: usize,
    pub skip: u16,
    pub label: Option<u32>,
    pub comment: Option<String>,
    pub body: Body,
}

#[derive(Default)]
pub struct Program {
    pub number: u32,
    pub header: Option<usize>,
    pub name: String,
    pub blocks: Vec<Block>,
    pub labels: HashMap<u32, usize>,
    pub loops: HashMap<usize, usize>,
    open: Vec<(usize, u32)>,
}

struct Cursor<'a>(&'a str);

impl Cursor<'_> {
    fn take(&mut self, s: &str) -> bool {
        if let Some(rest) = self.0.strip_prefix(s) {
            self.0 = rest;
            true
        } else {
            false
        }
    }

    fn require(&mut self, s: &str) -> Result<()> {
        if self.take(s) {
            Ok(())
        } else {
            Err(format!("Expected {s} before {:?}", self.0))
        }
    }

    fn number(&mut self) -> Result<(f64, bool)> {
        let end = self
            .0
            .bytes()
            .take_while(|c| c.is_ascii_digit() || *c == b'.')
            .count();
        let raw = &self.0[..end];
        let value = raw
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| format!("Invalid number before {:?}", self.0))?;
        self.0 = &self.0[end..];
        Ok((value, raw.contains('.')))
    }

    fn integer(&mut self) -> Result<u32> {
        let (n, point) = self.number()?;
        if point || n > u32::MAX as f64 {
            return Err("Expected an integer".into());
        }
        Ok(n as u32)
    }

    fn group(&mut self, depth: usize) -> Result<Expr> {
        self.require("[")?;
        let expr = self.expr(0, depth)?;
        self.require("]")?;
        Ok(expr)
    }

    fn atom(&mut self, depth: usize) -> Result<Expr> {
        if depth >= 64 {
            return Err("Expression nesting limit".into());
        }
        if self.take("+") {
            return self.atom(depth + 1);
        }
        if self.take("-") {
            return Ok(Expr::Negative(Box::new(self.atom(depth + 1)?)));
        }
        if self.take("#") {
            let index = if self.0.starts_with('[') {
                self.group(depth + 1)?
            } else if self.0.starts_with('#') {
                self.atom(depth + 1)?
            } else {
                Expr::Number(self.number()?.0)
            };
            return Ok(Expr::Var(Box::new(index)));
        }
        if self.0.starts_with('[') {
            return self.group(depth + 1);
        }
        if self.0.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
            return Ok(Expr::Number(self.number()?.0));
        }
        for name in [
            "ASIN", "ACOS", "ATAN", "SQRT", "ROUND", "SIN", "COS", "TAN", "ABS", "FIX", "FUP",
            "LN", "EXP", "BIN", "BCD", "ADP",
        ] {
            if self.take(name) {
                self.require("[")?;
                let a = self.expr(0, depth + 1)?;
                let second = if name == "ATAN" && self.take(",") {
                    let b = self.expr(0, depth + 1)?;
                    self.require("]")?;
                    Some(b)
                } else {
                    self.require("]")?;
                    if name == "ATAN" && self.take("/[") {
                        let b = self.expr(0, depth + 1)?;
                        self.require("]")?;
                        Some(b)
                    } else {
                        None
                    }
                };
                return Ok(Expr::Function(
                    name.into(),
                    Box::new(a),
                    second.map(Box::new),
                ));
            }
        }
        Err(format!("Expected expression before {:?}", self.0))
    }

    fn expr(&mut self, min: u8, depth: usize) -> Result<Expr> {
        let mut lhs = self.atom(depth)?;
        for _ in 0..256 {
            let operators = [
                ("OR", Op::Or, 1),
                ("XOR", Op::Xor, 1),
                ("AND", Op::And, 2),
                ("EQ", Op::Eq, 3),
                ("NE", Op::Ne, 3),
                ("LE", Op::Le, 3),
                ("LT", Op::Lt, 3),
                ("GE", Op::Ge, 3),
                ("GT", Op::Gt, 3),
                ("+", Op::Add, 4),
                ("-", Op::Sub, 4),
                ("*", Op::Mul, 5),
                ("/", Op::Div, 5),
                ("MOD", Op::Mod, 5),
            ];
            let op = operators
                .into_iter()
                .find(|(s, _, prec)| *prec >= min && self.0.starts_with(s));
            let Some((s, op, prec)) = op else {
                return Ok(lhs);
            };
            self.take(s);
            let rhs = self.expr(prec + 1, depth + 1)?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Err("Expression length limit".into())
    }

    fn body(&mut self) -> Result<Body> {
        if self.take("IF") {
            let cond = self.group(0)?;
            let branch = if self.take("GOTO") {
                Body::Goto(self.expr(0, 0)?)
            } else {
                self.require("THEN")?;
                self.assign()?
            };
            return Ok(Body::If(cond, Box::new(branch)));
        }
        if self.take("GOTO") {
            return Ok(Body::Goto(self.expr(0, 0)?));
        }
        if self.take("WHILE") {
            let cond = self.group(0)?;
            self.require("DO")?;
            return Ok(Body::While(cond, self.integer()?));
        }
        if self.take("END") {
            return Ok(Body::End(self.integer()?));
        }
        if self.0.starts_with('#') {
            return self.assign();
        }
        let mut words = Vec::new();
        while let Some(addr) = self.0.bytes().next() {
            if addr == b'/' {
                break;
            }
            if !addr.is_ascii_uppercase() {
                return Err(format!("Expected address before {:?}", self.0));
            }
            self.0 = &self.0[1..];
            let unsigned = self.0.trim_start_matches(['+', '-']);
            let (value, literal_point) =
                if unsigned.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
                    let negative = self.take("-");
                    if !negative {
                        self.take("+");
                    }
                    let (n, point) = self.number()?;
                    (Expr::Number(if negative { -n } else { n }), Some(point))
                } else {
                    (self.atom(0)?, None)
                };
            words.push(Word {
                addr,
                value,
                literal_point,
            });
        }
        Ok(Body::Words(words))
    }

    fn assign(&mut self) -> Result<Body> {
        self.require("#")?;
        let index = self.atom(0)?;
        self.require("=")?;
        Ok(Body::Assign(index, self.expr(0, 0)?))
    }
}

fn strip(raw: &str, preserve_space: bool) -> Result<String> {
    let mut depth = 0usize;
    let mut text = String::new();
    for c in raw.chars() {
        match c {
            '(' => depth += 1,
            ')' if depth != 0 => depth -= 1,
            ';' if depth == 0 => break,
            _ if depth == 0 && (preserve_space || !c.is_whitespace()) && c != '\u{feff}' => {
                text.push(c.to_ascii_uppercase())
            }
            _ => {}
        }
    }
    if depth == 0 {
        Ok(text)
    } else {
        Err("Unclosed comment".into())
    }
}

pub fn parse(
    src: &str,
    program_address: char,
    sequence_address: char,
    macro_syntax: &MacroSyntax,
    output: &mut RunResult,
) -> Vec<Program> {
    let mut programs = vec![Program::default()];
    let osp = *macro_syntax == MacroSyntax::Osp;
    let mut names = osp::Names::default();
    let mut skip_counts = [0; 9];
    let normalized = src.replace("\r\n", "\n");
    for (i, raw) in normalized.split(['\n', '\r']).enumerate() {
        let line = i + 1;
        let result = (|| {
            let clean = strip(raw, osp)?;
            let clean = clean.trim();
            let mut cursor = Cursor(clean);
            if clean.is_empty() || clean == "%" {
                return Ok(());
            }
            let mut warn_trailing_skip = || {
                output.warnings.push(Diagnostic {
                    src_line: line,
                    message: "Block skip must be at the start of the block; ignored trailing text"
                        .into(),
                });
            };
            let header_text = if osp {
                clean
            } else {
                clean.split('/').next().unwrap()
            };
            let header = header_text.strip_prefix(program_address).filter(|suffix| {
                if osp {
                    !suffix.trim().is_empty()
                        && suffix.trim().bytes().all(|b| b.is_ascii_alphanumeric())
                } else {
                    program_address != 'L' || suffix.bytes().all(|b| b.is_ascii_digit())
                }
            });
            if let Some(suffix) = header {
                let (number, name) = if osp {
                    (
                        names.program(suffix.trim())?,
                        format!("{program_address}{}", suffix.trim()),
                    )
                } else {
                    cursor.0 = suffix;
                    let number = cursor.integer()?;
                    if !cursor.0.is_empty() {
                        return Err("Unexpected text after program number".into());
                    }
                    (number, format!("{program_address}{number:04}"))
                };
                if programs
                    .iter()
                    .any(|p| p.header.is_some() && p.number == number)
                {
                    return Err(format!("Duplicate program O{number}"));
                }
                if programs.len() == 1
                    && programs[0].header.is_none()
                    && programs[0].blocks.is_empty()
                {
                    programs.clear();
                }
                if header_text != clean {
                    warn_trailing_skip();
                }
                programs.push(Program {
                    number,
                    name,
                    header: Some(line),
                    ..Program::default()
                });
                return Ok(());
            }
            let mut skip = 0;
            while cursor.take("/") {
                let level = match cursor.0.as_bytes().first() {
                    Some(b'1'..=b'9')
                        if !cursor.0.as_bytes().get(1).is_some_and(u8::is_ascii_digit) =>
                    {
                        let level = cursor.0.as_bytes()[0] - b'0';
                        cursor.0 = cursor.0[1..].trim_start();
                        level
                    }
                    Some(b'0'..=b'9') => {
                        return Err("Invalid block skip level (expected /1 through /9)".into())
                    }
                    _ if skip != 0 || cursor.0.trim_start().starts_with('/') => {
                        return Err("Use /1 explicitly when combining block skip levels".into());
                    }
                    _ => 1,
                };
                skip |= 1u16 << (level - 1);
                cursor.0 = cursor.0.trim_start();
            }
            for (index, count) in skip_counts.iter_mut().enumerate() {
                if skip & (1 << index) != 0 {
                    *count += 1;
                }
            }
            let label = if cursor.take(sequence_address.encode_utf8(&mut [0; 4])) {
                if osp {
                    let end = cursor.0.find(char::is_whitespace).unwrap_or(cursor.0.len());
                    let label = names.label(&cursor.0[..end])?;
                    cursor.0 = cursor.0[end..].trim_start();
                    Some(label)
                } else {
                    Some(cursor.integer()?)
                }
            } else {
                None
            };
            let body = if osp {
                names.body(cursor.0, program_address, sequence_address)?
            } else {
                let body = cursor.body()?;
                if cursor.0.starts_with('/') {
                    warn_trailing_skip();
                } else if !cursor.0.is_empty() {
                    return Err(format!("Unexpected text {:?}", cursor.0));
                }
                body
            };

            let p = programs.last_mut().unwrap();
            let pc = p.blocks.len();
            if let Some(n) = label {
                p.labels.entry(n).or_insert(pc);
            }
            let loop_error = match body {
                Body::While(_, n)
                    if (1..=3).contains(&n) && !p.open.iter().any(|&(_, id)| id == n) =>
                {
                    p.open.push((pc, n));
                    None
                }
                Body::While(_, _) => Some("Invalid or nested duplicate DO label"),
                Body::End(n) if p.open.last().is_some_and(|&(_, id)| id == n) => {
                    let (start, _) = p.open.pop().unwrap();
                    p.loops.insert(start, pc);
                    p.loops.insert(pc, start);
                    None
                }
                Body::End(_) => Some("END without matching DO"),
                _ => None,
            };
            let comment = if matches!(body, Body::Assign(..) | Body::If(..)) {
                raw.split_once('(')
                    .and_then(|(_, tail)| tail.split_once(')'))
                    .map(|(comment, _)| comment.trim().to_owned())
            } else {
                None
            };
            p.blocks.push(Block {
                line,
                skip,
                label,
                comment,
                body,
            });
            loop_error.map_or(Ok(()), |e| Err(e.into()))
        })();
        if let Err(message) = result {
            output.diagnostics.push(Diagnostic {
                src_line: line,
                message,
            });
        }
    }
    output.block_skip_usage = skip_counts
        .into_iter()
        .enumerate()
        .filter(|&(_, count)| count > 0)
        .map(|(index, line_count)| BlockSkipUsage {
            level: index as u8 + 1,
            line_count,
        })
        .collect();
    for p in &programs {
        for &(pc, _) in &p.open {
            output.diagnostics.push(Diagnostic {
                src_line: p.blocks[pc].line,
                message: "DO without matching END".into(),
            });
        }
    }
    programs
}

pub(crate) fn parse_replacement(src: &str) -> Result<Vec<Word>> {
    let clean = strip(src, false)?;
    if src.contains(['\r', '\n']) {
        return Err("Replacement must be a single block".into());
    }
    let mut cursor = Cursor(&clean);
    match cursor.body()? {
        Body::Words(words) if cursor.0.is_empty() => Ok(words),
        _ => Err("Replacement must be an NC word list".into()),
    }
}
