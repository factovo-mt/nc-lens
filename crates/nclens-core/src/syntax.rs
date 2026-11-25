use std::collections::HashMap;

pub(crate) use crate::Op;
use crate::{
    AngleUnit, BlockSkipUsage, Control, Diagnostic, Function, MacroGrammar, MacroSyntax, RunResult,
};
#[path = "osp.rs"]
pub(crate) mod osp;
pub(crate) use osp::LOCAL_BASE as OSP_LOCAL_BASE;

pub type Error = String;
pub type Result<T> = std::result::Result<T, Error>;

pub enum Expr {
    Number(f64),
    Var(Box<Expr>),
    VariableIndex(String, Box<Expr>),
    Negative(Box<Expr>),
    Binary(Op, Box<Expr>, Box<Expr>),
    Function(Function, AngleUnit, Box<Expr>, Option<Box<Expr>>),
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

struct Cursor<'a, 'g>(&'a str, &'g MacroGrammar, Option<&'g mut osp::Names>);

impl Cursor<'_, '_> {
    fn advance(&mut self, count: usize) {
        self.0 = &self.0[count..];
        if self.2.is_some() {
            self.0 = self.0.trim_start();
        }
    }
    fn take(&mut self, s: &str) -> bool {
        if !s.is_empty() && self.0.starts_with(s) {
            self.advance(s.len());
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
        self.advance(end);
        Ok((value, raw.contains('.')))
    }

    fn integer(&mut self) -> Result<u32> {
        let (n, point) = self.number()?;
        if point || n > u32::MAX as f64 {
            return Err("Expected an integer".into());
        }
        Ok(n as u32)
    }

    fn require_char(&mut self, ch: char) -> Result<()> {
        self.require(ch.encode_utf8(&mut [0; 4]))
    }

    fn group(&mut self, depth: usize) -> Result<Expr> {
        let [open, close] = self.1.expression;
        self.require_char(open)?;
        let expr = self.expr(0, depth)?;
        self.require_char(close)?;
        Ok(expr)
    }

    fn sign(&mut self) -> Option<Op> {
        let rule = self.1.operator(self.0)?;
        if matches!(rule.operation, Op::Add | Op::Sub) {
            self.advance(rule.symbol.len());
            Some(rule.operation)
        } else {
            None
        }
    }

    fn variable_prefix(&self) -> Option<&str> {
        self.1
            .variables
            .iter()
            .filter(|rule| {
                rule.range.is_some() && !rule.prefix.is_empty() && self.0.starts_with(&rule.prefix)
            })
            .map(|rule| rule.prefix.as_str())
            .max_by_key(|prefix| prefix.len())
    }

    fn variable_index(&mut self, depth: usize) -> Result<Expr> {
        let prefix = self
            .variable_prefix()
            .ok_or("Expected variable prefix")?
            .to_owned();
        self.advance(prefix.len());
        let index = if self.0.starts_with(self.1.expression[0]) {
            self.group(depth)?
        } else if self.variable_prefix().is_some() {
            self.atom(depth)?
        } else {
            Expr::Number(self.number()?.0)
        };
        Ok(Expr::VariableIndex(prefix, Box::new(index)))
    }

    fn atom(&mut self, depth: usize) -> Result<Expr> {
        if depth >= 64 {
            return Err("Expression nesting limit".into());
        }
        if let Some(sign) = self.sign() {
            let value = self.atom(depth + 1)?;
            return Ok(if sign == Op::Sub {
                Expr::Negative(Box::new(value))
            } else {
                value
            });
        }
        if self.0.starts_with(self.1.expression[0]) {
            return self.group(depth + 1);
        }
        if self.0.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
            return Ok(Expr::Number(self.number()?.0));
        }
        let [open, close] = self.1.arguments;
        let function = self
            .1
            .functions
            .iter()
            .filter(|rule| {
                self.0
                    .strip_prefix(&rule.name)
                    .is_some_and(|rest| rest.trim_start().starts_with(open))
            })
            .max_by_key(|rule| rule.name.len());
        if let Some(rule) = function {
            self.advance(rule.name.len());
            self.require_char(open)?;
            let first = self.expr(0, depth + 1)?;
            let second = if self.take(",") {
                let second = self.expr(0, depth + 1)?;
                self.require_char(close)?;
                Some(second)
            } else {
                self.require_char(close)?;
                let split_atan = rule.function == Function::Atan
                    && rule.arity[1] == 2
                    && self.1.operator(self.0).is_some_and(|op| {
                        op.operation == Op::Div
                            && self.0[op.symbol.len()..].trim_start().starts_with(open)
                    });
                if split_atan {
                    self.advance(self.1.operator(self.0).unwrap().symbol.len());
                    self.require_char(open)?;
                    let second = self.expr(0, depth + 1)?;
                    self.require_char(close)?;
                    Some(second)
                } else {
                    None
                }
            };
            let count = if second.is_some() { 2 } else { 1 };
            if !(rule.arity[0]..=rule.arity[1]).contains(&count) {
                return Err(format!("Invalid argument count for {}", rule.name));
            }
            return Ok(Expr::Function(
                rule.function,
                rule.angle_unit,
                Box::new(first),
                second.map(Box::new),
            ));
        }
        if self
            .variable_prefix()
            .is_some_and(|prefix| !prefix.as_bytes()[0].is_ascii_alphabetic())
            || (self.2.is_none() && self.variable_prefix().is_some())
        {
            return Ok(Expr::Var(Box::new(self.variable_index(depth + 1)?)));
        }
        if let Some(names) = self.2.as_mut() {
            let end = self
                .0
                .bytes()
                .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_')
                .count();
            if end != 0 {
                let id = names.variable(&self.0[..end], self.1)?;
                self.advance(end);
                return Ok(Expr::Var(Box::new(Expr::Number(f64::from(id)))));
            }
        }
        Err(format!("Expected expression before {:?}", self.0))
    }

    fn expr(&mut self, min: u16, depth: usize) -> Result<Expr> {
        let mut lhs = self.atom(depth)?;
        for _ in 0..256 {
            let Some(rule) = self.1.operator(self.0) else {
                return Ok(lhs);
            };
            let prec = u16::from(rule.precedence);
            if prec < min {
                return Ok(lhs);
            }
            self.advance(rule.symbol.len());
            let rhs = self.expr(prec + 1, depth + 1)?;
            lhs = Expr::Binary(rule.operation, Box::new(lhs), Box::new(rhs));
        }
        Err("Expression length limit".into())
    }

    fn control(&mut self, statement: Control) -> bool {
        let rule = self
            .1
            .controls
            .iter()
            .filter(|rule| self.0.starts_with(&rule.keyword))
            .max_by_key(|rule| rule.keyword.len());
        rule.is_some_and(|rule| rule.statement == statement && self.take(&rule.keyword))
    }

    fn body(&mut self) -> Result<Body> {
        if self.control(Control::If) {
            let cond = self.group(0)?;
            let branch = if self.control(Control::Goto) {
                Body::Goto(self.expr(0, 0)?)
            } else {
                if !self.control(Control::Then) {
                    return Err(format!("Expected THEN before {:?}", self.0));
                }
                self.assign()?
            };
            return Ok(Body::If(cond, Box::new(branch)));
        }
        if self.control(Control::Goto) {
            return Ok(Body::Goto(self.expr(0, 0)?));
        }
        if self.control(Control::While) {
            let cond = self.group(0)?;
            if !self.control(Control::Do) {
                return Err(format!("Expected DO before {:?}", self.0));
            }
            return Ok(Body::While(cond, self.integer()?));
        }
        if self.control(Control::End) {
            return Ok(Body::End(self.integer()?));
        }
        if self.variable_prefix().is_some() {
            return self.assign();
        }
        let mut words = Vec::new();
        while let Some(addr) = self.0.bytes().next() {
            if self.0.starts_with(&self.1.block_skip) {
                break;
            }
            if !addr.is_ascii_uppercase() {
                return Err(format!("Expected address before {:?}", self.0));
            }
            self.0 = &self.0[1..];
            let original = self.0;
            let sign = self.sign();
            let (value, literal_point) =
                if self.0.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
                    let (n, point) = self.number()?;
                    (
                        Expr::Number(if sign == Some(Op::Sub) { -n } else { n }),
                        Some(point),
                    )
                } else {
                    self.0 = original;
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
        let prefix = self
            .variable_prefix()
            .ok_or("Expected variable prefix")?
            .to_owned();
        self.advance(prefix.len());
        let index = Expr::VariableIndex(prefix, Box::new(self.atom(0)?));
        self.require(&self.1.assignment)?;
        Ok(Body::Assign(index, self.expr(0, 0)?))
    }
}

fn strip(
    mut raw: &str,
    preserve_space: bool,
    grammar: &MacroGrammar,
) -> Result<(String, Option<String>)> {
    let mut depth = 0usize;
    let mut text = String::new();
    let mut comment = None;
    let mut comment_start = "";
    while let Some(c) = raw.chars().next() {
        let rest = &raw[c.len_utf8()..];
        match *grammar.comment_pair.as_slice() {
            [open, _] if c == open => {
                if depth == 0 {
                    comment_start = rest;
                }
                depth += 1;
            }
            [_, close] if c == close && depth != 0 => {
                depth -= 1;
                if depth == 0 && comment.is_none() {
                    comment = Some(
                        comment_start[..comment_start.len() - raw.len()]
                            .trim()
                            .to_owned(),
                    );
                }
            }
            _ if depth != 0 => {}
            _ if grammar.line_comments.iter().any(|symbol| {
                raw.get(..symbol.len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(symbol))
            }) =>
            {
                break
            }
            _ if (preserve_space || !c.is_whitespace()) && c != '\u{feff}' => {
                text.push(c.to_ascii_uppercase());
            }
            _ => {}
        }
        raw = rest;
    }
    if depth == 0 {
        Ok((text, comment))
    } else {
        Err("Unclosed comment".into())
    }
}

pub fn parse(
    src: &str,
    program_address: char,
    sequence_address: char,
    grammar: &MacroGrammar,
    output: &mut RunResult,
) -> Vec<Program> {
    let mut programs = vec![Program::default()];
    let osp = grammar.syntax == MacroSyntax::Osp;
    let mut names = osp::Names::default();
    let mut skip_counts = [0; 9];
    let normalized = src.replace("\r\n", "\n");
    for (i, raw) in normalized.split(['\n', '\r']).enumerate() {
        let line = i + 1;
        let result = (|| {
            let (clean, comment) = strip(raw, osp, grammar)?;
            let clean = clean.trim();
            let mut cursor = Cursor(clean, grammar, None);
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
                clean.split(&grammar.block_skip).next().unwrap()
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
            while cursor.take(&grammar.block_skip) {
                let level = match cursor.0.as_bytes().first() {
                    Some(b'1'..=b'9')
                        if !cursor.0.as_bytes().get(1).is_some_and(u8::is_ascii_digit) =>
                    {
                        let level = cursor.0.as_bytes()[0] - b'0';
                        cursor.0 = cursor.0[1..].trim_start();
                        level
                    }
                    Some(b'0'..=b'9') => {
                        return Err(format!(
                            "Invalid block skip level (expected {marker}1 through {marker}9)",
                            marker = grammar.block_skip
                        ));
                    }
                    _ if skip != 0 || cursor.0.trim_start().starts_with(&grammar.block_skip) => {
                        return Err(format!(
                            "Use {}1 explicitly when combining block skip levels",
                            grammar.block_skip
                        ));
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
                names.body(cursor.0, program_address, sequence_address, grammar)?
            } else {
                let body = cursor.body()?;
                if cursor.0.starts_with(&grammar.block_skip) {
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
            let comment = comment.filter(|_| matches!(body, Body::Assign(..) | Body::If(..)));
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

pub(crate) fn parse_replacement(src: &str, grammar: &MacroGrammar) -> Result<Vec<Word>> {
    let (clean, _) = strip(src, false, grammar)?;
    if src.contains(['\r', '\n']) {
        return Err("Replacement must be a single block".into());
    }
    let mut cursor = Cursor(&clean, grammar, None);
    match cursor.body()? {
        Body::Words(words) if cursor.0.is_empty() => Ok(words),
        _ => Err("Replacement must be an NC word list".into()),
    }
}
