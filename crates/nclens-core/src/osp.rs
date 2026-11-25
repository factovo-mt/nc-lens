//! Basic OSP-P200M/P20M User Task syntax (Okuma manual 5228-E, sections 10–11).
use super::{Body, Cursor, Expr, Result};
use std::collections::HashMap;

pub(crate) const COMMON_BASE: u32 = 1_000_000_000;
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

    fn variable(&mut self, name: &str) -> Result<u32> {
        if let Some(index) = name.strip_prefix("VC").and_then(|n| n.parse::<u32>().ok()) {
            return (1..LOCAL_BASE - COMMON_BASE)
                .contains(&index)
                .then_some(COMMON_BASE + index)
                .ok_or_else(|| format!("Invalid common variable {name}"));
        }
        if !identifier(name) {
            return Err(format!("Invalid local variable {name}"));
        }
        Ok(intern(&mut self.locals, name, LOCAL_BASE))
    }

    fn expression(&mut self, src: &str) -> Result<Expr> {
        let mut normalized = String::new();
        let mut src = src.trim();
        while let Some(ch) = src.as_bytes().first().copied() {
            if ch.is_ascii_alphabetic() {
                let end = src
                    .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .unwrap_or(src.len());
                let token = &src[..end];
                match token {
                    "EQ" | "NE" | "LE" | "LT" | "GE" | "GT" | "AND" | "OR" | "XOR" | "MOD" => {
                        normalized.push_str(token)
                    }
                    "EMPTY" => normalized.push_str("#0"),
                    name => normalized.push_str(&format!("#{}", self.variable(name)?)),
                }
                src = &src[end..];
                continue;
            }
            let mut consumed = 1;
            let compare = [
                (">=", "GE"),
                ("<=", "LE"),
                ("<>", "NE"),
                ("!=", "NE"),
                ("==", "EQ"),
                ("=", "EQ"),
                (">", "GT"),
                ("<", "LT"),
            ]
            .iter()
            .find(|(op, _)| src.starts_with(op));
            if let Some((op, name)) = compare {
                normalized.push_str(name);
                consumed = op.len();
            } else if !ch.is_ascii_whitespace() {
                if !ch.is_ascii() {
                    return Err("Invalid character in OSP expression".into());
                }
                normalized.push(ch as char);
            }
            src = &src[consumed..];
        }
        let mut cursor = Cursor(&normalized);
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
    ) -> Result<Body> {
        let src = src.trim();
        if let Some(rest) = keyword(src, "IF") {
            let end = bracket_end(rest).ok_or("Expected [condition] after IF")?;
            let condition = self.expression(&rest[..end])?;
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
        if let Some(rest) = keyword(src, "GOTO") {
            let target = rest
                .strip_prefix(sequence_address)
                .ok_or("Expected sequence after GOTO")?;
            return Ok(Body::Goto(Expr::Number(f64::from(
                self.label(target.trim())?,
            ))));
        }
        if src == "RTS" {
            return Ok(Body::OspReturn);
        }
        if src == "MODOUT" {
            return Ok(Body::OspModalCancel);
        }
        if let Some((rest, modal)) = keyword(src, "CALL")
            .map(|rest| (rest, false))
            .or_else(|| keyword(src, "MODIN").map(|rest| (rest, true)))
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
                let (arg, value) = remaining.split_once('=').ok_or("Expected named argument")?;
                let arg = self.variable(arg.trim())?;
                if arg < LOCAL_BASE {
                    return Err("Expected a local argument name".into());
                }
                let value = value.trim_start();
                let end = next_assignment(value).unwrap_or(value.len());
                args.push((arg, self.expression(value[..end].trim())?));
                remaining = value[end..].trim();
            }
            return Ok(Body::OspCall {
                target,
                name: format!("{program_address}{name}"),
                modal,
                args,
            });
        }

        if let Some((lhs, rhs)) = src.split_once('=') {
            let lhs = lhs.trim();
            // Reserved address letters denote NC words, including on modal blocks.
            if identifier(lhs) && !(lhs.len() == 1 && "XYZABCUVWIJKRFSTHDPQLGM".contains(lhs)) {
                return Ok(Body::Assign(
                    Expr::Number(f64::from(self.variable(lhs)?)),
                    self.expression(rhs)?,
                ));
            }
        }
        let mut words = Vec::new();
        let mut remaining = src;
        while !remaining.is_empty() {
            if let Some((lhs, rhs)) = remaining.split_once('=') {
                let lhs = lhs.trim_end();
                if lhs.len() == 1 && lhs.as_bytes()[0].is_ascii_uppercase() {
                    let value = rhs.trim_start();
                    let end = next_word(value).unwrap_or(value.len());
                    words.push(super::Word {
                        addr: lhs.as_bytes()[0],
                        value: self.expression(&value[..end])?,
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
            let mut cursor = Cursor(token);
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

fn keyword<'a>(src: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = src.strip_prefix(keyword)?;
    rest.starts_with(|ch: char| ch.is_whitespace() || ch == '[')
        .then(|| rest.trim_start())
}

fn bracket_end(src: &str) -> Option<usize> {
    if !src.starts_with('[') {
        return None;
    }
    let mut depth = 0;
    for (i, ch) in src.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn next_assignment(src: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, ch) in src.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 && ch.is_whitespace() => {
                let next = src[i..].trim_start();
                if next
                    .split_once('=')
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

fn next_word(src: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, ch) in src.char_indices() {
        match ch {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 && ch.is_whitespace() => {
                let previous = src[..i].trim_end();
                if previous.is_empty() || previous.ends_with(['+', '-', '*', '/', '=', '<', '>']) {
                    continue;
                }
                let next = src[i..].trim_start();
                if next.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
                    && next[1..]
                        .trim_start()
                        .starts_with(|c: char| c.is_ascii_digit() || ".=+-[#".contains(c))
                {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}
