-- Detector calibration store (Laya integration plan §5.4 / §11.2, Phase 3)
--
-- Every detector in the hybrid panel contributes a calibrated probability and a
-- reliability weight. Both are *fitted offline* against the ground-truth labels in
-- `reviewer_overrides` (migration 021) and stored here, versioned, so the audit trail
-- can record which calibration produced a decision.
--
--   p_calibrated = sigmoid( logit(p_raw) / temperature )
--   p_axis       = 1 - PRODUCT( 1 - weight * p_calibrated )
--
-- IMPORTANT — fail-safe default: a row only takes effect when `calibrated = TRUE`.
-- The table ships empty, so with no fit having been run the decision engine keeps using
-- its pre-existing aggregator and the pipeline behaves exactly as it did before
-- (see docs/analysis/laya-integration-plan.md §7 "Fail-open matrix").

CREATE TABLE IF NOT EXISTS detector_calibration (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Detector identity as it appears in `verdicts.check_name`
    -- (e.g. 'laya-bias', 'laya-prompt-injection', 'llm-guard-toxicity').
    detector      TEXT NOT NULL,
    -- Primitive the probability came from: 'choice' | 'score'.
    primitive     TEXT NOT NULL DEFAULT 'choice',
    -- Number of options / rubric levels, used to bucket temperature fits
    -- (calibration error depends on the option count, per Laya's own guidance).
    option_count  INT NOT NULL DEFAULT 2,
    -- Fitted parameters.
    temperature   DOUBLE PRECISION NOT NULL DEFAULT 1.0,
    weight        DOUBLE PRECISION NOT NULL DEFAULT 0.3,
    -- FALSE marks a documented default that must NOT change decision behaviour.
    -- Only `calibrated = TRUE` rows enable the fusion path.
    calibrated    BOOLEAN NOT NULL DEFAULT FALSE,
    version       INT NOT NULL DEFAULT 1,
    fitted_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (detector, primitive, option_count, version)
);

CREATE INDEX IF NOT EXISTS idx_detector_calibration_active
    ON detector_calibration (detector, primitive, option_count)
    WHERE calibrated = TRUE;

-- Documented seed defaults for the detectors that exist today. `calibrated = FALSE`
-- means these are inert: they exist so an operator can see the shape of the table and
-- so the fitting job has a starting point, without altering any current decision.
INSERT INTO detector_calibration (detector, primitive, option_count, temperature, weight, calibrated, version)
VALUES
    ('laya-bias',             'choice', 2, 1.0, 0.50, FALSE, 1),
    ('laya-prompt-injection', 'choice', 2, 1.0, 0.50, FALSE, 1),
    ('laya-hallucination',    'choice', 2, 1.0, 0.50, FALSE, 1),
    ('laya-groundedness',     'score',  3, 1.0, 0.50, FALSE, 1),
    ('laya-toxicity',         'score',  3, 1.0, 0.30, FALSE, 1),
    ('laya-semantic-pii',     'choice', 2, 1.0, 0.50, FALSE, 1),
    ('laya-tool-use',         'choice', 5, 1.0, 0.50, FALSE, 1),
    ('laya-verbosity',        'score',  3, 1.0, 0.20, FALSE, 1),
    ('prompt_injection',      'score',  1, 1.0, 0.40, FALSE, 1),
    ('bias_classification',   'score',  1, 1.0, 0.20, FALSE, 1),
    ('groundedness',          'score',  1, 1.0, 0.20, FALSE, 1),
    ('semantic_pii',          'score',  1, 1.0, 0.20, FALSE, 1),
    ('presidio-pii',          'score',  1, 1.0, 0.80, FALSE, 1),
    ('llm-guard-toxicity',    'score',  1, 1.0, 0.80, FALSE, 1),
    ('input-bias',            'score',  1, 1.0, 0.70, FALSE, 1),
    ('deepeval-hallucination','score',  1, 1.0, 0.30, FALSE, 1)
ON CONFLICT (detector, primitive, option_count, version) DO NOTHING;
