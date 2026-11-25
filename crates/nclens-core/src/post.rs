pub use crate::duplicate::{DuplicateM, DuplicateWord};
use serde::{
    de::{DeserializeSeed, IgnoredAny, MapAccess, Visitor},
    Deserialize, Serialize,
};
use std::{
    collections::BTreeMap,
    fmt, fs,
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PostProfile {
    #[serde(rename = "mnemonic")]
    pub dialect: crate::DialectSettings,
    pub version: u32,
    pub name: String,
    pub comment: String,
    pub codes: CodeSettings,
    pub variables: VariableSettings,
    pub expand: ExpandSettings,
}

impl Default for PostProfile {
    fn default() -> Self {
        Self {
            dialect: crate::DialectSettings::default(),
            version: 1,
            name: "Default".into(),
            comment: String::new(),
            codes: CodeSettings::default(),
            variables: VariableSettings::default(),
            expand: ExpandSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegerUnits {
    Millimeters,
    #[default]
    LeastIncrement,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompensationType {
    #[serde(rename = "FANUC-A")]
    A,
    #[serde(rename = "FANUC-B")]
    B,
    #[default]
    #[serde(rename = "FANUC-C")]
    C,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolChange {
    T,
    #[default]
    M6,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MacroVariables {
    #[default]
    #[serde(rename = "15M")]
    M15,
    #[serde(rename = "16M")]
    M16,
    #[serde(rename = "6M")]
    M6,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CodeSettings {
    pub duplicate_word: DuplicateWord,
    pub warn_on_duplicate: bool,
    pub duplicate_g_group: DuplicateWord,
    pub max_m_per_block: usize,
    pub duplicate_m: DuplicateM,
    pub rapid_mm_min: f64,
    pub max_rpm: f64,
    pub atc_sec: f64,
    pub compensation_sec: f64,
    /// 集計に工具中心の補正経路を使う。
    pub measure_compensated_path: bool,
    pub g73_retract_mm: f64,
    pub g83_clearance_mm: f64,
    pub decimal_places: u8,
    pub integer_units: IntegerUnits,
    pub compensation: CompensationType,
    pub tool_change: ToolChange,
    pub macro_variables: MacroVariables,
    /// Keys are decimal M and G code numbers.
    pub m_calls: BTreeMap<String, u32>,
    pub g_calls: BTreeMap<String, u32>,
    /// T calls O9000 with its value in #149.
    pub t_call: bool,
    /// S calls O9029 with its value in #147.
    pub s_call: bool,
    /// B calls O9028 with its value in #146.
    pub b_call: bool,
    pub ignore_g: Vec<f64>,
    pub ignore_m: Vec<u32>,
    pub program_address: char,
}

impl Default for CodeSettings {
    fn default() -> Self {
        Self {
            duplicate_word: DuplicateWord::Last,
            warn_on_duplicate: true,
            duplicate_g_group: DuplicateWord::Last,
            max_m_per_block: 3,
            duplicate_m: DuplicateM::All,
            rapid_mm_min: 5000.,
            max_rpm: 8000.,
            atc_sec: 0.,
            compensation_sec: 0.,
            measure_compensated_path: false,
            g73_retract_mm: 0.5,
            g83_clearance_mm: 1.,
            decimal_places: 3,
            integer_units: IntegerUnits::default(),
            compensation: CompensationType::default(),
            tool_change: ToolChange::default(),
            macro_variables: MacroVariables::default(),
            m_calls: BTreeMap::new(),
            g_calls: BTreeMap::new(),
            t_call: false,
            s_call: false,
            b_call: false,
            ignore_g: Vec::new(),
            ignore_m: Vec::new(),
            program_address: 'O',
        }
    }
}

impl CodeSettings {
    pub(crate) fn input_scale(&self) -> f64 {
        match self.integer_units {
            IntegerUnits::Millimeters => 1.,
            IntegerUnits::LeastIncrement => 10_f64.powi(-(self.decimal_places as i32)),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct VariableSettings {
    /// Decimal variable numbers, without the # prefix.
    pub common: BTreeMap<String, f64>,
    /// G54 through G59 in order. Each row is X, Y, Z in mm.
    pub work: [[f64; 3]; 6],
    pub external: [f64; 3],
    /// Decimal tool compensation numbers.
    pub offsets: BTreeMap<String, ToolOffset>,
    pub g28: [f64; 3],
    /// P2 through P4 in order.
    pub g30: [[f64; 3]; 3],
    pub tools: BTreeMap<String, ToolInfo>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ToolOffset {
    pub h_geometry: f64,
    pub h_wear: f64,
    pub d_geometry: f64,
    pub d_wear: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ToolInfo {
    pub diameter_mm: Option<f64>,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DistanceMode {
    #[default]
    Preserve,
    Absolute,
    Incremental,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ExpandSettings {
    pub cycles: bool,
    pub g10: bool,
    pub decimal_point: bool,
    pub omit_zero_moves: bool,
    pub distance_mode: DistanceMode,
    pub block_skip: bool,
    pub block_skip_levels: Vec<u8>,
}

impl Default for ExpandSettings {
    fn default() -> Self {
        Self {
            cycles: false,
            g10: false,
            decimal_point: true,
            omit_zero_moves: false,
            distance_mode: DistanceMode::Preserve,
            block_skip: false,
            block_skip_levels: vec![1],
        }
    }
}

impl ExpandSettings {
    pub fn enabled_block_skip_levels(&self) -> &[u8] {
        if self.block_skip {
            &self.block_skip_levels
        } else {
            &[]
        }
    }
}

#[derive(Debug)]
pub struct PostError {
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for PostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(line) = self.line {
            write!(f, "line {line}: ")?;
        }
        f.write_str(&self.message)
    }
}

impl std::error::Error for PostError {}

pub const DEFAULT_POST_TOML: &str = include_str!("../assets/default.post.toml");

pub fn load_post(path: impl AsRef<Path>) -> Result<PostProfile, PostError> {
    crate::text_settings::read_settings(path, crate::post_from_str, crate::post_from_json)
}

pub(crate) fn parse_settings<T: serde::de::DeserializeOwned>(
    src: &str,
    invalid: impl for<'a> FnOnce(&'a T) -> Option<Vec<&'a str>>,
) -> Result<T, PostError> {
    let value = toml::from_str(src).map_err(|error: toml::de::Error| PostError {
        line: Some(line_number(src, error.span().map_or(0, |s| s.start))),
        message: error.message().into(),
    })?;
    if let Some(field) = invalid(&value) {
        let start = SettingSpan(&field)
            .deserialize(toml::Deserializer::new(src))
            .ok()
            .flatten()
            .unwrap_or(0);
        return Err(PostError {
            line: Some(line_number(src, start)),
            message: format!("Invalid setting {}", field.join(".")),
        });
    }
    Ok(value)
}

pub fn save_post(path: impl AsRef<Path>, profile: &PostProfile) -> Result<(), PostError> {
    crate::text_settings::write_settings(path, profile, crate::post_to_string, crate::post_to_json)
}

pub(crate) fn save_settings(path: impl AsRef<Path>, src: &str) -> Result<(), PostError> {
    let path = path.as_ref();
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let (staged, mut file) = loop {
        let seq = NEXT.fetch_add(1, Ordering::Relaxed);
        let staged =
            path.with_file_name(format!(".nclens-settings-{}-{seq}.tmp", std::process::id()));
        match fs::File::create_new(&staged) {
            Ok(file) => break (staged, file),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(file_error(err)),
        }
    };
    let result = file
        .write_all(src.as_bytes())
        .and_then(|()| file.sync_all());
    drop(file);
    let result = result.and_then(|()| fs::rename(&staged, path));
    if result.is_err() {
        let _ = fs::remove_file(&staged);
    }
    result.map_err(file_error)
}

pub(crate) fn file_error(error: impl fmt::Display) -> PostError {
    PostError {
        line: None,
        message: error.to_string(),
    }
}

fn line_number(src: &str, byte: usize) -> usize {
    src.as_bytes()[..byte.min(src.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
        + 1
}

// Ask the TOML deserializer for the span after semantic validation identifies a field.
// Walking keys preserves the spans of dotted keys, inline tables and quoted keys.
struct SettingSpan<'a>(&'a [&'a str]);

impl<'de> DeserializeSeed<'de> for SettingSpan<'_> {
    type Value = Option<usize>;

    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        if self.0.is_empty() {
            return toml::Spanned::<toml::Value>::deserialize(deserializer)
                .map(|value| Some(value.span().start));
        }
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for SettingSpan<'_> {
    type Value = Option<usize>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a post setting table")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
        while let Some(key) = map.next_key::<String>()? {
            if key == self.0[0] {
                return map.next_value_seed(SettingSpan(&self.0[1..]));
            }
            map.next_value::<IgnoredAny>()?;
        }
        Ok(None)
    }
}

impl PostProfile {
    /// Returns the path of the first invalid field.
    pub fn validate(&self) -> Result<(), Vec<&str>> {
        if let Some(mut field) = crate::dialect::invalid_fields(&self.dialect)
            .into_iter()
            .next()
        {
            field.insert(0, "mnemonic");
            return Err(field);
        }
        if self
            .expand
            .block_skip_levels
            .iter()
            .any(|n| !(1..=9).contains(n))
        {
            return Err(vec!["expand", "block_skip_levels"]);
        }
        let c = &self.codes;
        let v = &self.variables;
        if self.version != 1 {
            return Err(vec!["version"]);
        }
        if self.name.trim().is_empty() {
            return Err(vec!["name"]);
        }
        for (key, val) in [
            ("rapid_mm_min", c.rapid_mm_min),
            ("max_rpm", c.max_rpm),
            ("atc_sec", c.atc_sec),
            ("compensation_sec", c.compensation_sec),
            ("g73_retract_mm", c.g73_retract_mm),
            ("g83_clearance_mm", c.g83_clearance_mm),
        ] {
            if !val.is_finite() || val < 0. || (key == "rapid_mm_min" && val == 0.) {
                return Err(vec!["codes", key]);
            }
        }
        if c.decimal_places > 9 {
            return Err(vec!["codes", "decimal_places"]);
        }
        if !matches!(c.program_address, 'O' | ':' | 'L') {
            return Err(vec!["codes", "program_address"]);
        }
        if c.ignore_g.iter().any(|g| !g.is_finite() || *g < 0.) {
            return Err(vec!["codes", "ignore_g"]);
        }
        for (key, calls, range, fractional) in [
            ("m_calls", &c.m_calls, 9001..=9009, false),
            ("g_calls", &c.g_calls, 9010..=9999, true),
        ] {
            for (code, p) in calls {
                if !range.contains(p)
                    || !code.parse::<f64>().is_ok_and(|n| {
                        n.is_finite()
                            && n >= 0.
                            && (fractional || n.fract() == 0.)
                            && n.to_string() == *code
                    })
                {
                    return Err(vec!["codes", key, code]);
                }
            }
        }
        for (n, val) in &v.common {
            if !n
                .parse::<u32>()
                .is_ok_and(|id| matches!(id, 100..=199 | 500..=999) && id.to_string() == *n)
                || !val.is_finite()
            {
                return Err(vec!["variables", "common", n]);
            }
        }
        for (key, values) in [
            ("work", v.work.as_flattened()),
            ("external", &v.external),
            ("g28", &v.g28),
            ("g30", v.g30.as_flattened()),
        ] {
            if values.iter().any(|x| !x.is_finite()) {
                return Err(vec!["variables", key]);
            }
        }
        for (n, value) in &v.offsets {
            if !tool_number(n) {
                return Err(vec!["variables", "offsets", n]);
            }
            for (key, val) in [
                ("h_geometry", value.h_geometry),
                ("h_wear", value.h_wear),
                ("d_geometry", value.d_geometry),
                ("d_wear", value.d_wear),
            ] {
                if !val.is_finite() {
                    return Err(vec!["variables", "offsets", n, key]);
                }
            }
        }
        for (n, value) in &v.tools {
            if !tool_number(n) {
                return Err(vec!["variables", "tools", n]);
            }
            if value.diameter_mm.is_some_and(|x| !x.is_finite() || x < 0.) {
                return Err(vec!["variables", "tools", n, "diameter_mm"]);
            }
        }
        Ok(())
    }
}

fn tool_number(n: &str) -> bool {
    n.parse::<u32>()
        .is_ok_and(|id| (1..=999).contains(&id) && id.to_string() == n)
}
