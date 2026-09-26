pub mod aggregator;
pub mod feedback;
pub mod policy;
pub mod policy_api;
pub mod router;

pub use aggregator::{
    base_detector, is_judge_detector, AggregationResult, AxisFusion, DetectorContribution,
    FusionConfig, FusionResult, VerdictAggregator, EVIDENCE_SUFFIX, JUDGE_DETECTOR_PREFIX,
};
pub use feedback::{annotate_from_precedents, find_similar_precedents, Precedent};
pub use policy::{PolicyEngine, ResolvedPolicy};
pub use policy_api::{policy_crud_router, PolicyApiState};
pub use router::{decision_router, spawn_verdict_collector, DecisionServiceState};
