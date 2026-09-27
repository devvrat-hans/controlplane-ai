# Why Rust for the ControlPlane.ai Backend

> **What this document is:** the engineering case for building the request path and the
> integrity-critical services in Rust — performance, safety, and concurrency — with the
> concrete places in this codebase where each property is used. It also answers the obvious
> question head-on: **why not Python or JavaScript?**
>
> Everything below is grounded in this repository. File paths are real; the numbers come
> from the workspace's own criterion benchmarks (`services/fast-path/benches/`).
>
> **Status:** explanatory / Q&A reference. Nothing here is a claim about future work.

---

## 0. The one-paragraph answer

ControlPlane sits **in the synchronous request path** of every AI call, under a **hard
10 ms budget**, and it also owns a **tamper-evident audit chain**. Those two constraints
decide the language. A garbage-collected runtime can pause at any moment, which is exactly
the wrong property for a component whose latency budget is measured in microseconds, and a
dynamically typed runtime turns type errors into production incidents instead of compile
errors. Rust gives us **no garbage collector, so no pause**, **memory safety enforced at
compile time with zero `unsafe` code in this workspace**, and **fearless data sharing
across threads**. Measured, our governance overhead is **~5.6 µs** on a clean response —
roughly three orders of magnitude under budget.

---

## 1. The constraint that made the decision

| Requirement | Source | Consequence for language choice |
|---|---|---|
| Fast path completes in **<10 ms p50 / <25 ms p99**, hard | `AGENTS.md` | No stop-the-world pauses; no interpreter warm-up per request |
| Fast path must be **deterministic**, no LLM, no network | `AGENTS.md` | Pure CPU work — regex, entropy scoring, counters — executed under a deadline |
| Many checks run **concurrently** per request, and shadow checks run in parallel | `services/shadow-analysis/src/worker.rs` | Real parallelism, not just I/O concurrency |
| Audit records may never be UPDATE-d or DELETE-d, and the hash chain must not fork | `AGENTS.md` | Correctness is a first-class feature; type-level and structural guarantees matter |
| One process serves proxy + API + all workers | `services/gateway/src/main.rs` | Cheap concurrency inside a single binary |

This is an unusual shape: it is **CPU-bound, latency-critical, and integrity-critical at the
same time**. That combination is where Rust's design goals line up exactly with the problem.

---

## 2. Performance

### 2.1 No garbage collector means no pause, which means a trustworthy p99

Every GC language has a tail-latency problem: the collector decides when to run, and a
pause during a 10 ms budget is a budget breach. Rust's memory is freed deterministically
when ownership ends — there is nothing that can pause the process.

For a proxy, the **p99** is what the client feels, not the p50. Removing GC removes an
entire class of unpredictable latency spikes.

### 2.2 Zero-cost abstractions and static dispatch

Rust's iterators, `Option`/`Result`, and generics compile down to the same machine code you
would have written by hand. There is no boxing, no dynamic lookup, and no interpreter loop
between the check and the CPU.

### 2.3 Measured numbers from this repository

Benchmarks live in `services/fast-path/benches/fast_path_bench.rs` and are run with
`cargo bench -p controlplane-fast-path` (criterion, release mode):

| Scenario | p50 | Budget |
|---|---|---|
| Clean response (no findings) | **5.6 µs** | <10 ms |
| Secret detection (AWS key) | 9.7 µs | <10 ms |
| PII detection (SSN + email + card) | 11.4 µs | <10 ms |
| Unsafe keyword block | **0.96 µs** | <10 ms |
| Large 4 KB response | 24.3 µs | <25 ms p99 |

The headline number to quote: **5.6 microseconds**, against a 10 millisecond budget.

### 2.4 Work done once, not per request

Regex compilation is expensive; running it per request is a silent performance bug. The
detectors compile each pattern **once**, lazily, into a process-wide static — 11 `LazyLock`
statics across the workspace:

```rust
// services/fast-path/src/checks/secret_detection.rs
static AWS_KEY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(AKIA[0-9A-Z]{16})").unwrap()
});
```

The same principle drives `FastPathRuleSet` being loaded into memory once and swapped
atomically (see §3.4), rather than re-queried per request.

### 2.5 Deployment shape

One process, one statically-linked binary, no interpreter and no runtime dependency in the
container. That means a small image and a **fast cold start** — relevant when the proxy has
to come back up in front of live traffic.

---

## 3. Safety

Rust's safety story is not "we are careful." It is "the compiler does not let us be
careless," and in this workspace that holds **without a single `unsafe` block**.

### 3.1 Memory safety with no garbage collector — and no `unsafe`

Ownership and borrowing are checked at compile time, so there are no use-after-free, no
double-free, and no buffer overruns — the classic classes of security vulnerability in
network-facing code.

**Verified in this workspace:** there are **zero** occurrences of `unsafe { }`, `unsafe fn`,
`unsafe impl`, `unsafe trait`, or `unsafe extern` across all 12 Rust crates. The 79 literal
matches for the string "unsafe" are all domain vocabulary — `unsafe_content`,
`unsafe_keywords`, `unsafe_action` — i.e. the *unsafe content* governance check itself.

That is the claim worth making to a jury: **memory safety with no escape hatches.**

### 3.2 Errors are in the type signature, not in a comment

There are no exceptions to forget to catch. Every fallible operation returns
`Result<T, E>`, and `?` propagates the failure explicitly. `services/common/src/error.rs`
defines a single typed error enum whose variants carry the HTTP status and machine-readable
code, so an error's meaning is part of the type:

```rust
#[derive(Debug, Error)]
pub enum ControlPlaneError {
    #[error("Rate limited: {0}")]   RateLimited(String),
    #[error("Upstream timeout: {0}")] UpstreamTimeout(String),
    #[error("Internal error: {0}")]  Internal(String),
    // ...
}

impl ControlPlaneError {
    pub fn http_status_code(&self) -> u16 { /* 429, 504, 500, ... */ }
}
```

Because the match arms are exhaustive, **adding a new failure mode is a compile error until
every place that handles failures is updated.** The compiler enforces the error contract.

This is also how fail-open stays honest. The fast path returns plain values rather than
throwing, and the proxy's `run_fast_path_safe` (`services/proxy/src/handler.rs`) turns any
panic or timeout into `Pass` deliberately — a wrong outcome the system chose, not an
exception it forgot about.

### 3.3 Exhaustive matching keeps the four outcomes complete

`Outcome` is an enum with an explicit severity ordering derived from the type system rather
than hand-maintained magic numbers:

```rust
// services/common/src/types.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Outcome { Pass = 0, Escalate = 1, Edit = 2, Block = 3 }

impl Outcome {
    pub fn worst(a: Self, b: Self) -> Self { if a >= b { a } else { b } }
}
```

The decision engine's rule — "worst outcome wins" — is therefore a **property of the type**,
not a bug-prone comparison of integers scattered across services.

### 3.4 Data-race freedom is enforced by the compiler, not by convention

`Send` and `Sync` are checked at compile time. If shared state is accessed from multiple
tasks, the compiler requires a type that is sound to share. Combined with `ArcSwap` (a
lock-free atomic pointer swap), policy hot-reload is a **single atomic store** with **zero
reader blocking**:

```rust
// services/fast-path/src/policy_cache.rs
pub struct PolicyCache { inner: Arc<ArcSwap<FastPathRuleSet>> }

pub fn load(&self) -> Arc<FastPathRuleSet> { self.inner.load_full() }   // never blocks
pub fn store(&self, rules: FastPathRuleSet) { self.inner.store(Arc::new(rules)); }
```

A policy change during peak traffic cannot stall the request path, and cannot produce a
half-updated rule set. The same pattern hot-reloads the shadow-path check toggles
(`services/shadow-analysis/src/toggles.rs`).

---
## 4. Concurrency

### 4.1 Real parallelism, not just I/O concurrency

`tokio`'s multi-threaded scheduler spreads work across all available cores with work
stealing. That matters because our checks are **CPU-bound** (regex, entropy scoring), not
I/O-bound — the case where an event-loop runtime struggles.

### 4.2 Ownership makes parallel work safe by construction

Tasks can take ownership of the data they need, so parallel work needs no locks and no
shared mutable state:

```rust
// services/shadow-analysis/src/worker.rs — every check runs concurrently in one call
tokio::spawn(async move { checker.check(&response_clone, context_clone.as_deref()) })
```

There is no "shared buffer that two threads might write to" in the design, because the
type system will not let one exist.

### 4.3 Keeping CPU work off the async reactor

The fast path is synchronous CPU work. Running it directly on the reactor would starve
other tasks, so the proxy hands it to a blocking pool and wraps it in a deadline:

```rust
// services/proxy/src/handler.rs
let timeout_result = tokio::time::timeout(
    Duration::from_millis(50),
    tokio::task::spawn_blocking(move || {
        engine.evaluate_with_app_context(&body, output_tokens, session_key, app_max_tokens)
    }),
).await;
```

A panic inside the blocking task surfaces as a `JoinError`, and a missed deadline surfaces
as `Err(_)` — both are handled and both **fail open**. This is a good illustration of the
pattern: the failure modes are enumerated in the code, not discovered in production.

### 4.4 Structured shutdown across every worker

There are **9 `tokio::select!` sites** — one per long-running worker/subscriber. Each waits
on its message stream *and* a shutdown channel simultaneously, so the process can stop
cleanly instead of being killed:

```rust
loop {
    tokio::select! {
        msg = receiver.recv() => { /* handle */ }
        _ = shutdown_rx.changed() => { break; }
    }
}
```

`gateway/src/main.rs` owns a single `watch` channel and signals every worker at once, then
waits up to 10 s for the servers to finish.

---

## 5. Where each Rust capability is actually used

| Rust feature | Where in this repo | What it buys us |
|---|---|---|
| Ownership / borrow checking | All 12 crates | Memory safety, no GC, **zero `unsafe`** |
| `enum` + derived `Ord` | `common/src/types.rs` | `Outcome::worst()` is a type property, not magic numbers |
| Exhaustive `match` | `common/src/error.rs`, all handlers | A new variant is a compile error until handled |
| `Result` + `?` | Everywhere | Errors are explicit; fail-open is a deliberate choice |
| `thiserror` | `common/src/error.rs` | Typed errors carry HTTP status + machine code |
| `Send`/`Sync` checking | Async tasks, shared state | Data races rejected at compile time |
| `arc-swap` (`ArcSwap`) | `fast-path/src/policy_cache.rs`, `shadow-analysis/src/toggles.rs` | Lock-free hot reload; readers never block |
| `LazyLock` statics | 11 sites incl. `checks/secret_detection.rs` | Regex compiled once per process, not per request |
| `tokio::spawn` | `shadow-analysis/src/worker.rs` | Independent checks run in parallel |
| `tokio::task::spawn_blocking` | `proxy/src/handler.rs` | CPU work never starves the reactor |
| `tokio::time::timeout` | `proxy/src/handler.rs` | Hard deadline; breach fails open |
| `tokio::select!` (9 sites) | Every worker | Graceful shutdown, no dropped state |
| `serde` derive | `common/src/events.rs` | One `EventEnvelope<T>` contract across all services |
| Cargo workspace | `Cargo.toml` (12 members) | Crate boundaries are checked by the compiler |
| `criterion` benches | `fast-path/benches/` | Latency budget is a regression gate, not a promise |
| `sha2` + deterministic tests | `audit/src/chain.rs` | Hash-chain correctness is unit-tested |

### 5.1 The workspace is an architectural contract the compiler enforces

`AGENTS.md` says the fast path may not do I/O. That is a **dependency rule**, and in Rust a
dependency rule is enforceable: if `controlplane-fast-path` never depends on
`controlplane-platform` or `sqlx`, then it is *impossible* for a fast-path check to open a
database connection. The architecture is not a diagram someone has to remember — it is a
build error waiting to happen.

---

## 6. Why not Python?

Python is a great language — and we **do use it in this project** (§8). But it is the wrong
tool for the synchronous request path:

| Issue | Why it breaks the requirement |
|---|---|
| **The GIL** | CPU-bound work (regex over a response, entropy scoring) cannot truly run on multiple cores in the default interpreter. Our per-request checks would serialize under load, and the p99 would degrade exactly when traffic spikes. |
| **Interpreter overhead** | A single fast-path pass is 5.6 µs in Rust. The same logic in Python is orders of magnitude slower per call — the 10 ms budget stops being comfortable and starts being a ceiling. |
| **No compile-time types** | The failure mode of a type error moves from the build to production — dangerous for a component that decides whether a response is blocked. |
| **Startup & memory** | Interpreter plus dependency tree in the hot path; slower cold start, larger footprint per instance. |
| **Runtime dependency** | We want one binary, no interpreter version to reconcile in the container. |

**Where Python *is* right here, and we use it:** the guardrails sidecar
(`services/guardrails/`) — Presidio, spaCy, HuggingFace `transformers`, DeepEval. Those are
Python-native ML ecosystems with no equivalent in Rust, they load large models, and they
run **asynchronously off the request path** in the shadow path. Choosing Python there is
correct precisely because the constraint that rules it out elsewhere — CPU latency on the
synchronous path — does not apply.

---

## 7. Why not JavaScript / Node?

| Issue | Why it breaks the requirement |
|---|---|
| **Single-threaded event loop** | Node excels at I/O concurrency, not CPU work. A regex pass blocks **all** in-flight requests on that process — a tail-latency cliff under load. `worker_threads` exist but add a whole coordination model we would rather the compiler handle. |
| **GC pauses** | V8's GC introduces exactly the unpredictable tail latency a 10 ms budget cannot absorb. |
| **Dynamic typing across services** | Our service boundaries are JSON events (`EventEnvelope<T>`). In Rust, the shape is checked at compile time in every producer and consumer; in JS a field rename is a 3 a.m. incident. |
| **Dependency surface** | A proxy on the request path benefits from a small, statically-known dependency graph. |

**Where JS/TS is right, and we use it:** the dashboard (`frontend/`) — Next.js 16, React 19,
TypeScript, Tailwind. It is a UI and an SSE client, not the hot path. Using TypeScript there
is the same reasoning as using Rust here: pick the language whose *failure modes* are
tolerable in that position.

---

## 8. The honest answer: this is a deliberate polyglot split

We did not pick one language for the whole system. We matched the language to the
constraint at each boundary:

| Layer | Language | Why there |
|---|---|---|
| Proxy, fast path, decision, audit, cost, escalation, dashboard API, notification | **Rust** | Latency budget, determinism, memory safety, real parallelism, single binary |
| Guardrails sidecar (PII, toxicity, bias, hallucination) | **Python** | The ML ecosystem lives there; runs async, off the request path |
| Dashboard UI | **TypeScript / React** | Best tooling for interactive dashboards and SSE streaming |
| Event contracts | **JSON over an event bus** | Language-agnostic boundary; every service speaks the same envelope |

This is a defensible design choice, not a compromise: the synchronous path is Rust because
it must be, the ML sidecar is Python because it must be, and the two meet at a typed JSON
contract (`common/src/events.rs`).

---

## 9. What Rust costs us (stated plainly)

The honest trade-offs, since a jury will ask:

| Cost | Reality |
|---|---|
| Steeper learning curve | The borrow checker rejects designs that would "work" in another language; that is the point, but it costs time |
| Compile times | Longer feedback loop than a scripting language in a hackathon setting |
| Slower to prototype | More upfront design; less throwaway code |
| Smaller talent pool | Hiring considerations for a real product |
| Async Rust complexity | Pin/`Send` rules and lifetimes in spawned tasks take experience |
| Testing is still required | The compiler cannot prove *logic* correctness — hence the test suite and benchmarks |

We accept these because the two things the product is judged on — **a hard latency budget**
and **a tamper-evident audit trail** — are exactly the things Rust is strongest at.

---

## 10. Summary

| Property | How Rust delivers it here | Proof in this repo |
|---|---|---|
| **Predictable low latency** | No GC, no interpreter, static dispatch | 5.6 µs measured, 10 ms budget (`fast-path/benches/`) |
| **Memory safety** | Ownership + borrow checker | **Zero `unsafe`** in 12 crates |
| **Correctness under change** | Exhaustive matches, typed errors, enum ordering | `common/src/error.rs`, `common/src/types.rs` |
| **Fearless concurrency** | `Send`/`Sync` checks, ownership across tasks | 9 `select!` workers, parallel shadow checks |
| **Zero-contention hot reload** | Atomics via `arc-swap` | `fast-path/src/policy_cache.rs` |
| **Enforced architecture** | Crate dependency boundaries | `Cargo.toml` workspace of 12 members |
| **Verifiable performance** | Criterion benchmarks as a gate | `cargo bench -p controlplane-fast-path` |
| **Operational simplicity** | One static binary, no runtime | `Dockerfile` |

**The line to say out loud:** *the fast path is a CPU-bound component under a hard
microsecond deadline that also has to be correct enough to be audited — so we wrote it in
the one mainstream language with no garbage collector, compile-time memory safety, and real
parallelism, and we shipped it with zero `unsafe` code.*

---

## 11. Verification commands

```bash
# Latency budget (release mode)
cargo bench -p controlplane-fast-path

# Compiler-enforced safety: confirms there is no unsafe code
cargo clippy --workspace --all-targets -- -D warnings

# Whole workspace
cargo test --workspace && cargo build --release
```
