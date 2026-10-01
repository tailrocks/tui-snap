use super::{TestContext, get};
use std::collections::HashMap;
use std::process::Command;

/// Read-only correlation key joining attempt artifacts to nextest results (N05, N10).
///
/// `testsuite` is the nextest binary id (`name` on each `JUnit` `testsuite`),
/// `testcase` is the test name, and `run_uuid` is the `uuid` on the `JUnit`
/// `testsuites` root. This helper only *reads* identity for correlation; it
/// never rewrites pass/fail status and never invents test entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JunitKey {
    /// `NEXTEST_RUN_ID`.
    pub run_uuid: String,
    /// `NEXTEST_BINARY_ID`.
    pub testsuite: String,
    /// `NEXTEST_TEST_NAME`.
    pub testcase: String,
}

impl JunitKey {
    /// Correlation key for this process, or `None` outside nextest (or when any
    /// of the three identity vars is missing — partial keys are refused).
    #[must_use]
    pub fn current() -> Option<Self> {
        let env: HashMap<String, String> = std::env::vars().collect();
        Self::from_map(&env)
    }

    /// [`JunitKey::current`] over an injected environment.
    ///
    /// ```
    /// # use std::collections::HashMap;
    /// # use tuiscotti_runtime::runner::JunitKey;
    /// assert!(JunitKey::from_map(&HashMap::new()).is_none());
    /// let mut env = HashMap::new();
    /// env.insert("NEXTEST_RUN_ID".into(), "run-1".into());
    /// env.insert("NEXTEST_BINARY_ID".into(), "my-crate::integration".into());
    /// env.insert("NEXTEST_TEST_NAME".into(), "renders_empty".into());
    /// let key = JunitKey::from_map(&env).unwrap();
    /// assert!(key.matches("my-crate::integration", "renders_empty"));
    /// assert!(!key.matches("my-crate::integration", "other"));
    /// assert!(key.run_matches("run-1"));
    /// ```
    #[must_use]
    pub fn from_map(env: &HashMap<String, String>) -> Option<Self> {
        Some(Self {
            run_uuid: get(env, "NEXTEST_RUN_ID")?,
            testsuite: get(env, "NEXTEST_BINARY_ID")?,
            testcase: get(env, "NEXTEST_TEST_NAME")?,
        })
    }

    /// Do `JUnit` `testsuite`/`testcase` names identify this attempt's test?
    #[must_use]
    pub fn matches(&self, testsuite: &str, testcase: &str) -> bool {
        self.testsuite == testsuite && self.testcase == testcase
    }

    /// Does a `JUnit` root `uuid` identify this attempt's run?
    #[must_use]
    pub fn run_matches(&self, run_uuid: &str) -> bool {
        self.run_uuid == run_uuid
    }
}

/// Build a child [`Command`] with the context's env applied, without touching
/// the parent environment or working directory.
pub fn child_command(ctx: &TestContext, program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program);
    ctx.apply_to(&mut cmd);
    cmd
}

#[cfg(test)]
mod tests {
    use crate::runner::*;

    #[test]
    fn binary_id_shapes() {
        assert_eq!(
            parse_binary_id(Some("my-crate"), None),
            ("my-crate".to_string(), "my-crate".to_string())
        );
        assert_eq!(
            parse_binary_id(Some("my-crate::integration"), None),
            ("my-crate".to_string(), "integration".to_string())
        );
        assert_eq!(
            parse_binary_id(Some("my-crate::bench/perf"), None),
            ("my-crate".to_string(), "bench/perf".to_string())
        );
        assert_eq!(
            parse_binary_id(None, Some("pkg")),
            ("pkg".to_string(), "pkg".to_string())
        );
        assert_eq!(
            parse_binary_id(None, None),
            (UNKNOWN_PACKAGE.to_string(), UNKNOWN_PACKAGE.to_string())
        );
    }

    #[test]
    fn json_roundtrip() {
        let s = "a\"b\\c\nd\te\x01f";
        let line = format!("{{\"event\":\"x\",\"detail\":\"{}\"}}", json_escape(s));
        assert_eq!(extract_field(&line, "detail").as_deref(), Some(s));
        assert_eq!(extract_field(&line, "event").as_deref(), Some("x"));
        assert!(extract_field(&line, "nope").is_none());
    }

    #[test]
    fn local_run_ids_unique() {
        let a = generate_local_run_id();
        let b = generate_local_run_id();
        assert_ne!(a, b);
        assert!(a.starts_with("local-"));
    }
}
