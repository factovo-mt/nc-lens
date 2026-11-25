use serde::{Deserialize, Serialize};

use crate::{MacroSyntax, MnemonicSettings};

macro_rules! macro_fields {
    ($($field:ident: $ty:ty),+ $(,)?) => {
        #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(default, deny_unknown_fields)]
        pub struct MacroSettings {
            $(#[serde(skip_serializing_if = "Option::is_none")]
            pub $field: Option<$ty>,)+
        }

        #[derive(Debug, Clone)]
        pub(crate) struct MacroGrammar {
            $(pub $field: $ty,)+
        }

        impl MacroGrammar {
            pub fn resolve(base: &MacroSettings, osp: Option<&MacroSettings>, custom: &MacroSettings) -> Self {
                Self {
                    $($field: custom.$field.as_ref()
                        .or_else(|| osp.and_then(|settings| settings.$field.as_ref()))
                        .or(base.$field.as_ref())
                        .expect(concat!("missing built-in macro setting: ", stringify!($field)))
                        .clone(),)+
                }
            }
        }
    };
}

macro_fields! {
    syntax: MacroSyntax,
    expression: [char; 2],
    arguments: [char; 2],
    assignment: String,
    block_skip: String,
    comment_pair: Vec<char>,
    line_comments: Vec<String>,
    variables: Vec<VariableDefinition>,
    operators: Vec<OperatorDefinition>,
    comparisons: Vec<OperatorDefinition>,
    controls: Vec<ControlDefinition>,
    functions: Vec<FunctionDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariableDefinition {
    pub prefix: String,
    pub kind: VariableKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<[u32; 2]>,
    #[serde(default, skip_serializing_if = "zero")]
    pub offset: u32,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VariableKind {
    Empty,
    Local,
    Common,
    Retained,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

impl Op {
    fn comparison(self) -> bool {
        matches!(
            self,
            Self::Eq | Self::Ne | Self::Lt | Self::Le | Self::Gt | Self::Ge
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorDefinition {
    pub symbol: String,
    pub operation: Op,
    pub precedence: u8,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Control {
    If,
    Then,
    Goto,
    While,
    Do,
    End,
    Call,
    Rts,
    Modin,
    Modout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlDefinition {
    pub keyword: String,
    pub statement: Control,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Function {
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Sqrt,
    Abs,
    Round,
    Fix,
    Fup,
    Ln,
    Exp,
    Pow,
    Bin,
    Bcd,
    Adp,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AngleUnit {
    #[default]
    Degrees,
    Radians,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FunctionDefinition {
    pub name: String,
    pub function: Function,
    pub arity: [u8; 2],
    #[serde(default)]
    pub angle_unit: AngleUnit,
    #[serde(default)]
    pub description: String,
}

fn zero(number: &u32) -> bool {
    *number == 0
}

impl VariableDefinition {
    fn stored_range(&self) -> Option<[u32; 2]> {
        let [lo, hi] = self.range?;
        Some([lo.checked_add(self.offset)?, hi.checked_add(self.offset)?])
    }

    pub(crate) fn resolve(&self, id: u32) -> Option<u32> {
        let [lo, hi] = self.range?;
        if (lo..=hi).contains(&id) {
            self.offset.checked_add(id)
        } else {
            None
        }
    }
}

impl MacroGrammar {
    pub fn operator(&self, src: &str) -> Option<&OperatorDefinition> {
        self.operators
            .iter()
            .chain(&self.comparisons)
            .filter(|rule| {
                src.strip_prefix(&rule.symbol).is_some_and(|rest| {
                    self.syntax != MacroSyntax::Osp
                        || !identifier(&rule.symbol)
                        || !rest.starts_with(|ch: char| ch.is_ascii_alphanumeric() || ch == '_')
                })
            })
            .max_by_key(|rule| rule.symbol.len())
    }

    pub fn variable_kind(&self, number: u32) -> Option<VariableKind> {
        if number == 0
            && self
                .variables
                .iter()
                .any(|rule| rule.kind == VariableKind::Empty && rule.range.is_none())
        {
            return Some(VariableKind::Empty);
        }
        if self.syntax == MacroSyntax::Osp && number >= crate::syntax::OSP_LOCAL_BASE {
            return Some(VariableKind::Local);
        }
        self.variables.iter().find_map(|rule| {
            let [first, last] = rule.stored_range()?;
            ((first..=last).contains(&number)).then_some(rule.kind)
        })
    }

    pub fn resolve_variable(&self, prefix: &str, number: u32) -> Option<u32> {
        self.variables
            .iter()
            .filter(|rule| rule.prefix == prefix)
            .find_map(|rule| rule.resolve(number))
    }

    pub fn invalid_field(&self) -> Option<&'static str> {
        use std::collections::BTreeSet;
        let pair = |[open, close]: [char; 2]| {
            matches!((open, close), ('[', ']') | ('(', ')') | ('{', '}'))
        };
        if !pair(self.expression) {
            return Some("expression");
        }
        if !pair(self.arguments) {
            return Some("arguments");
        }
        let valid_symbol = |symbol: &str| {
            !symbol.is_empty()
                && !symbol.starts_with(|ch: char| ch.is_ascii_digit())
                && symbol.chars().all(|c| {
                    !c.is_whitespace()
                        && !c.is_control()
                        && !c.is_ascii_lowercase()
                        && !".,[](){}%".contains(c)
                })
        };
        if !valid_symbol(&self.assignment) {
            return Some("assignment");
        }
        if !valid_symbol(&self.block_skip) || self.block_skip == self.assignment {
            return Some("block_skip");
        }
        match self.comment_pair.as_slice() {
            [] => {}
            &[open, close]
                if pair([open, close])
                    && !self.expression.contains(&open)
                    && !self.arguments.contains(&open) => {}
            _ => return Some("comment_pair"),
        }
        let mut seen = BTreeSet::new();
        let commented = |symbol: &str| {
            self.line_comments
                .iter()
                .any(|comment| symbol.contains(comment))
        };
        for symbol in &self.line_comments {
            if !valid_symbol(symbol)
                || !seen.insert(symbol.as_str())
                || self.assignment.contains(symbol)
                || self.block_skip.contains(symbol)
            {
                return Some("line_comments");
            }
        }
        // Block skips and division are in different positions in a block.
        for (table, rules, comparisons) in [
            ("operators", &self.operators, false),
            ("comparisons", &self.comparisons, true),
        ] {
            for rule in rules {
                if !valid_symbol(&rule.symbol)
                    || commented(&rule.symbol)
                    || !seen.insert(rule.symbol.as_str())
                    || rule.operation.comparison() != comparisons
                    || rule.precedence == 0
                    || rule.precedence == u8::MAX
                    || (!comparisons && rule.symbol == self.assignment)
                {
                    return Some(table);
                }
            }
        }
        if self.operators.iter().all(|rule| rule.operation != Op::Add)
            || self.operators.iter().all(|rule| rule.operation != Op::Sub)
        {
            return Some("operators");
        }
        let has_control = |statement| self.controls.iter().any(|rule| rule.statement == statement);
        if self.syntax == MacroSyntax::Fanuc
            && ((has_control(Control::If)
                && !(has_control(Control::Then) || has_control(Control::Goto)))
                || (has_control(Control::Then) && !has_control(Control::If))
                || has_control(Control::While) != has_control(Control::Do)
                || has_control(Control::While) != has_control(Control::End))
        {
            return Some("controls");
        }
        for rule in &self.controls {
            let osp_only = matches!(
                rule.statement,
                Control::Call | Control::Rts | Control::Modin | Control::Modout
            );
            let fanuc_only = matches!(
                rule.statement,
                Control::Then | Control::While | Control::Do | Control::End
            );
            if (self.syntax == MacroSyntax::Fanuc && osp_only)
                || (self.syntax == MacroSyntax::Osp && fanuc_only)
                || !identifier(&rule.keyword)
                || commented(&rule.keyword)
                || rule.keyword == self.assignment
                || rule.keyword.starts_with(&self.block_skip)
                || !seen.insert(rule.keyword.as_str())
            {
                return Some("controls");
            }
        }
        for rule in &self.functions {
            let expected = match rule.function {
                Function::Atan => [1, 2],
                Function::Pow => [2, 2],
                _ => [1, 1],
            };
            if !identifier(&rule.name)
                || commented(&rule.name)
                || rule.name == self.assignment
                || !seen.insert(rule.name.as_str())
                || rule.arity[0] < expected[0]
                || rule.arity[1] > expected[1]
                || rule.arity[0] > rule.arity[1]
            {
                return Some("functions");
            }
        }
        for (i, rule) in self.variables.iter().enumerate() {
            if (!rule.prefix.is_empty()
                && (!valid_symbol(&rule.prefix)
                    || commented(&rule.prefix)
                    || rule.prefix == self.assignment
                    || rule.prefix.starts_with(&self.block_skip)
                    || seen.contains(rule.prefix.as_str())))
                || (rule.prefix.is_empty()
                    && (rule.range.is_some() || rule.kind != VariableKind::Local))
                || (rule.range.is_some()
                    && !rule.stored_range().is_some_and(|[lo, hi]| {
                        lo <= hi
                            && (lo > 0 || rule.kind == VariableKind::Empty)
                            && (self.syntax != MacroSyntax::Osp
                                || hi < crate::syntax::OSP_LOCAL_BASE)
                    }))
                || (rule.range.is_none() && rule.offset != 0)
                || (rule.range.is_none()
                    && (self.syntax != MacroSyntax::Osp
                        || !matches!(rule.kind, VariableKind::Local | VariableKind::Empty)
                        || (!rule.prefix.is_empty() && !identifier(&rule.prefix))))
                || self.variables[..i].iter().any(|other| {
                    match (rule.stored_range(), other.stored_range()) {
                        (Some([lo, hi]), Some([first, last])) => {
                            (lo <= last && first <= hi)
                                || (rule.prefix == other.prefix
                                    && rule.range.zip(other.range).is_some_and(
                                        |([lo, hi], [first, last])| lo <= last && first <= hi,
                                    ))
                        }
                        (None, None) => rule.prefix == other.prefix,
                        _ => false,
                    }
                })
            {
                return Some("variables");
            }
        }
        None
    }
}

fn identifier(value: &str) -> bool {
    value.as_bytes().first().is_some_and(u8::is_ascii_uppercase)
        && value
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

#[derive(Debug, Clone, Serialize)]
pub struct MacroReferenceRow {
    pub name: String,
    pub symbol: String,
    pub description: String,
    pub example: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MacroReference {
    pub variables: Vec<MacroReferenceRow>,
    pub operators: Vec<MacroReferenceRow>,
    pub comparisons: Vec<MacroReferenceRow>,
    pub controls: Vec<MacroReferenceRow>,
    pub functions: Vec<MacroReferenceRow>,
}

pub fn macro_reference(settings: &MnemonicSettings) -> MacroReference {
    let profile = crate::PostProfile {
        dialect: settings.clone(),
        ..Default::default()
    };
    let resolved = crate::dialect::Dialect::new(&profile);
    let program_address = resolved.program_address;
    let sequence_address = resolved.sequence_address;
    let grammar = resolved.macro_grammar;
    let [open, close] = grammar.expression;
    let [arg_open, arg_close] = grammar.arguments;
    let variable = grammar
        .variables
        .iter()
        .find(|rule| rule.kind == VariableKind::Local)
        .map_or_else(
            || "A".into(),
            |rule| match rule.range {
                Some([first, _]) => format!("{}{first}", rule.prefix),
                None => format!("{}A", rule.prefix),
            },
        );
    let assign = &grammar.assignment;
    let keyword = |statement, fallback| {
        grammar
            .controls
            .iter()
            .find(|rule| rule.statement == statement)
            .map_or(fallback, |rule| rule.keyword.as_str())
    };
    let operator = |rule: &OperatorDefinition| MacroReferenceRow {
        name: format!("{:?}", rule.operation),
        symbol: rule.symbol.clone(),
        description: if rule.description.is_empty() {
            format!("優先順位 {}", rule.precedence)
        } else {
            format!("{}（優先順位 {}）", rule.description, rule.precedence)
        },
        example: format!("{open}4 {} 2{close}", rule.symbol),
    };
    let entry = |name: &str, symbol, description: &str, example| MacroReferenceRow {
        name: name.into(),
        symbol,
        description: description.into(),
        example,
    };
    let mut operators: Vec<_> = grammar.operators.iter().map(operator).collect();
    operators.extend([
        entry(
            "代入",
            assign.clone(),
            "右辺の値を左辺に格納",
            format!("{variable}{assign}2"),
        ),
        entry(
            "式の括弧",
            format!("{open} {close}"),
            "式をグループ化",
            format!("{open}2{close}"),
        ),
        entry(
            "引数の括弧",
            format!("{arg_open} {arg_close}"),
            "関数の引数を囲む",
            format!("{arg_open}1{arg_close}"),
        ),
        entry(
            "ブロックスキップ",
            grammar.block_skip.clone(),
            "選択したスキップ番号のブロックを飛ばす",
            format!("{}1 G0 X0", grammar.block_skip),
        ),
    ]);
    if let &[open, close] = grammar.comment_pair.as_slice() {
        operators.push(entry(
            "コメントの括弧",
            format!("{open} {close}"),
            "括弧内をコメントとして扱う",
            format!("{open}メモ{close}"),
        ));
    }
    operators.extend(grammar.line_comments.iter().map(|symbol| {
        entry(
            "行末コメント",
            symbol.clone(),
            "記号から行末までをコメントとして扱う",
            format!("{symbol}メモ"),
        )
    }));
    MacroReference {
        variables: grammar
            .variables
            .iter()
            .map(|rule| {
                let prefix = &rule.prefix;
                let (symbol, example) = match rule.range {
                    Some([lo, hi]) if lo != hi => (
                        format!("{prefix}{lo}–{prefix}{hi}"),
                        format!("{prefix}{lo}"),
                    ),
                    Some([lo, _]) => (format!("{prefix}{lo}"), format!("{prefix}{lo}")),
                    None if rule.kind == VariableKind::Empty => (prefix.clone(), prefix.clone()),
                    None => (format!("{prefix}name"), format!("{prefix}A")),
                };
                MacroReferenceRow {
                    name: format!("{:?}", rule.kind),
                    symbol,
                    description: rule.description.clone(),
                    example,
                }
            })
            .collect(),
        operators,
        comparisons: grammar.comparisons.iter().map(operator).collect(),
        controls: grammar
            .controls
            .iter()
            .map(|rule| {
                let key = &rule.keyword;
                let target = if grammar.syntax == MacroSyntax::Osp {
                    format!("{sequence_address}10")
                } else {
                    "10".into()
                };
                let example = match rule.statement {
                    Control::If if grammar.syntax == MacroSyntax::Osp => {
                        format!("{key} {open}1{close} {target}")
                    }
                    Control::If
                        if !grammar
                            .controls
                            .iter()
                            .any(|rule| rule.statement == Control::Then) =>
                    {
                        format!(
                            "{key}{open}1{close}{}{target}",
                            keyword(Control::Goto, "GOTO")
                        )
                    }
                    Control::If => format!(
                        "{key}{open}1{close}{}{variable}{assign}2",
                        keyword(Control::Then, "THEN")
                    ),
                    Control::Then => format!(
                        "{}{open}1{close}{key}{variable}{assign}2",
                        keyword(Control::If, "IF")
                    ),
                    Control::Goto => format!("{key} {target}"),
                    Control::While => format!("{key}{open}1{close}{}1", keyword(Control::Do, "DO")),
                    Control::Do => {
                        format!("{}{open}1{close}{key}1", keyword(Control::While, "WHILE"))
                    }
                    Control::End => format!("{key}1"),
                    Control::Call | Control::Modin => format!("{key} {program_address}100"),
                    Control::Rts | Control::Modout => key.clone(),
                };
                MacroReferenceRow {
                    name: format!("{:?}", rule.statement),
                    symbol: key.clone(),
                    description: rule.description.clone(),
                    example,
                }
            })
            .collect(),
        functions: grammar
            .functions
            .iter()
            .map(|rule| {
                let name = &rule.name;
                let args = if rule.arity[0] == 2 { "2,3" } else { "1" };
                let [min, max] = rule.arity;
                let count = if min == max {
                    min.to_string()
                } else {
                    format!("{min}〜{max}")
                };
                let unit = match rule.function {
                    Function::Sin
                    | Function::Cos
                    | Function::Tan
                    | Function::Asin
                    | Function::Acos
                    | Function::Atan => match rule.angle_unit {
                        AngleUnit::Degrees => "、角度は度",
                        AngleUnit::Radians => "、角度はラジアン",
                    },
                    _ => "",
                };
                let info = format!("引数 {count} 個{unit}");
                let description = if rule.description.is_empty() {
                    info
                } else {
                    format!("{}（{info}）", rule.description)
                };
                MacroReferenceRow {
                    name: format!("{:?}", rule.function),
                    symbol: name.clone(),
                    description,
                    example: format!("{name}{arg_open}{args}{arg_close}"),
                }
            })
            .collect(),
    }
}
