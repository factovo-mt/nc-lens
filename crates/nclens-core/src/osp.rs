//! Basic OSP-P200M/P20M User Task syntax (Okuma manual 5228-E, sections 10–11).
use super::{Body, Cursor, Expr, Result};
use crate::{Control, MacroGrammar, VariableKind};
use std::collections::HashMap;

pub(crate) const LOCAL_BASE: u32 = 2_000_000_000;

#[derive(Default)]
pub(super) struct Names {
    programs: HashMap<String, u32>,
    labels: HashMap<String, u32>,
    locals: HashMap<String, u32>,
}

fn identifier(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_alphabetic())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn intern(map: &mut HashMap<String, u32>, name: &str, base: u32) -> u32 {
    let next = base + map.len() as u32;
    *map.entry(name.to_owned()).or_insert(next)
}

impl Names {
    pub(super) fn program(&mut self, name: &str) -> Result<u32> {
        if let Ok(number) = name.parse::<u32>() {
            return Ok(number);
        }
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(format!("Invalid program name {name}"));
        }
        Ok(intern(&mut self.programs, name, LOCAL_BASE))
    }

    pub(super) fn label(&mut self, name: &str) -> Result<u32> {
        if let Ok(n) = name.parse() {
            return Ok(n);
        }
        if !identifier(name) {
            return Err(format!("Invalid sequence name {name}"));
        }
        Ok(intern(&mut self.labels, name, LOCAL_BASE))
    }

    pub(super) fn variable(&mut self, name: &str, grammar: &MacroGrammar) -> Result<u32> {
        if let Some((prefix, number)) = grammar
            .variables
            .iter()
            .filter_map(|rule| {
                rule.range?;
                Some((
                    rule.prefix.as_str(),
                    name.strip_prefix(&rule.prefix)?.parse::<u32>().ok()?,
                ))
            })
            .max_by_key(|(prefix, _)| prefix.len())
        {
            return grammar
                .resolve_variable(prefix, number)
                .ok_or_else(|| format!("Invalid common variable {name}"));
        }
        if !identifier(name) {
            return Err(format!("Invalid local variable {name}"));
        }
        if grammar.variables.iter().any(|rule| {
            rule.kind == VariableKind::Empty && rule.range.is_none() && rule.prefix == name
        }) {
            return Ok(0);
        }
        if !grammar.variables.iter().any(|rule| {
            rule.kind == VariableKind::Local
                && rule.range.is_none()
                && name.starts_with(&rule.prefix)
        }) {
            return Err(format!("Invalid local variable {name}"));
        }
        Ok(intern(&mut self.locals, name, LOCAL_BASE))
    }

    fn expression(&mut self, src: &str, grammar: &MacroGrammar) -> Result<Expr> {
        let mut cursor = Cursor(src.trim(), grammar, Some(self));
        let expr = cursor.expr(0, 0)?;
        if !cursor.0.is_empty() {
            return Err(format!("Unexpected OSP expression text {:?}", cursor.0));
        }
        Ok(expr)
    }

    pub(super) fn body(
        &mut self,
        src: &str,
        program_address: char,
        sequence_address: char,
        grammar: &MacroGrammar,
    ) -> Result<Body> {
        let src = src.trim();
        if let Some(rest) = keyword(src, grammar, Control::If) {
            let end =
                bracket_end(rest, grammar.expression).ok_or("Expected [condition] after IF")?;
            let condition = self.expression(&rest[..end], grammar)?;
            let target = rest[end..]
                .trim()
                .strip_prefix(sequence_address)
                .ok_or("Expected sequence after IF")?;
            let label = self.label(target.trim())?;
            return Ok(Body::If(
                condition,
                Box::new(Body::Goto(Expr::Number(f64::from(label)))),
            ));
        }
        if let Some(rest) = keyword(src, grammar, Control::Goto) {
            let target = rest
                .strip_prefix(sequence_address)
                .ok_or("Expected sequence after GOTO")?;
            return Ok(Body::Goto(Expr::Number(f64::from(
                self.label(target.trim())?,
            ))));
        }
        if keyword(src, grammar, Control::Rts) == Some("") {
            return Ok(Body::OspReturn);
        }
        if keyword(src, grammar, Control::Modout) == Some("") {
            return Ok(Body::OspModalCancel);
        }
        if let Some((rest, modal)) = keyword(src, grammar, Control::Call)
            .map(|rest| (rest, false))
            .or_else(|| keyword(src, grammar, Control::Modin).map(|rest| (rest, true)))
        {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let name = rest[..end]
                .strip_prefix(program_address)
                .ok_or("Expected program name")?;
            let name = name.to_owned();
            let target = self.program(&name)?;
            let mut args = Vec::new();
            let mut remaining = rest[end..].trim();
            while !remaining.is_empty() {
                let (arg, value) = remaining
                    .split_once(&grammar.assignment)
                    .ok_or("Expected named argument")?;
                let arg = self.variable(arg.trim(), grammar)?;
                if grammar.variable_kind(arg) != Some(VariableKind::Local) {
                    return Err("Expected a local argument name".into());
                }
                let value = value.trim_start();
                let end = next_assignment(value, grammar).unwrap_or(value.len());
                args.push((arg, self.expression(value[..end].trim(), grammar)?));
                remaining = value[end..].trim();
            }
            return Ok(Body::OspCall {
                target,
                name: format!("{program_address}{name}"),
                modal,
                args,
            });
        }

        if let Some((lhs, rhs)) = src.split_once(&grammar.assignment) {
            let lhs = lhs.trim();
            // Reserved address letters denote NC words, including on modal blocks.
            let numbered = grammar.variables.iter().any(|rule| {
                rule.range.is_some()
                    && lhs
                        .strip_prefix(&rule.prefix)
                        .is_some_and(|number| number.parse::<u32>().is_ok())
            });
            if numbered
                || (identifier(lhs) && !(lhs.len() == 1 && "XYZABCUVWIJKRFSTHDPQLGM".contains(lhs)))
            {
                return Ok(Body::Assign(
                    Expr::Number(f64::from(self.variable(lhs, grammar)?)),
                    self.expression(rhs, grammar)?,
                ));
            }
        }
        let mut words = Vec::new();
        let mut remaining = src;
        while !remaining.is_empty() {
            if let Some((lhs, rhs)) = remaining.split_once(&grammar.assignment) {
                let lhs = lhs.trim_end();
                if lhs.len() == 1 && lhs.as_bytes()[0].is_ascii_uppercase() {
                    let value = rhs.trim_start();
                    let end = next_word(value, grammar).unwrap_or(value.len());
                    words.push(super::Word {
                        addr: lhs.as_bytes()[0],
                        value: self.expression(&value[..end], grammar)?,
                        literal_point: None,
                    });
                    remaining = value[end..].trim_start();
                    continue;
                }
            }
            let end = remaining
                .find(char::is_whitespace)
                .unwrap_or(remaining.len());
            let token = &remaining[..end];
            let mut cursor = Cursor(token, grammar, None);
            let Body::Words(parsed) = cursor.body()? else {
                return Err(format!("Unsupported OSP statement {token}"));
            };
            if !cursor.0.is_empty() {
                return Err(format!("Expected address before {:?}", cursor.0));
            }
            words.extend(parsed);
            remaining = remaining[end..].trim_start();
        }
        Ok(Body::Words(words))
    }
}

fn keyword<'a>(src: &'a str, grammar: &MacroGrammar, statement: Control) -> Option<&'a str> {
    grammar
        .controls
        .iter()
        .filter(|rule| rule.statement == statement)
        .filter_map(|rule| {
            let rest = src.strip_prefix(&rule.keyword)?;
            (rest.is_empty()
                || rest.starts_with(char::is_whitespace)
                || rest.starts_with(grammar.expression[0]))
            .then_some(rest.trim_start())
        })
        .min_by_key(|rest| rest.len())
}

fn bracket_end(src: &str, [open, close]: [char; 2]) -> Option<usize> {
    if !src.starts_with(open) {
        return None;
    }
    let mut depth = 0;
    for (i, ch) in src.char_indices() {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return Some(i + ch.len_utf8());
            }
        }
    }
    None
}

fn next_assignment(src: &str, grammar: &MacroGrammar) -> Option<usize> {
    let mut depth = 0usize;
    for (i, ch) in src.char_indices() {
        match ch {
            ch if ch == grammar.expression[0] || ch == grammar.arguments[0] => depth += 1,
            ch if ch == grammar.expression[1] || ch == grammar.arguments[1] => {
                depth = depth.saturating_sub(1)
            }
            _ if depth == 0 && ch.is_whitespace() => {
                let next = src[i..].trim_start();
                if next
                    .split_once(&grammar.assignment)
                    .is_some_and(|(name, _)| identifier(name.trim_end()))
                {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn next_word(src: &str, grammar: &MacroGrammar) -> Option<usize> {
    let mut depth = 0usize;
    for (i, ch) in src.char_indices() {
        match ch {
            ch if ch == grammar.expression[0] || ch == grammar.arguments[0] => depth += 1,
            ch if ch == grammar.expression[1] || ch == grammar.arguments[1] => {
                depth = depth.saturating_sub(1)
            }
            _ if depth == 0 && ch.is_whitespace() => {
                let previous = src[..i].trim_end();
                if previous.is_empty()
                    || grammar
                        .operators
                        .iter()
                        .chain(&grammar.comparisons)
                        .any(|rule| previous.ends_with(&rule.symbol))
                {
                    continue;
                }
                let next = src[i..].trim_start();
                if next.as_bytes().first().is_some_and(u8::is_ascii_uppercase) && {
                    let value = next[1..].trim_start();
                    value.starts_with(|c: char| c.is_ascii_digit() || c == '.')
                        || value.starts_with(&grammar.assignment)
                        || value.starts_with(grammar.expression[0])
                        || grammar
                            .variables
                            .iter()
                            .any(|rule| !rule.prefix.is_empty() && value.starts_with(&rule.prefix))
                        || grammar.operator(value).is_some_and(|rule| {
                            matches!(rule.operation, crate::Op::Add | crate::Op::Sub)
                        })
                } {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}
