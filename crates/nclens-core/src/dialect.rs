use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{MacroGrammar, MacroSettings};

pub const FANUC_DIALECT_TOML: &str = include_str!("../assets/fanuc.dialect.toml");
pub const OSP_DIALECT_TOML: &str = include_str!("../assets/osp.dialect.toml");

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MacroSyntax {
    #[default]
    Fanuc,
    Osp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DialectSettings {
    pub name: String,
    /// Select the built-in dialect to inherit. Without a base, the name selects a
    /// matching built-in dialect and other names inherit FANUC.
    pub base: Option<String>,
    #[serde(rename = "macro", skip_serializing_if = "macro_settings_empty")]
    pub macro_settings: MacroSettings,
    /// Source letter to canonical address role.
    #[serde(serialize_with = "serialize_addresses")]
    pub addresses: BTreeMap<char, AddressRole>,
    pub g_codes: BTreeMap<String, GCodeDefinition>,
    pub m_codes: BTreeMap<String, MCodeDefinition>,
    /// Keys are source code words (e.g. G100, M50 or A100).
    pub custom: BTreeMap<String, CustomCode>,
    /// Address letter to a short explanation shown in the GUI tooltip.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub address_descriptions: BTreeMap<String, String>,
}

pub type MnemonicSettings = DialectSettings;

fn macro_settings_empty(settings: &MacroSettings) -> bool {
    *settings == MacroSettings::default()
}

fn serialize_addresses<S: serde::Serializer>(
    value: &BTreeMap<char, AddressRole>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::ser::SerializeMap;
    let mut map = serializer.serialize_map(Some(value.len()))?;
    for (key, role) in value {
        map.serialize_entry(key.encode_utf8(&mut [0; 4]), role)?;
    }
    map.end()
}

impl Default for DialectSettings {
    fn default() -> Self {
        Self {
            name: "fanuc".into(),
            base: None,
            macro_settings: MacroSettings::default(),
            addresses: BTreeMap::new(),
            g_codes: BTreeMap::new(),
            m_codes: BTreeMap::new(),
            custom: BTreeMap::new(),
            address_descriptions: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum AddressRole {
    X = b'X',
    Y = b'Y',
    Z = b'Z',
    A = b'A',
    B = b'B',
    C = b'C',
    U = b'U',
    V = b'V',
    W = b'W',
    I = b'I',
    J = b'J',
    K = b'K',
    R = b'R',
    F = b'F',
    S = b'S',
    T = b'T',
    H = b'H',
    D = b'D',
    O = b'O',
    N = b'N',
    P = b'P',
    L = b'L',
    Q = b'Q',
    G = b'G',
    M = b'M',
    DwellSeconds = 0,
    Repeat = 1,
}

impl AddressRole {
    pub(crate) fn canonical(self) -> u8 {
        match self {
            Self::DwellSeconds => b'X',
            Self::Repeat => b'L',
            _ => self as u8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GCodeDefinition {
    pub function: String,
    pub group: Option<u32>,
    /// Short explanation shown in the GUI tooltip (empty: none).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MCodeDefinition {
    pub function: String,
    pub program: Option<u32>,
    /// Short explanation shown in the GUI tooltip (empty: none).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum CustomCode {
    Call(u32),
    Replace(String),
}

use std::path::Path;
use std::sync::OnceLock;

use crate::{syntax, PostError, PostProfile};

fn builtins() -> &'static [DialectSettings; 2] {
    static BUILTINS: OnceLock<[DialectSettings; 2]> = OnceLock::new();
    BUILTINS.get_or_init(|| {
        [FANUC_DIALECT_TOML, OSP_DIALECT_TOML]
            .map(|src| toml::from_str(src).expect("valid built-in dialect"))
    })
}

pub(crate) fn macro_grammar(settings: &DialectSettings) -> MacroGrammar {
    let [fanuc, osp] = builtins();
    let use_osp = settings.macro_settings.syntax.as_ref().map_or_else(
        || settings.base.as_deref().unwrap_or(&settings.name) == "osp",
        |syntax| *syntax == MacroSyntax::Osp,
    );
    MacroGrammar::resolve(
        &fanuc.macro_settings,
        use_osp.then_some(&osp.macro_settings),
        &settings.macro_settings,
    )
}

pub fn builtin_dialects() -> Vec<DialectSettings> {
    builtins().to_vec()
}

pub fn load_dialect(path: impl AsRef<Path>) -> Result<DialectSettings, PostError> {
    crate::text_settings::read_settings(path, crate::dialect_from_str, crate::dialect_from_json)
}

pub fn save_dialect(path: impl AsRef<Path>, settings: &DialectSettings) -> Result<(), PostError> {
    crate::text_settings::write_settings(
        path,
        settings,
        crate::dialect_to_string,
        crate::dialect_to_json,
    )
}

pub fn validate_dialect(settings: &DialectSettings) -> Vec<String> {
    invalid_fields(settings)
        .into_iter()
        .map(|field| format!("Invalid dialect setting {}", field.join(".")))
        .collect()
}

fn numeric_key(key: &str) -> Option<f64> {
    key.parse::<f64>()
        .ok()
        .filter(|n| n.is_finite() && *n >= 0. && !key.is_empty())
}

pub(crate) fn invalid_fields(settings: &DialectSettings) -> Vec<Vec<&str>> {
    let mut problems = Vec::new();
    let grammar = macro_grammar(settings);
    if let Some(field) = grammar.invalid_field() {
        problems.push(vec!["macro", field]);
    }
    if settings.name.trim().is_empty() {
        problems.push(vec!["name"]);
    }
    if settings
        .base
        .as_deref()
        .is_some_and(|base| !matches!(base, "fanuc" | "osp"))
    {
        problems.push(vec!["base"]);
    }
    if settings
        .addresses
        .keys()
        .any(|key| !key.is_ascii_uppercase())
    {
        problems.push(vec!["addresses"]);
    }
    if settings.address_descriptions.keys().any(|key| {
        key.len() != 1 || !key.bytes().all(|b| b.is_ascii_uppercase())
    }) {
        problems.push(vec!["address_descriptions"]);
    }
    let mut seen = std::collections::BTreeSet::new();
    for (key, def) in &settings.g_codes {
        if !numeric_key(key).is_some_and(|n| seen.insert(n.to_string()))
            || def.group.is_some_and(|g| g > 99)
            || !(matches!(def.function.as_str(), "ignore" | "unsupported")
                || canonical_g(&def.function).is_some())
        {
            problems.push(vec!["g_codes", key]);
        }
    }
    seen.clear();
    for (key, def) in &settings.m_codes {
        if !numeric_key(key).is_some_and(|n| n.fract() == 0. && seen.insert(n.to_string()))
            || match def.function.as_str() {
                "call" => def.program.is_none(),
                "ignore" => def.program.is_some(),
                other => canonical_m(other).is_none() || def.program.is_some(),
            }
        {
            problems.push(vec!["m_codes", key]);
        }
    }
    seen.clear();
    for (key, def) in &settings.custom {
        if !custom_key(key).is_some_and(|(letter, value)| seen.insert(format!("{letter}{value}")))
            || matches!(def, CustomCode::Replace(text) if !syntax::parse_replacement(text, &grammar).is_ok_and(|words| !words.is_empty()))
        {
            problems.push(vec!["custom", key]);
        }
    }
    problems
}

fn custom_key(key: &str) -> Option<(char, f64)> {
    let letter = key.chars().next()?.to_ascii_uppercase();
    letter.is_ascii_uppercase().then_some(())?;
    Some((
        letter,
        key[1..].parse::<f64>().ok().filter(|n| n.is_finite())?,
    ))
}

fn canonical_g(function: &str) -> Option<f64> {
    match function {
        "length_dynamic" => return Some(f64::from(DYNAMIC_LENGTH)),
        "dwell_feed" => return Some(f64::from(DWELL_FEED)),
        "work_select_h" => return Some(f64::from(WORK_SELECT_H)),
        _ => {}
    }
    builtins()[0]
        .g_codes
        .iter()
        .find_map(|(code, def)| (def.function == function).then(|| code.parse().unwrap()))
}

fn canonical_m(function: &str) -> Option<f64> {
    builtins()[0]
        .m_codes
        .iter()
        .find_map(|(code, def)| (def.function == function).then(|| code.parse().unwrap()))
}

// Internal IDs keep unmatched source codes from accidentally invoking these operations.
pub(crate) const DYNAMIC_LENGTH: u32 = 100_043;
pub(crate) const DWELL_FEED: u32 = 100_004;
pub(crate) const WORK_SELECT_H: u32 = 100_015;

pub(crate) fn semantic_group(code: u32) -> Option<u32> {
    builtins()[0].g_codes.get(&code.to_string())?.group
}

#[derive(Clone)]
pub(crate) enum Action {
    Word(u8, f64),
    Ignore,
    Unsupported,
    Custom(CustomCode),
}

pub(crate) struct Dialect {
    pub macro_grammar: MacroGrammar,
    pub program_address: char,
    pub sequence_address: char,
    addresses: BTreeMap<char, AddressRole>,
    g_codes: BTreeMap<String, (Action, Option<u32>)>,
    m_codes: BTreeMap<String, Action>,
    custom: BTreeMap<String, CustomCode>,
}

impl Dialect {
    pub fn new(post: &PostProfile) -> Self {
        let settings = &post.dialect;
        let [fanuc, osp] = builtins();
        let mut addresses = fanuc.addresses.clone();
        let mut g = fanuc.g_codes.clone();
        let mut m = fanuc.m_codes.clone();
        let mut custom = BTreeMap::new();
        let use_osp = settings.base.as_deref().unwrap_or(&settings.name) == "osp";
        let mut program_address = post.codes.program_address;
        let mut sequence_address = 'N';
        for layer in use_osp.then_some(osp).into_iter().chain([settings]) {
            for (&letter, &role) in &layer.addresses {
                if role == AddressRole::O && letter != 'O' {
                    program_address = letter;
                } else if role == AddressRole::N {
                    sequence_address = letter;
                }
                addresses.insert(letter, role);
            }
            g.extend(
                layer
                    .g_codes
                    .iter()
                    .filter_map(|(key, def)| Some((numeric_key(key)?.to_string(), def.clone()))),
            );
            m.extend(
                layer
                    .m_codes
                    .iter()
                    .filter_map(|(key, def)| Some((numeric_key(key)?.to_string(), def.clone()))),
            );
            custom.extend(layer.custom.iter().filter_map(|(key, def)| {
                let (letter, value) = custom_key(key)?;
                Some((format!("{letter}{value}"), def.clone()))
            }));
        }
        let g_codes = g
            .into_iter()
            .map(|(key, def)| {
                let action = match def.function.as_str() {
                    "unsupported" => Action::Unsupported,
                    other => canonical_g(other).map_or(Action::Ignore, |g| Action::Word(b'G', g)),
                };
                (key, (action, def.group.filter(|&group| group != 0)))
            })
            .collect();
        let m_codes = m
            .into_iter()
            .map(|(key, def)| {
                let action = if def.function == "call" {
                    def.program
                        .map_or(Action::Ignore, |p| Action::Custom(CustomCode::Call(p)))
                } else {
                    canonical_m(&def.function).map_or(Action::Ignore, |m| Action::Word(b'M', m))
                };
                (key, action)
            })
            .collect();
        Self {
            macro_grammar: macro_grammar(settings),
            program_address,
            sequence_address,
            addresses,
            g_codes,
            m_codes,
            custom,
        }
    }

    pub fn address(&self, letter: u8) -> u8 {
        self.addresses
            .get(&(letter as char))
            .map_or(letter, |role| role.canonical())
    }

    pub fn action(&self, letter: u8, value: f64) -> (Action, Option<u32>) {
        let word = format!("{}{value}", letter as char);
        if let Some(action) = self.custom.get(&word) {
            return (Action::Custom(action.clone()), None);
        }
        let address = self.address(letter);
        let key = value.to_string();
        if address == b'G' {
            if let Some((action, group)) = self.g_codes.get(&key) {
                return (action.clone(), *group);
            }
        } else if address == b'M' {
            if let Some(action) = self.m_codes.get(&key) {
                return (action.clone(), None);
            }
        }
        if address == b'G'
            && [DYNAMIC_LENGTH, DWELL_FEED, WORK_SELECT_H]
                .map(f64::from)
                .contains(&value)
        {
            return (Action::Unsupported, None);
        }
        (Action::Word(address, value), None)
    }

    pub fn source_address(&self, canonical: u8) -> u8 {
        if self.address(canonical) == canonical {
            return canonical;
        }
        self.addresses
            .iter()
            .find(|(_, role)| role.canonical() == canonical)
            .map_or(canonical, |(letter, _)| *letter as u8)
    }

    pub fn modal_code(&self, canonical: f64) -> Option<(u32, f64)> {
        let number = self.source_code(b'G', canonical);
        let (action, group) = self.g_codes.get(&number.to_string())?;
        if matches!(action, Action::Word(_, code) if *code == canonical) {
            return group.map(|group| (group, number));
        }
        None
    }

    pub fn source_code(&self, addr: u8, value: f64) -> f64 {
        let key = value.to_string();
        let matches = |action: &Action| matches!(action, Action::Word(_, v) if *v == value);
        let found = if addr == b'G' {
            if self
                .g_codes
                .get(&key)
                .is_some_and(|entry| matches(&entry.0))
            {
                return value;
            }
            self.g_codes
                .iter()
                .find(|(_, entry)| matches(&entry.0))
                .map(|(key, _)| key)
        } else if addr == b'M' {
            if self.m_codes.get(&key).is_some_and(matches) {
                return value;
            }
            self.m_codes
                .iter()
                .find(|(_, action)| matches(action))
                .map(|(key, _)| key)
        } else {
            None
        };
        found.and_then(|key| key.parse().ok()).unwrap_or(value)
    }
}
