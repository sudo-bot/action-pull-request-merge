//! Read action inputs from environment variables.
//!
//! GitHub Actions exposes each `inputs.<name>` value as the environment
//! variable `INPUT_<NAME>` where `<NAME>` is the input name uppercased with
//! spaces (the runner does **not** translate hyphens; it simply uppercases).
//!
//! See the runner source: <https://github.com/actions/runner/blob/main/src/Runner.Worker/Container/ContainerInfo.cs>
//! and the `@actions/core` implementation: <https://github.com/actions/toolkit/blob/main/packages/core/src/core.ts>

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::env;

/// Match `@actions/core` getInput name normalisation: replace spaces with
/// underscores and uppercase, then prefix with `INPUT_`.
pub fn input_env_name(name: &str) -> String {
    format!("INPUT_{}", name.replace(' ', "_").to_uppercase())
}

/// Source of inputs. Production code uses [`EnvSource`]; tests use
/// [`MapSource`].
pub trait InputSource {
    fn get(&self, name: &str) -> Option<String>;
}

pub struct EnvSource;

impl InputSource for EnvSource {
    fn get(&self, name: &str) -> Option<String> {
        env::var(input_env_name(name)).ok()
    }
}

#[derive(Default, Debug, Clone)]
pub struct MapSource {
    pub values: HashMap<String, String>,
}

impl MapSource {
    pub fn new<I, K, V>(it: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        Self {
            values: it.into_iter().map(|(k, v)| (k.into(), v.into())).collect(),
        }
    }
}

impl InputSource for MapSource {
    fn get(&self, name: &str) -> Option<String> {
        self.values.get(name).cloned()
    }
}

/// Strongly typed inputs to the action. Mirrors `action.yml`.
#[derive(Debug, Clone)]
pub struct ActionInputs {
    pub github_token: String,
    pub number: u64,
    pub merge_method: MergeMethod,
    pub allowed_usernames_regex: String,
    pub filter_label: String,
    pub merge_title: String,
    pub merge_message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
    FastForward,
}

impl MergeMethod {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "merge" => Ok(Self::Merge),
            "squash" => Ok(Self::Squash),
            "rebase" => Ok(Self::Rebase),
            "fast-forward" => Ok(Self::FastForward),
            other => Err(anyhow!(
                "invalid merge-method '{}': expected one of merge, squash, rebase, fast-forward",
                other
            )),
        }
    }
}

fn required<S: InputSource>(src: &S, name: &str) -> Result<String> {
    let value = src
        .get(name)
        .ok_or_else(|| anyhow!("Input required and not supplied: {}", name))?;
    if value.is_empty() {
        return Err(anyhow!("Input required and not supplied: {}", name));
    }
    Ok(value)
}

fn optional<S: InputSource>(src: &S, name: &str, default: &str) -> String {
    src.get(name)
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

impl ActionInputs {
    pub fn from_source<S: InputSource>(src: &S) -> Result<Self> {
        let github_token = required(src, "github-token")?;
        let number_raw = required(src, "number")?;
        let number: u64 = number_raw
            .trim()
            .parse()
            .map_err(|e| anyhow!("Input 'number' must be a positive integer: {}", e))?;

        let merge_method_raw = optional(src, "merge-method", "merge");
        let merge_method = MergeMethod::parse(&merge_method_raw)?;

        let allowed_usernames_regex = optional(src, "allowed-usernames-regex", "^.*$");
        let filter_label = optional(src, "filter-label", "");
        let merge_title = optional(src, "merge-title", "");
        let merge_message = optional(src, "merge-message", "");

        Ok(Self {
            github_token,
            number,
            merge_method,
            allowed_usernames_regex,
            filter_label,
            merge_title,
            merge_message,
        })
    }

    pub fn from_env() -> Result<Self> {
        Self::from_source(&EnvSource)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(pairs: &[(&str, &str)]) -> MapSource {
        MapSource::new(pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())))
    }

    #[test]
    fn input_env_name_uppercases_and_keeps_hyphens() {
        // Mirror @actions/core: spaces -> underscores, uppercase. Hyphens
        // remain as-is even though env vars technically dislike them.
        assert_eq!(input_env_name("github-token"), "INPUT_GITHUB-TOKEN");
        assert_eq!(input_env_name("merge method"), "INPUT_MERGE_METHOD");
    }

    #[test]
    fn parses_full_input_set() {
        let s = src(&[
            ("github-token", "ghp_abc"),
            ("number", "42"),
            ("merge-method", "squash"),
            ("allowed-usernames-regex", "^bot$"),
            ("filter-label", "merge-it"),
            ("merge-title", "Title"),
            ("merge-message", "Body"),
        ]);
        let inputs = ActionInputs::from_source(&s).unwrap();
        assert_eq!(inputs.github_token, "ghp_abc");
        assert_eq!(inputs.number, 42);
        assert_eq!(inputs.merge_method, MergeMethod::Squash);
        assert_eq!(inputs.allowed_usernames_regex, "^bot$");
        assert_eq!(inputs.filter_label, "merge-it");
        assert_eq!(inputs.merge_title, "Title");
        assert_eq!(inputs.merge_message, "Body");
    }

    #[test]
    fn applies_defaults_when_optional_missing() {
        let s = src(&[("github-token", "ghp_abc"), ("number", "1")]);
        let inputs = ActionInputs::from_source(&s).unwrap();
        assert_eq!(inputs.merge_method, MergeMethod::Merge);
        assert_eq!(inputs.allowed_usernames_regex, "^.*$");
        assert_eq!(inputs.filter_label, "");
        assert_eq!(inputs.merge_title, "");
        assert_eq!(inputs.merge_message, "");
    }

    #[test]
    fn rejects_missing_token() {
        let s = src(&[("number", "1")]);
        let err = ActionInputs::from_source(&s).unwrap_err().to_string();
        assert!(err.contains("github-token"), "got: {}", err);
    }

    #[test]
    fn rejects_empty_required_input() {
        let s = src(&[("github-token", ""), ("number", "1")]);
        let err = ActionInputs::from_source(&s).unwrap_err().to_string();
        assert!(err.contains("github-token"));
    }

    #[test]
    fn rejects_non_numeric_number() {
        let s = src(&[("github-token", "x"), ("number", "abc")]);
        let err = ActionInputs::from_source(&s).unwrap_err().to_string();
        assert!(err.contains("number"));
    }

    #[test]
    fn rejects_unknown_merge_method() {
        let s = src(&[
            ("github-token", "x"),
            ("number", "1"),
            ("merge-method", "smush"),
        ]);
        let err = ActionInputs::from_source(&s).unwrap_err().to_string();
        assert!(err.contains("merge-method"));
    }

    #[test]
    fn merge_method_parses_all_variants() {
        assert_eq!(MergeMethod::parse("merge").unwrap(), MergeMethod::Merge);
        assert_eq!(MergeMethod::parse("squash").unwrap(), MergeMethod::Squash);
        assert_eq!(MergeMethod::parse("rebase").unwrap(), MergeMethod::Rebase);
        assert_eq!(
            MergeMethod::parse("fast-forward").unwrap(),
            MergeMethod::FastForward
        );
        assert!(MergeMethod::parse("nope").is_err());
    }
}
