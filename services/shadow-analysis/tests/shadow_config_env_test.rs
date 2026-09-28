//! Environment parsing for [`ShadowConfig::default`]: the Laya settings used by the
//! hallucination check (URL, key, timeout, model) and the guardrails timeout. Laya is
//! used only when `LAYA_URL` is set, so an unset URL means the check is skipped.
//!
//! These live in their own test binary because they mutate process-wide environment
//! variables; a separate binary means they cannot race the unit tests in `src`, and the
//! module-local mutex serialises them against each other.
//!
//! Run with: `cargo test -p controlplane-shadow-analysis --test shadow_config_env_test`

use std::sync::Mutex;

use controlplane_shadow_analysis::ShadowConfig;

/// Every variable `ShadowConfig::default` reads for Laya.
const JUDGE_KEYS: [&str; 5] = [
    "LAYA_URL",
    "LAYA_MODEL",
    "LAYA_TIMEOUT_MS",
    "LAYA_API_KEY",
    "TYPESAFE_API_KEY",
];

/// Serialises the tests in this binary, which all mutate the same process environment.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Applies a known environment for one test and restores the previous one on drop.
struct EnvGuard {
    saved: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    /// Clears every judge variable, then sets exactly the pairs given.
    fn set(vars: &[(&'static str, &str)]) -> Self {
        let saved = JUDGE_KEYS
            .iter()
            .map(|key| (*key, std::env::var(key).ok()))
            .collect();

        for key in JUDGE_KEYS {
            std::env::remove_var(key);
        }
        for (key, value) in vars {
            std::env::set_var(key, value);
        }

        Self { saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in &self.saved {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

/// Take the lock, recovering from a poisoned mutex so one failing test cannot cascade.
fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ─── The default-off guarantee ───────────────────────────────────────────────────

#[test]
fn with_nothing_set_laya_is_unconfigured() {
    let _lock = env_lock();
    let _env = EnvGuard::set(&[]);

    let config = ShadowConfig::default();

    // No LAYA_URL: the hallucination check is skipped ("Laya not configured").
    assert!(config.laya_url.is_none());
    assert!(config.laya_api_key.is_none());
    // The timeout still has a sane default, it is simply never used.
    assert_eq!(config.laya_timeout_ms, 10_000);
    assert!(config.laya_model.is_none());
}

#[test]
fn a_blank_laya_url_is_treated_as_unset() {
    let _lock = env_lock();

    for value in ["", "   "] {
        let _env = EnvGuard::set(&[("LAYA_URL", value)]);
        assert!(
            ShadowConfig::default().laya_url.is_none(),
            "LAYA_URL={value:?} is not a usable endpoint"
        );
    }
}

#[test]
fn a_configured_laya_url_is_carried_through() {
    let _lock = env_lock();
    let _env = EnvGuard::set(&[("LAYA_URL", "http://laya:8000")]);

    assert_eq!(
        ShadowConfig::default().laya_url.as_deref(),
        Some("http://laya:8000")
    );
}

#[test]
fn surrounding_whitespace_never_produces_a_configured_but_broken_laya() {
    // The judge fails open, so a value that merely *looks* configured is the dangerous
    // case: the pipeline reports no errors while the judge scores nothing at all. A
    // whitespace-padded value must behave exactly like the clean one.
    let _lock = env_lock();
    let _env = EnvGuard::set(&[
        ("LAYA_URL", "  http://laya:8000  "),
        ("LAYA_API_KEY", "  secret-key\n"),
        ("LAYA_TIMEOUT_MS", " 750 "),
        ("LAYA_MODEL", "  typed-decisions  "),
    ]);

    let config = ShadowConfig::default();

    assert_eq!(config.laya_url.as_deref(), Some("http://laya:8000"));
    assert_eq!(config.laya_api_key.as_deref(), Some("secret-key"));
    assert_eq!(config.laya_timeout_ms, 750);
    assert_eq!(config.laya_model.as_deref(), Some("typed-decisions"));
}

#[test]
fn the_judge_api_key_prefers_laya_then_falls_back_to_typesafe() {
    let _lock = env_lock();

    // Only the Jev key set: used as the bearer token for the Jev backend.
    {
        let _env = EnvGuard::set(&[("TYPESAFE_API_KEY", "jev-key")]);
        assert_eq!(
            ShadowConfig::default().laya_api_key.as_deref(),
            Some("jev-key")
        );
    }

    // Both set: the Laya-specific key wins.
    {
        let _env = EnvGuard::set(&[
            ("LAYA_API_KEY", "laya-key"),
            ("TYPESAFE_API_KEY", "jev-key"),
        ]);
        assert_eq!(
            ShadowConfig::default().laya_api_key.as_deref(),
            Some("laya-key")
        );
    }

    // A blank key is not a key.
    {
        let _env = EnvGuard::set(&[("LAYA_API_KEY", "  ")]);
        assert!(ShadowConfig::default().laya_api_key.is_none());
    }
}

#[test]
fn the_timeout_parses_and_falls_back_on_garbage() {
    let _lock = env_lock();

    {
        let _env = EnvGuard::set(&[("LAYA_TIMEOUT_MS", "750")]);
        assert_eq!(ShadowConfig::default().laya_timeout_ms, 750);
    }

    for value in ["", "soon", "-1", "1.5", "99999999999999999999"] {
        let _env = EnvGuard::set(&[("LAYA_TIMEOUT_MS", value)]);
        assert_eq!(
            ShadowConfig::default().laya_timeout_ms,
            10_000,
            "LAYA_TIMEOUT_MS={value:?} must not produce a bogus budget"
        );
    }
}

#[test]
fn auto_and_blank_model_overrides_let_the_router_decide() {
    let _lock = env_lock();

    // "auto" is the documented way to say "let Laya's Router pick the checkpoint", so it
    // must be sent as a genuine absence rather than as a literal model name. Casing and
    // padding must not turn the sentinel into a model named `AUTO`, which `laya-serve`
    // would reject — silently disabling the judge because it fails open.
    for value in ["auto", "AUTO", "Auto", " auto ", "\tauto\n", ""] {
        let _env = EnvGuard::set(&[("LAYA_MODEL", value)]);
        assert!(
            ShadowConfig::default().laya_model.is_none(),
            "LAYA_MODEL={value:?} should leave the choice to the Router"
        );
    }
}

#[test]
fn an_explicit_model_override_is_forwarded() {
    let _lock = env_lock();
    let _env = EnvGuard::set(&[("LAYA_MODEL", "typed-decisions")]);

    assert_eq!(
        ShadowConfig::default().laya_model.as_deref(),
        Some("typed-decisions")
    );
}

#[test]
fn guardrails_timeout_defaults_to_10s_and_is_configurable() {
    let _lock = env_lock();
    {
        let _env = EnvGuard::set(&[("GUARDRAILS_TIMEOUT_MS", "2500")]);
        assert_eq!(ShadowConfig::default().guardrails_timeout_ms, 2_500);
    }
    for value in ["", "soon", "0", "-1"] {
        let _env = EnvGuard::set(&[("GUARDRAILS_TIMEOUT_MS", value)]);
        assert_eq!(
            ShadowConfig::default().guardrails_timeout_ms,
            10_000,
            "GUARDRAILS_TIMEOUT_MS={value:?} must fall back to the 10 s default"
        );
    }
}

// ─── The guard is only useful if it can fail ─────────────────────────────────────

#[test]
fn the_env_harness_actually_restores_state() {
    // If this ever fails, every other assertion in this binary is suspect: it would mean
    // one test's environment leaked into the next.
    // Hold the lock for the whole test: reading `before` unlocked raced with other
    // tests in this binary that set LAYA_URL, making this test flaky.
    let _lock = env_lock();
    let before = std::env::var("LAYA_URL").ok();

    {
        let _env = EnvGuard::set(&[("LAYA_URL", "http://laya:8000")]);
        assert_eq!(ShadowConfig::default().laya_url.as_deref(), Some("http://laya:8000"));
    }

    assert_eq!(
        std::env::var("LAYA_URL").ok(),
        before,
        "the guard must restore the previous environment"
    );
}
