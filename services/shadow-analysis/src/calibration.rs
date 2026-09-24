//! Detector calibration — temperature scaling for judge probabilities.
//!
//! See `docs/analysis/laya-integration-plan.md` §5.4 (temperature fitting) and §9
//! (calibrated probabilities are what the verdict's `confidence` carries).
//!
//! ## Why this exists
//!
//! Laya's own model card reports raw ECE of 0.213–0.466 on its benchmark. A raw
//! probability of 0.9 from an over-confident model does not mean "9 times in 10"; it
//! means "the model is excited". Every threshold in the pipeline (0.70 edit / 0.90
//! escalate) is only meaningful once those probabilities are calibrated.
//!
//! A single scalar per (detector, primitive, option-count bucket) is enough:
//!
//! ```text
//! p_calibrated = sigmoid( logit(p_raw) / temperature )
//! ```
//!
//! `temperature > 1` softens an over-confident distribution; `temperature < 1`
//! sharpens an under-confident one. `temperature = 1` is the identity.
//!
//! ## Fail-safe by construction
//!
//! [`Calibration::inert`] (the default everywhere) applies no transformation at all,
//! so nothing in this module changes behaviour until a fit has actually been written
//! to `detector_calibration` with `calibrated = TRUE`. Any database error while
//! loading degrades to `inert` rather than failing the shadow path.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use sqlx::PgPool;
use tracing::{debug, info, warn};

/// The primitive a probability came from. Laya's `choice` and `score` heads have
/// different calibration error profiles, so they are fitted separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Primitive {
    Choice,
    Score,
}

impl Primitive {
    pub fn as_str(&self) -> &'static str {
        match self {
            Primitive::Choice => "choice",
            Primitive::Score => "score",
        }
    }
}

/// Identifies one fitted temperature.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ScaleKey {
    detector: String,
    primitive: &'static str,
    bucket: &'static str,
}

/// Bucket an option count. Calibration error grows with the number of options
/// (accuracy collapses past ~20 at the default option budget), so option counts are
/// pooled into coarse buckets rather than fitted one-by-one — with a hackathon-sized
/// label corpus, per-count fits would be pure noise.
pub fn option_bucket(option_count: usize) -> &'static str {
    match option_count {
        0..=2 => "2",
        3..=5 => "3-5",
        6..=10 => "6-10",
        _ => "11-20",
    }
}

fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

/// Numerically-safe logit, clamped away from the asymptotes so a reported
/// probability of exactly 0.0 or 1.0 cannot produce an infinite logit.
fn logit(p: f64) -> f64 {
    const EPSILON: f64 = 1e-6;
    let p = p.clamp(EPSILON, 1.0 - EPSILON);
    (p / (1.0 - p)).ln()
}

/// Apply a temperature to a probability.
///
/// Returns `p` unchanged for a non-finite or non-positive temperature, and for the
/// identity `1.0` — so a malformed fitted parameter can never silently distort a
/// decision. Always returns a value in `[0, 1]`.
pub fn apply_temperature(p: f64, temperature: f64) -> f64 {
    if !p.is_finite() {
        return p.clamp(0.0, 1.0);
    }
    let p = p.clamp(0.0, 1.0);
    if !temperature.is_finite() || temperature <= 0.0 || (temperature - 1.0).abs() < f64::EPSILON {
        return p;
    }
    sigmoid(logit(p) / temperature).clamp(0.0, 1.0)
}

/// Fitted calibration parameters for the detectors that have one.
///
/// Cloning is cheap (a handful of entries) and the store is read once per shadow
/// analysis, not per check.
#[derive(Debug, Clone, Default)]
pub struct Calibration {
    temperatures: HashMap<ScaleKey, f64>,
    weights: HashMap<String, f64>,
    version: Option<i32>,
}

impl Calibration {
    /// No transformation, no weights. The default state of the system.
    pub fn inert() -> Self {
        Self::default()
    }

    /// True when no fitted parameter is loaded — the fusion must not engage.
    pub fn is_inert(&self) -> bool {
        self.temperatures.is_empty() && self.weights.is_empty()
    }

    /// Version of the fit that produced these parameters, for the audit trail.
    pub fn version(&self) -> Option<i32> {
        self.version
    }

    /// Insert one fitted temperature. Exposed so the evaluation harness and tests can
    /// build a calibration without a database.
    pub fn insert_temperature(
        &mut self,
        detector: &str,
        primitive: Primitive,
        option_count: usize,
        temperature: f64,
    ) {
        self.temperatures.insert(
            ScaleKey {
                detector: detector.to_string(),
                primitive: primitive.as_str(),
                bucket: option_bucket(option_count),
            },
            temperature,
        );
    }

    /// Insert one fitted reliability weight.
    pub fn insert_weight(&mut self, detector: &str, weight: f64) {
        self.weights.insert(detector.to_string(), weight.clamp(0.0, 1.0));
    }

    pub fn set_version(&mut self, version: Option<i32>) {
        self.version = version;
    }

    /// Fitted temperature for a detector, or `1.0` (the identity) when unfitted.
    pub fn temperature_for(
        &self,
        detector: &str,
        primitive: Primitive,
        option_count: usize,
    ) -> f64 {
        self.temperatures
            .get(&ScaleKey {
                detector: detector.to_string(),
                primitive: primitive.as_str(),
                bucket: option_bucket(option_count),
            })
            .copied()
            .unwrap_or(1.0)
    }

    /// Fitted reliability weight for a detector, if one was fitted.
    pub fn weight_for(&self, detector: &str) -> Option<f64> {
        self.weights.get(detector).copied()
    }

    /// Every fitted weight, for building the decision engine's fusion config.
    pub fn weights(&self) -> &HashMap<String, f64> {
        &self.weights
    }

    /// Calibrate a raw detector probability.
    pub fn apply(
        &self,
        detector: &str,
        primitive: Primitive,
        option_count: usize,
        p_raw: f64,
    ) -> f64 {
        apply_temperature(p_raw, self.temperature_for(detector, primitive, option_count))
    }
}

/// Row shape read from `detector_calibration`.
#[derive(Debug, sqlx::FromRow)]
struct CalibrationRow {
    detector: String,
    primitive: String,
    option_count: i32,
    temperature: f64,
    weight: f64,
    version: i32,
}

/// Load the fitted calibration rows from PostgreSQL.
///
/// Fail-open: any database error returns an inert calibration (identity), never an
/// error. A shadow path that cannot read its calibration must behave exactly as it
/// did before the feature existed.
pub async fn load_calibration(pool: &PgPool) -> Calibration {
    // The latest version wins: a re-fit writes a new `version`, and older rows stay
    // for the audit trail. `DISTINCT ON` selects the highest version per key.
    let rows: Result<Vec<CalibrationRow>, sqlx::Error> = sqlx::query_as(
        r#"
        SELECT DISTINCT ON (detector, primitive, option_count)
               detector, primitive, option_count, temperature, weight, version
        FROM detector_calibration
        WHERE calibrated = TRUE
        ORDER BY detector, primitive, option_count, version DESC
        "#,
    )
    .fetch_all(pool)
    .await;

    let rows = match rows {
        Ok(rows) => rows,
        Err(e) => {
            warn!(error = %e, "Could not load detector calibration — using raw probabilities");
            return Calibration::inert();
        }
    };

    if rows.is_empty() {
        debug!("No calibrated detectors configured — judge probabilities stay raw");
        return Calibration::inert();
    }

    let mut calibration = Calibration::inert();
    let mut version: Option<i32> = None;

    for row in rows {
        let primitive = match row.primitive.as_str() {
            "score" => Primitive::Score,
            _ => Primitive::Choice,
        };
        calibration.insert_temperature(
            &row.detector,
            primitive,
            row.option_count.max(0) as usize,
            row.temperature,
        );
        calibration.insert_weight(&row.detector, row.weight);
        version = Some(version.map_or(row.version, |v: i32| v.max(row.version)));
    }

    calibration.set_version(version);
    debug!(
        detectors = calibration.weights().len(),
        version = ?version,
        "Detector calibration loaded"
    );
    calibration
}

/// Shared, lock-free holder for the fitted calibration, mirroring `ToggleStore`.
///
/// The shadow path reads it on every analysis; a background task refreshes it so a
/// re-fit (which writes a new `version` to `detector_calibration`) takes effect without
/// restarting the gateway.
#[derive(Clone, Default)]
pub struct CalibrationStore {
    inner: Arc<ArcSwap<Calibration>>,
}

impl CalibrationStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(ArcSwap::from_pointee(Calibration::inert())),
        }
    }

    pub fn load(&self) -> Arc<Calibration> {
        self.inner.load_full()
    }

    pub fn store(&self, calibration: Calibration) {
        self.inner.store(Arc::new(calibration));
    }

    /// Load the latest fit and swap it in. Returns `true` when the version changed
    /// (or when a fit appeared/disappeared), so the caller can log the transition.
    ///
    /// Fail-open: a database error leaves the previously loaded calibration in place.
    pub async fn refresh(&self, pool: &PgPool) -> bool {
        let next = load_calibration(pool).await;
        let previous_version = self.load().version();
        let changed = previous_version != next.version();
        self.store(next);
        changed
    }
}

/// Background task: load the fit at startup, then poll for a new version.
///
/// Polling (rather than an event) is deliberate: a calibration fit is an offline batch
/// job with no event subject of its own, and this mirrors the existing policy-reloader
/// pattern. The query is a single indexed lookup against a tiny table.
pub fn spawn_calibration_reloader(
    pool: PgPool,
    store: CalibrationStore,
    interval: Duration,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        if store.refresh(&pool).await {
            info!(version = ?store.load().version(), "Detector calibration loaded");
        }

        let mut ticker = tokio::time::interval(interval.max(Duration::from_secs(1)));
        ticker.tick().await; // the immediate first tick was handled above

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    if store.refresh(&pool).await {
                        info!(
                            version = ?store.load().version(),
                            "Detector calibration changed — fusion parameters reloaded"
                        );
                    }
                }
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        info!("Calibration reloader shutting down");
                        break;
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temperature_one_is_the_identity() {
        for p in [0.0, 0.01, 0.3, 0.5, 0.77, 0.99, 1.0] {
            assert!(
                (apply_temperature(p, 1.0) - p).abs() < 1e-12,
                "identity must be exact for {p}"
            );
        }
    }

    #[test]
    fn temperature_above_one_softens_extremes_toward_the_middle() {
        // An over-confident 0.95 must move *down* — that is the whole point of the fit.
        let softened = apply_temperature(0.95, 3.0);
        assert!(softened < 0.95, "expected softening, got {softened}");
        assert!(softened > 0.5, "expected to stay on the positive side, got {softened}");
    }

    #[test]
    fn temperature_below_one_sharpens() {
        let sharpened = apply_temperature(0.7, 0.5);
        assert!(sharpened > 0.7, "expected sharpening, got {sharpened}");
    }

    #[test]
    fn calibration_is_monotonic_so_threshold_bands_stay_ordered() {
        let mut previous = 0.0;
        for step in 0..=20 {
            let p = step as f64 / 20.0;
            let calibrated = apply_temperature(p, 2.5);
            assert!(
                calibrated >= previous - 1e-12,
                "monotonicity broken at {p}: {calibrated} < {previous}"
            );
            previous = calibrated;
        }
    }

    #[test]
    fn calibration_keeps_endpoints_on_their_own_side_of_the_middle() {
        // A probability of exactly 0 or 1 cannot stay exact through a logit, but it must
        // not flip sides: a confident "clean" reading must never become a finding.
        for temperature in [0.25, 1.0, 2.0, 5.0] {
            let low = apply_temperature(0.0, temperature);
            let high = apply_temperature(1.0, temperature);

            assert!((0.0..=1.0).contains(&low), "low out of range: {low}");
            assert!((0.0..=1.0).contains(&high), "high out of range: {high}");
            assert!(low < 0.5, "0.0 must stay below the middle at T={temperature}: {low}");
            assert!(high > 0.5, "1.0 must stay above the middle at T={temperature}: {high}");
        }
    }

    #[test]
    fn malformed_temperatures_are_ignored_rather_than_guessed() {
        assert_eq!(apply_temperature(0.8, 0.0), 0.8);
        assert_eq!(apply_temperature(0.8, -2.0), 0.8);
        assert_eq!(apply_temperature(0.8, f64::NAN), 0.8);
        assert_eq!(apply_temperature(0.8, f64::INFINITY), 0.8);
    }

    #[test]
    fn option_buckets_group_option_counts() {
        assert_eq!(option_bucket(2), "2");
        assert_eq!(option_bucket(3), "3-5");
        assert_eq!(option_bucket(5), "3-5");
        assert_eq!(option_bucket(6), "6-10");
        assert_eq!(option_bucket(11), "11-20");
        assert_eq!(option_bucket(40), "11-20");
    }

    #[test]
    fn inert_calibration_is_inert_and_identity() {
        let calibration = Calibration::inert();
        assert!(calibration.is_inert());
        assert!(calibration.version().is_none());

        for p in [0.0, 0.45, 0.7, 0.9, 1.0] {
            let applied = calibration.apply("laya-bias", Primitive::Choice, 2, p);
            assert!(
                (applied - p).abs() < 1e-12,
                "inert calibration must not move {p} (got {applied})"
            );
        }
    }

    #[test]
    fn fitted_temperature_only_applies_to_its_own_bucket() {
        let mut calibration = Calibration::inert();
        calibration.insert_temperature("laya-bias", Primitive::Choice, 2, 3.0);
        assert!(!calibration.is_inert());

        // Same detector + primitive + option count -> fitted temperature applies.
        let fitted = calibration.apply("laya-bias", Primitive::Choice, 2, 0.95);
        assert!(fitted < 0.95, "expected the fit to apply, got {fitted}");

        // A different option-count bucket falls back to the identity.
        let unfitted_bucket = calibration.apply("laya-bias", Primitive::Choice, 7, 0.95);
        assert!((unfitted_bucket - 0.95).abs() < 1e-12);

        // A different primitive falls back to the identity too.
        let unfitted_primitive = calibration.apply("laya-bias", Primitive::Score, 2, 0.95);
        assert!((unfitted_primitive - 0.95).abs() < 1e-12);

        // An unknown detector falls back to the identity.
        let unfitted_detector = calibration.apply("laya-nope", Primitive::Choice, 2, 0.95);
        assert!((unfitted_detector - 0.95).abs() < 1e-12);
    }

    #[test]
    fn temperature_lookup_defaults_to_one() {
        let calibration = Calibration::inert();
        assert_eq!(
            calibration.temperature_for("anything", Primitive::Choice, 2),
            1.0
        );
        assert!(calibration.weight_for("anything").is_none());
    }

    #[test]
    fn weights_are_clamped_to_a_valid_range() {
        let mut calibration = Calibration::inert();
        calibration.insert_weight("laya-bias", 1.8);
        calibration.insert_weight("laya-toxicity", -0.5);

        assert_eq!(calibration.weight_for("laya-bias"), Some(1.0));
        assert_eq!(calibration.weight_for("laya-toxicity"), Some(0.0));
    }

    #[test]
    fn store_starts_inert_and_can_be_replaced() {
        let store = CalibrationStore::new();
        assert!(store.load().is_inert());
        assert!(store.load().version().is_none());

        let mut fitted = Calibration::inert();
        fitted.insert_weight("laya-bias", 0.4);
        fitted.set_version(Some(7));
        store.store(fitted);

        assert!(!store.load().is_inert());
        assert_eq!(store.load().version(), Some(7));
    }

    #[test]
    fn store_is_shared_across_clones() {
        let store = CalibrationStore::new();
        let reader = store.clone();

        let mut fitted = Calibration::inert();
        fitted.set_version(Some(3));
        store.store(fitted);

        assert_eq!(reader.load().version(), Some(3));
    }
}
