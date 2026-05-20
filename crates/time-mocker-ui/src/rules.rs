//! Auto-inject pattern rules — glob or regex matched against process path and name.

use anyhow::{anyhow, Result};
use globset::{Glob, GlobMatcher};
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PatternKind {
    Glob,
    Regex,
}

impl PatternKind {
    pub fn label(self) -> &'static str {
        match self {
            PatternKind::Glob => "Glob",
            PatternKind::Regex => "Regex",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub pattern: String,
    pub kind: PatternKind,
    pub enabled: bool,
}

#[derive(Default)]
pub struct CompiledRules {
    matchers: Vec<Matcher>,
}

enum Matcher {
    Glob(GlobMatcher),
    Regex(Regex),
}

impl Matcher {
    fn is_match(&self, s: &str) -> bool {
        match self {
            Matcher::Glob(g) => g.is_match(s),
            Matcher::Regex(r) => r.is_match(s),
        }
    }
}

impl CompiledRules {
    pub fn compile(rules: &[Rule]) -> Self {
        let matchers = rules
            .iter()
            .filter(|r| r.enabled)
            .filter_map(|r| compile_one(r).ok())
            .collect();
        Self { matchers }
    }

    pub fn matches(&self, path: &str, name: &str) -> bool {
        self.matchers
            .iter()
            .any(|m| m.is_match(path) || m.is_match(name))
    }
}

fn compile_one(rule: &Rule) -> Result<Matcher> {
    match rule.kind {
        PatternKind::Glob => {
            let g = Glob::new(&rule.pattern).map_err(|e| anyhow!("glob: {e}"))?;
            Ok(Matcher::Glob(g.compile_matcher()))
        }
        PatternKind::Regex => Ok(Matcher::Regex(
            Regex::new(&rule.pattern).map_err(|e| anyhow!("regex: {e}"))?,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_matches_name() {
        let rules = vec![Rule {
            pattern: "*chrome*".into(),
            kind: PatternKind::Glob,
            enabled: true,
        }];
        let c = CompiledRules::compile(&rules);
        assert!(c.matches("", "chrome.exe"));
        assert!(!c.matches("", "firefox.exe"));
    }

    #[test]
    fn regex_matches_path() {
        let rules = vec![Rule {
            pattern: r"^.*\\MyApp\.exe$".into(),
            kind: PatternKind::Regex,
            enabled: true,
        }];
        let c = CompiledRules::compile(&rules);
        assert!(c.matches(r"C:\foo\MyApp.exe", "MyApp.exe"));
        assert!(!c.matches(r"C:\foo\Other.exe", "Other.exe"));
    }

    #[test]
    fn disabled_rules_skipped() {
        let rules = vec![Rule {
            pattern: "*".into(),
            kind: PatternKind::Glob,
            enabled: false,
        }];
        let c = CompiledRules::compile(&rules);
        assert!(!c.matches("anything", "anything"));
    }
}
