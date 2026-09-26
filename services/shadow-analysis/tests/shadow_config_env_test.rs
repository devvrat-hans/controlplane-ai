//! The judge's master switch is an environment variable — and the guarantee that keeps
//! existing deployments unchanged is that **unset means off**.
//!
//! `docs/analysis/laya-integration-plan.md` §7 states it as: "`DECISION_JUDGE=off` ⇒
//! today's pipeline exactly; zero behavioural change". That is a property of
//! [`ShadowConfig::default`], and it is the difference between "we added an optional
//! model" and "we changed everyone's governance pipeline".
//!
//! These live in their own test binary because they mutate process-wide environment
//! variables; a separate binary means they cannot race the unit tests in `src`, and the
//! module-local mutex serialises them against each other.
//!
//! Run with: `cargo test -p controlplane-shadow-analysis --test shadow_config_env_test`

use std::sync::Mutex;

use controlplane_shadow_analysis::ShadowConfig;

/// Every variable `ShadowConfig::default` reads for the judge.
const JUDGE_KEYS: [&str; 6] = [
    "DECISION_JUDGE",
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
fn with_nothing_set_the_judge_is_disabled() {
    let _lock = env_lock();
    let _env = EnvGuard::set(&[]);

    let config = ShadowConfig::default();

    assert!(
        !config.decision_judge_enabled,
        "an unset DECISION_JUDGE must leave the shadow path exactly as it was"
    );
    assert!(config.laya_url.is_none());
    assert!(config.laya_api_key.is_none());
    // The timeout still has a sane default, it is simply never used.
    assert_eq!(config.laya_timeout_ms, 5_000);
    assert!(config.laya_model.is_none());
}

#[test]
fn setting_the_backend_to_laya_or_jev_enables_the_judge() {
    let _lock = env_lock();

    for backend in ["laya", "jev", "LAYA", "Jev", "  laya  ", "\tjev\n"] {
        let _env = EnvGuard::set(&[("DECISION_JUDGE", backend)]);
        assert!(
            ShadowConfig::default().decision_judge_enabled,
            "DECISION_JUDGE={backend:?} should enable the judge"
        );
    }
}

#[test]
fn any_other_value_leaves_the_judge_off() {
    let _lock = env_lock();

    for value in [
        "off", "OFF", "none", "", " ", "true", "1", "yes", "laya2", "jevish",
    ] {
        let _env = EnvGuard::set(&[("DECISION_JUDGE", value)]);
        assert!(
            !ShadowConfig::default().decision_judge_enabled,
            "DECISION_JUDGE={value:?} must not enable the judge — only laya/jev do"
        );
    }
}

#[test]
fn enabling_the_judge_requires_no_other_variable() {
    // Enabling it is sufficient; LAYA_URL is what decides whether a call is made, and the
    // worker skips the judge with a warning when it is missing rather than failing.
    let _lock = env_lock();
    let _env = EnvGuard::set(&[("DECISION_JUDGE", "laya")]);

    let config = ShadowConfig::default();
    assert!(config.decision_judge_enabled);
    assert!(config.laya_url.is_none());
}

// ─── URL / key / model parsing ───────────────────────────────────────────────────

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
fn surrounding_whitespace_never_produces_a_configured_but_broken_judge() {
    // The judge fails open, so a value that merely *looks* configured is the dangerous
    // case: the pipeline reports no errors while the judge scores nothing at all. A
    // whitespace-padded value must behave exactly like the clean one.
    let _lock = env_lock();
    let _env = EnvGuard::set(&[
        ("DECISION_JUDGE", "  laya\n"),
        ("LAYA_URL", "  http://laya:8000  "),
        ("LAYA_API_KEY", "  secret-key\n"),
        ("LAYA_TIMEOUT_MS", " 750 "),
        ("LAYA_MODEL", "  typed-decisions  "),
    ]);

    let config = ShadowConfig::default();

    assert!(config.decision_judge_enabled);
    assert_eq!(config.laya_url.as_deref(), Some("http://laya:8000"));
    assert_eq!(config.laya_api_key.as_deref(), Some("secret-key"));
    assert_eq!(config.laya_timeout_ms, 750);
    assert_eq!(config.laya_model.as_deref(), Some("typed-decisions"));
}

#[test]
fn the_judge_api_key_prefers_laya_then_falls_back_to_typesafe() {
    let _lock = env_lock();

    // Only the Jev key set: used as the bearer token for DECISION_JUDGE=jev.
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
            5_000,
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

// ─── The guard is only useful if it can fail ─────────────────────────────────────

#[test]
fn the_env_harness_actually_restores_state() {
    // If this ever fails, every other assertion in this binary is suspect: it would mean
    // one test's environment leaked into the next.
    let before = std::env::var("DECISION_JUDGE").ok();

    {
        let _lock = env_lock();
        let _env = EnvGuard::set(&[("DECISION_JUDGE", "laya")]);
        assert!(ShadowConfig::default().decision_judge_enabled);
    }

    assert_eq!(
        std::env::var("DECISION_JUDGE").ok(),
        before,
        "the guard must restore the previous environment"
    );
}
