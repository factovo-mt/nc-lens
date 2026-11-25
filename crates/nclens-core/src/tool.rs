//! 工具番号ごとの認識結果。

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::{Diagnostic, RunResult};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    EndMill,
    Ball,
    Drill,
    Tap,
    Boring,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSource {
    Post,
    DOffset,
    Comment,
    #[default]
    Default,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ToolInfo {
    pub number: u32,
    pub diameter_mm: f64,
    pub corner_radius_mm: Option<f64>,
    /// コメントの H。
    pub flute_length_mm: Option<f64>,
    /// コメントの L。
    pub stickout_mm: Option<f64>,
    pub kind: ToolKind,
    pub name: Option<String>,
    pub comment: Option<String>,
    /// 直径の出どころ。
    pub source: ToolSource,
}

#[derive(Default)]
struct Evidence {
    line: usize,
    comment: Option<String>,
    radius: Option<f64>,
    explicit_d: bool,
}

pub(crate) struct Catalog<'a> {
    post: HashMap<u32, &'a crate::ToolInfo>,
    comments: HashMap<usize, (String, bool)>,
    evidence: BTreeMap<u32, Evidence>,
    indices: HashMap<u32, usize>,
}

impl<'a> Catalog<'a> {
    pub(crate) fn new(post: &'a crate::PostProfile, src: &str) -> Self {
        Self {
            post: post
                .variables
                .tools
                .iter()
                .filter_map(|(key, value)| Some((key.parse().ok()?, value)))
                .collect(),
            comments: src
                .replace("\r\n", "\n")
                .split(['\n', '\r'])
                .enumerate()
                .filter_map(|(i, raw)| {
                    let (before, rest) = raw.split_once('(')?;
                    let (comment, after) = rest.split_once(')')?;
                    let standalone = before.trim().is_empty() && after.trim().is_empty();
                    Some((i + 1, (comment.trim().to_owned(), standalone)))
                })
                .collect(),
            evidence: BTreeMap::new(),
            indices: HashMap::new(),
        }
    }

    pub(crate) fn select(&mut self, number: u32, line: usize) {
        let evidence = self.evidence.entry(number).or_insert_with(|| Evidence {
            line,
            ..Evidence::default()
        });
        let inline = self.comments.get(&line);
        let adjacent = [line.saturating_sub(1), line + 1]
            .into_iter()
            .filter_map(|i| self.comments.get(&i))
            .find(|(text, standalone)| *standalone && parse(text).diameter_mm > 0.);
        let candidate = inline
            .filter(|(text, _)| parse(text).diameter_mm > 0.)
            .or(adjacent)
            .or(inline);
        if let Some((text, _)) = candidate {
            if evidence
                .comment
                .as_ref()
                .is_none_or(|previous| parse(previous).diameter_mm == 0.)
            {
                evidence.comment = Some(text.clone());
            }
        }
    }

    pub(crate) fn observe(
        &mut self,
        number: u32,
        line: usize,
        radius: Option<f64>,
        explicit_d: bool,
    ) {
        let evidence = self.evidence.entry(number).or_insert_with(|| Evidence {
            line,
            ..Evidence::default()
        });
        if let Some(radius) = radius.filter(|r| r.is_finite() && *r != 0.) {
            if evidence.radius.is_none() || (explicit_d && !evidence.explicit_d) {
                evidence.radius = Some(radius.abs());
                evidence.explicit_d = explicit_d;
            }
        }
    }

    pub(crate) fn index(&mut self, result: &mut RunResult, number: u32) -> usize {
        *self.indices.entry(number).or_insert_with(|| {
            let index = result.tools.len();
            result.tools.push(ToolInfo {
                number,
                ..ToolInfo::default()
            });
            index
        })
    }

    pub(crate) fn finish(&mut self, result: &mut RunResult) {
        for number in self.evidence.keys().copied().collect::<Vec<_>>() {
            self.index(result, number);
        }
        for tool in &mut result.tools {
            let Some(evidence) = self.evidence.get(&tool.number) else {
                continue;
            };
            let number = tool.number;
            let mut info = evidence.comment.as_deref().map(parse).unwrap_or_default();
            info.number = number;
            if let Some(post) = self.post.get(&number) {
                if let Some(diameter) = post.diameter_mm.filter(|d| *d > 0. && d.is_finite()) {
                    info.diameter_mm = diameter;
                    info.source = ToolSource::Post;
                }
                if let Some(name) = &post.name {
                    info.name = Some(name.clone());
                    let kind = kind(&name.to_uppercase());
                    if kind != ToolKind::Unknown {
                        info.kind = kind;
                    }
                }
            }
            if info.source != ToolSource::Post {
                if let Some(radius) = evidence.radius {
                    info.diameter_mm = radius * 2.;
                    info.source = ToolSource::DOffset;
                }
            }
            if info.kind == ToolKind::Ball
                && info.corner_radius_mm.is_none()
                && info.diameter_mm > 0.
            {
                info.corner_radius_mm = Some(info.diameter_mm / 2.);
            }
            if info.diameter_mm == 0. {
                result.warnings.push(Diagnostic {
                    src_line: evidence.line,
                    message: format!("Unknown diameter for T{number}"),
                });
            }
            *tool = info;
        }
    }
}

fn kind(text: &str) -> ToolKind {
    [
        (["BALL", "ボール", "BEND"], ToolKind::Ball),
        (["DRILL", "ドリル", "DRL"], ToolKind::Drill),
        (["TAP", "タップ", "ねじ切り"], ToolKind::Tap),
        (["BORING", "ボーリング", "B. BAR"], ToolKind::Boring),
        (["END", "エンドミル", "エンド"], ToolKind::EndMill),
    ]
    .into_iter()
    .find_map(|(markers, kind)| markers.iter().any(|m| text.contains(m)).then_some(kind))
    .unwrap_or_default()
}

fn number(text: &str) -> Option<f64> {
    let digits = text
        .trim_start()
        .trim_start_matches(['=', '-', ' ', 'Φ', 'φ', 'Ø', '⌀']);
    let end = digits
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(digits.len());
    digits[..end].parse::<f64>().ok().filter(|n| n.is_finite())
}

fn parse(text: &str) -> ToolInfo {
    let upper = text.to_uppercase();
    let kind = kind(&upper);
    let diameter = if kind != ToolKind::Unknown || upper.starts_with(['Φ', 'Ø', '⌀']) {
        number(&upper).unwrap_or_default()
    } else {
        0.
    };
    let field = |tag| upper.split_once(tag).and_then(|(_, tail)| number(tail));
    ToolInfo {
        diameter_mm: diameter,
        corner_radius_mm: field("-R"),
        flute_length_mm: field("H="),
        stickout_mm: field("L="),
        kind,
        name: Some(text.to_owned()),
        comment: Some(text.to_owned()),
        source: if diameter > 0. {
            ToolSource::Comment
        } else {
            ToolSource::Default
        },
        ..ToolInfo::default()
    }
}
