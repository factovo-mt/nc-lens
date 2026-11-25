use std::{ffi::OsString, path::PathBuf};

use nclens_cli::{parse_set, read_settings, read_text, Config, Error};
use nclens_core::{post_from_str, PostProfile};
use serde::Serialize;
use serde_json::Value;

#[derive(Default)]
pub struct Startup {
    input: Option<PathBuf>,
    post: Option<PostProfile>,
    pub settings: Option<(PathBuf, Value)>,
    overrides: Vec<(String, Value)>,
}

impl Startup {
    pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Self, Error> {
        let mut startup = Self::default();
        let mut args = args.into_iter();
        let mut positional = false;
        while let Some(arg) = args.next() {
            if !positional && arg == "--" {
                positional = true;
                continue;
            }
            if !positional && arg.to_string_lossy().starts_with("--") {
                let value = args.next().ok_or_else(|| {
                    Error::config(format!("引数の値がありません：{}", arg.to_string_lossy()))
                })?;
                match arg.to_str() {
                    Some("--post") if startup.post.is_none() => {
                        startup.post = Some(
                            post_from_str(&read_text(&PathBuf::from(value))?)
                                .map_err(Error::config)?,
                        );
                    }
                    Some("--settings") if startup.settings.is_none() => {
                        let path = PathBuf::from(value);
                        let settings = read_settings(&path)?;
                        startup.settings = Some((path, settings));
                    }
                    Some("--set") => {
                        startup
                            .overrides
                            .push(parse_set(value.to_str().ok_or_else(|| {
                                Error::config("--set の値を UTF-8 として読めません")
                            })?)?)
                    }
                    _ => {
                        return Err(Error::config(format!(
                            "無効な引数：{}",
                            arg.to_string_lossy()
                        )))
                    }
                }
            } else if startup.input.replace(PathBuf::from(&arg)).is_some() {
                return Err(Error::config("起動時に開ける NC ファイルは 1 個です"));
            }
        }
        Ok(startup)
    }

    pub fn session_config(&self) -> bool {
        self.settings.is_some() || self.post.is_some() || !self.overrides.is_empty()
    }

    pub fn resolve(
        &self,
        post: Option<PostProfile>,
        switches: nclens_cli::AppSwitches,
    ) -> Result<Launch, Error> {
        let config = if self.post.is_some() || !self.overrides.is_empty() {
            Some(Config::resolve(
                self.post.clone().or(post).unwrap_or_default(),
                switches,
                self.overrides.clone(),
            )?)
        } else {
            None
        };
        Ok(Launch {
            path: self
                .input
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            post: config.as_ref().map(|c| c.post.clone()),
            optional_stop: config.map(|c| c.optional_stop),
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Launch {
    path: Option<String>,
    post: Option<PostProfile>,
    optional_stop: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use nclens_cli::AppSwitches;

    #[test]
    fn startup_applies_overrides_to_selected_post() {
        let args = [
            "--set",
            "variables.work.G54.x=10",
            "--set",
            "expand.block_skip=false",
            "--",
            "NC jobs/part.ncd",
        ];
        let startup = Startup::parse(args.map(OsString::from)).unwrap();
        let launch = startup
            .resolve(
                Some(PostProfile {
                    name: "Selected".into(),
                    ..PostProfile::default()
                }),
                AppSwitches {
                    block_skip: Some(true),
                    optional_stop: Some(true),
                },
            )
            .unwrap();
        assert_eq!(launch.path.as_deref(), Some("NC jobs/part.ncd"));
        assert_eq!(launch.optional_stop, Some(true));
        let post = launch.post.unwrap();
        assert_eq!(post.name, "Selected");
        assert!(!post.expand.block_skip);
        assert_eq!(post.variables.work[0][0], 10.);
    }
}
