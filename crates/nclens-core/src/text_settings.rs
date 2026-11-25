use crate::{dialect, post, DialectSettings, PostError, PostProfile};

/// Parse and validate a post from TOML without accessing a file.
pub fn post_from_str(src: &str) -> Result<PostProfile, PostError> {
    post::parse_settings(src, |profile: &PostProfile| profile.validate().err())
}

/// Validate a post and serialize it to TOML.
pub fn post_to_string(profile: &PostProfile) -> Result<String, PostError> {
    validate_post(profile)?;
    toml::to_string_pretty(profile).map_err(post::file_error)
}

/// Parse and validate a dialect from TOML without accessing a file.
pub fn dialect_from_str(src: &str) -> Result<DialectSettings, PostError> {
    post::parse_settings(src, |settings| {
        dialect::invalid_fields(settings).into_iter().next()
    })
}

/// Validate a dialect and serialize it to TOML.
pub fn dialect_to_string(settings: &DialectSettings) -> Result<String, PostError> {
    validate_dialect(settings)?;
    toml::to_string_pretty(settings).map_err(post::file_error)
}

/// Parse and validate a post from JSON.
pub fn post_from_json(src: &str) -> Result<PostProfile, PostError> {
    let profile = parse_json(src)?;
    validate_post(&profile)?;
    Ok(profile)
}

/// Validate a post and serialize it to JSON.
pub fn post_to_json(profile: &PostProfile) -> Result<String, PostError> {
    validate_post(profile)?;
    serde_json::to_string_pretty(profile).map_err(post::file_error)
}

/// Parse and validate a dialect from JSON.
pub fn dialect_from_json(src: &str) -> Result<DialectSettings, PostError> {
    let settings = parse_json(src)?;
    validate_dialect(&settings)?;
    Ok(settings)
}

/// Validate a dialect and serialize it to JSON.
pub fn dialect_to_json(settings: &DialectSettings) -> Result<String, PostError> {
    validate_dialect(settings)?;
    serde_json::to_string_pretty(settings).map_err(post::file_error)
}

fn parse_json<T: serde::de::DeserializeOwned>(src: &str) -> Result<T, PostError> {
    serde_json::from_str(src).map_err(|error| PostError {
        line: Some(error.line()),
        message: error.to_string(),
    })
}

fn validate_post(profile: &PostProfile) -> Result<(), PostError> {
    profile
        .validate()
        .map_err(|field| post::file_error(format!("Invalid post setting {}", field.join("."))))
}

fn validate_dialect(settings: &DialectSettings) -> Result<(), PostError> {
    if let Some(problem) = dialect::validate_dialect(settings).into_iter().next() {
        return Err(post::file_error(problem));
    }
    Ok(())
}

fn json_extension(path: &std::path::Path) -> Result<bool, PostError> {
    let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    if extension.eq_ignore_ascii_case("json") {
        Ok(true)
    } else if extension.eq_ignore_ascii_case("toml") {
        Ok(false)
    } else {
        Err(post::file_error("Expected a .toml or .json settings file"))
    }
}

pub(crate) fn read_settings<T>(
    path: impl AsRef<std::path::Path>,
    toml: fn(&str) -> Result<T, PostError>,
    json: fn(&str) -> Result<T, PostError>,
) -> Result<T, PostError> {
    let path = path.as_ref();
    let hinted_json = json_extension(path)?;
    let src = std::fs::read_to_string(path).map_err(post::file_error)?;
    let src = src.trim_start_matches('\u{feff}');
    if src.trim_start().starts_with('{') || (hinted_json && src.trim().is_empty()) {
        json(src)
    } else {
        toml(src)
    }
}

pub(crate) fn write_settings<T>(
    path: impl AsRef<std::path::Path>,
    value: &T,
    toml: fn(&T) -> Result<String, PostError>,
    json: fn(&T) -> Result<String, PostError>,
) -> Result<(), PostError> {
    let path = path.as_ref();
    let src = if json_extension(path)? {
        json(value)?
    } else {
        toml(value)?
    };
    post::save_settings(path, &src)
}
