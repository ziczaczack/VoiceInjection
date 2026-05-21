use regex::{escape, RegexBuilder};
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DictRule {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub case_insensitive: bool,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

pub fn apply(text: &str, rules: &[DictRule]) -> String {
    let mut out = text.to_string();
    for rule in rules {
        if !rule.enabled || rule.from.is_empty() {
            continue;
        }
        if rule.case_insensitive {
            match RegexBuilder::new(&escape(&rule.from))
                .case_insensitive(true)
                .build()
            {
                Ok(re) => {
                    out = re.replace_all(&out, rule.to.as_str()).into_owned();
                }
                Err(e) => {
                    log::warn!("dictionary regex build failed for '{}': {e}", rule.from);
                }
            }
        } else {
            out = out.replace(&rule.from, &rule.to);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(from: &str, to: &str, ci: bool) -> DictRule {
        DictRule {
            from: from.into(),
            to: to.into(),
            case_insensitive: ci,
            enabled: true,
        }
    }

    #[test]
    fn plain_replace() {
        let r = vec![rule("react js", "React.js", false)];
        assert_eq!(apply("i love react js a lot", &r), "i love React.js a lot");
    }

    #[test]
    fn case_insensitive() {
        let r = vec![rule("typescript", "TypeScript", true)];
        assert_eq!(apply("TypeScript and TYPESCRIPT", &r), "TypeScript and TypeScript");
    }

    #[test]
    fn disabled_skipped() {
        let mut rr = rule("a", "b", false);
        rr.enabled = false;
        assert_eq!(apply("a a a", &std::slice::from_ref(&rr)), "a a a");
    }

    #[test]
    fn regex_metachars_escaped() {
        let r = vec![rule("c++", "C++", false)];
        assert_eq!(apply("i know c++", &r), "i know C++");
    }
}
