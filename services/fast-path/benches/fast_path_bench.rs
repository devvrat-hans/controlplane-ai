use criterion::{black_box, criterion_group, criterion_main, Criterion};

use controlplane_fast_path::{FastPathEngine, FastPathRuleSet, PolicyCache};

fn sample_clean_response() -> &'static str {
    r#"{"content":[{"type":"text","text":"The capital of France is Paris. It is known for the Eiffel Tower, the Louvre Museum, and its rich cultural heritage. Paris has been a major European city for centuries and continues to be a global center for art, fashion, and gastronomy."}],"usage":{"input_tokens":15,"output_tokens":48}}"#
}

fn sample_response_with_secret() -> &'static str {
    r#"{"content":[{"type":"text","text":"Here is the API key you requested: AKIAIOSFODNN7EXAMPLE. Use it to authenticate with the AWS S3 service."}],"usage":{"input_tokens":20,"output_tokens":30}}"#
}

fn sample_response_with_pii() -> &'static str {
    r#"{"content":[{"type":"text","text":"The patient John Smith (SSN: 123-45-6789) lives at 742 Evergreen Terrace. His email is john.smith@example.com and his credit card is 4532015112830366."}],"usage":{"input_tokens":25,"output_tokens":40}}"#
}

fn sample_unsafe_response() -> &'static str {
    r#"{"content":[{"type":"text","text":"To create a bomb, first you need to gather the following materials and assemble them carefully in a remote location."}],"usage":{"input_tokens":10,"output_tokens":25}}"#
}

fn bench_fast_path_clean(c: &mut Criterion) {
    let policy_cache = PolicyCache::new(FastPathRuleSet::default());
    let engine = FastPathEngine::new(policy_cache);
    let body = sample_clean_response();

    c.bench_function("fast_path_clean_response", |b| {
        b.iter(|| engine.evaluate(black_box(body)))
    });
}

fn bench_fast_path_secret_detection(c: &mut Criterion) {
    let policy_cache = PolicyCache::new(FastPathRuleSet::default());
    let engine = FastPathEngine::new(policy_cache);
    let body = sample_response_with_secret();

    c.bench_function("fast_path_secret_detection", |b| {
        b.iter(|| engine.evaluate(black_box(body)))
    });
}

fn bench_fast_path_pii_detection(c: &mut Criterion) {
    let policy_cache = PolicyCache::new(FastPathRuleSet::default());
    let engine = FastPathEngine::new(policy_cache);
    let body = sample_response_with_pii();

    c.bench_function("fast_path_pii_detection", |b| {
        b.iter(|| engine.evaluate(black_box(body)))
    });
}

fn bench_fast_path_unsafe_block(c: &mut Criterion) {
    let mut rules = FastPathRuleSet::default();
    rules.unsafe_keywords = vec!["bomb".to_string()];
    let policy_cache = PolicyCache::new(rules);
    let engine = FastPathEngine::new(policy_cache);
    let body = sample_unsafe_response();

    c.bench_function("fast_path_unsafe_block", |b| {
        b.iter(|| engine.evaluate(black_box(body)))
    });
}

fn bench_fast_path_large_response(c: &mut Criterion) {
    let policy_cache = PolicyCache::new(FastPathRuleSet::default());
    let engine = FastPathEngine::new(policy_cache);

    // Simulate a larger response (~4KB)
    let paragraph = "This is a sample paragraph of text that represents typical AI model output. It contains various sentences about different topics including technology, science, and general knowledge. ";
    let large_body = format!(
        r#"{{"content":[{{"type":"text","text":"{}"}}],"usage":{{"input_tokens":100,"output_tokens":500}}}}"#,
        paragraph.repeat(20)
    );

    c.bench_function("fast_path_4kb_response", |b| {
        b.iter(|| engine.evaluate(black_box(&large_body)))
    });
}

criterion_group!(
    benches,
    bench_fast_path_clean,
    bench_fast_path_secret_detection,
    bench_fast_path_pii_detection,
    bench_fast_path_unsafe_block,
    bench_fast_path_large_response,
);
criterion_main!(benches);
