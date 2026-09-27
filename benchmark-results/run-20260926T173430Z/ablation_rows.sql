WITH det AS (
    SELECT v.call_id, v.axis, v.check_name, v.outcome, v.confidence,
           regexp_replace(v.check_name, '-evidence$', '') AS base_name,
           CASE WHEN v.check_name LIKE 'laya-%' THEN 'judge' ELSE 'heuristic' END AS family
    FROM verdicts v
    WHERE v.created_at > NOW() - (3650::int * INTERVAL '1 day')
),
weights AS (
    SELECT detector, MAX(weight) AS weight FROM detector_calibration WHERE calibrated = TRUE GROUP BY detector
),
temperatures AS (
    SELECT DISTINCT ON (detector) detector, temperature FROM detector_calibration
    WHERE calibrated = TRUE ORDER BY detector, version DESC
),
scored AS (
    SELECT d.call_id, d.axis, d.family, d.outcome, d.confidence,
           LEAST(0.9999, GREATEST(0.0, COALESCE(w.weight, 1.0) * d.confidence)) AS weighted
    FROM det d
    LEFT JOIN weights w ON w.detector = d.base_name
    LEFT JOIN temperatures t ON t.detector = d.base_name
),
labels AS (
    SELECT DISTINCT ON (ro.call_id, ro.axis) ro.call_id, ro.axis,
           CASE WHEN ro.reviewer_action = 'confirm' THEN 1 ELSE 0 END AS y
    FROM reviewer_overrides ro
    WHERE ro.created_at > NOW() - (3650::int * INTERVAL '1 day')
    ORDER BY ro.call_id, ro.axis, ro.created_at DESC
),
predictions AS (
    SELECT call_id, axis, 'heuristic-only' AS config, MAX(confidence) AS p
    FROM scored WHERE family = 'heuristic' AND outcome <> 'pass' GROUP BY call_id, axis
    UNION ALL
    SELECT call_id, axis, 'judge-only', MAX(confidence)
    FROM scored WHERE family = 'judge' AND outcome <> 'pass' GROUP BY call_id, axis
    UNION ALL
    SELECT call_id, axis, 'fused', 1.0 - EXP(SUM(LN(GREATEST(1e-9, 1.0 - weighted))))
    FROM scored WHERE confidence >= 0.45 GROUP BY call_id, axis
)
SELECT l.axis, c.config, l.y, COALESCE(pr.p, 0)
FROM labels l
CROSS JOIN (SELECT unnest(ARRAY['heuristic-only','judge-only','fused']) AS config) c
LEFT JOIN predictions pr ON pr.call_id = l.call_id AND pr.axis = l.axis AND pr.config = c.config
ORDER BY l.axis, c.config;
