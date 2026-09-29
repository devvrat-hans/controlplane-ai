#!/usr/bin/env python3
"""FinanceBench / MMLU x ControlPlane.ai benchmark harness.

Asks benchmark questions through the ControlPlane proxy (so every question is
governed by the fast-path/shadow-path pipeline), then reports QA metrics and
governance metrics side by side.

Datasets (--dataset):
  financebench  https://huggingface.co/datasets/PatronusAI/financebench
                150 financial-filing QA questions with a gold answer and gold
                "evidence" passages. Default dataset.
  mmlu          https://huggingface.co/datasets/cais/mmlu (config "all", split "test")
                14,042 four-option multiple-choice questions across 57 subjects.
                Scored by the answer letter (A-D) the model gives.

Usage:
  python3 scripts/benchmark_financebench.py                               # 50 FinanceBench questions
  python3 scripts/benchmark_financebench.py --limit 0                     # all 150 questions
  python3 scripts/benchmark_financebench.py --limit 25 --context none
  python3 scripts/benchmark_financebench.py --dataset mmlu                # 1000 random MMLU questions
  python3 scripts/benchmark_financebench.py --dataset mmlu --limit 200 --json-out mmlu.json

Requires only the Python standard library (no `pip install`).
"""

from __future__ import annotations

import argparse
import json
import random
import re
import statistics
import sys
import tempfile
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

DATASET_ID = "PatronusAI/financebench"
JSONL_URL = f"https://huggingface.co/datasets/{DATASET_ID}/resolve/main/financebench_merged.jsonl"
ROWS_API = "https://datasets-server.huggingface.co/rows"
DEFAULT_APP_ID = "10000000-0000-0000-0000-000000000001"  # ChatBot-Prod (cost cap 75 tokens)
DEFAULT_MODEL = "qwen2.5:1.5b"

MMLU_DATASET_ID = "cais/mmlu"
MMLU_CONFIG = "all"
MMLU_SPLIT = "test"
LETTERS = "ABCD"

# The four top-level categories from the MMLU paper (Hendrycks et al., categories.py):
# STEM, humanities, social sciences, and other (business, health, misc.).
MMLU_CATEGORIES = {
    "STEM": [
        "abstract_algebra", "astronomy", "college_biology", "college_chemistry",
        "college_computer_science", "college_mathematics", "college_physics",
        "computer_security", "conceptual_physics", "electrical_engineering",
        "elementary_mathematics", "high_school_biology", "high_school_chemistry",
        "high_school_computer_science", "high_school_mathematics", "high_school_physics",
        "high_school_statistics", "machine_learning",
    ],
    "Humanities": [
        "formal_logic", "high_school_european_history", "high_school_us_history",
        "high_school_world_history", "international_law", "jurisprudence",
        "logical_fallacies", "moral_disputes", "moral_scenarios", "philosophy",
        "prehistory", "professional_law", "world_religions",
    ],
    "Social sciences": [
        "econometrics", "high_school_geography", "high_school_government_and_politics",
        "high_school_macroeconomics", "high_school_microeconomics", "high_school_psychology",
        "human_sexuality", "professional_psychology", "public_relations",
        "security_studies", "sociology", "us_foreign_policy",
    ],
    "Other": [
        "anatomy", "business_ethics", "clinical_knowledge", "college_medicine",
        "global_facts", "human_aging", "management", "marketing", "medical_genetics",
        "miscellaneous", "nutrition", "professional_accounting", "professional_medicine",
        "virology",
    ],
}
SUBJECT_TO_CATEGORY = {s: c for c, subjects in MMLU_CATEGORIES.items() for s in subjects}

# Refusal / abstention patterns. Reported separately because "the model could not
# answer" is a different failure mode from "the model answered incorrectly".
REFUSAL_PATTERNS = [
    r"\bi (?:don't|do not|cannot|can't|am unable to) (?:know|determine|answer|find|say)",
    r"\bnot enough information\b",
    r"\binsufficient (?:information|context|data)\b",
    r"\b(?:cannot|can't|unable to) be determined\b",
    r"\bno information (?:is )?(?:available|provided)\b",
    r"\bthe (?:context|passage|document) does not (?:mention|contain|provide|specify)\b",
]

MAGNITUDES = {
    "hundred": 1e2,
    "thousand": 1e3,
    "million": 1e6,
    "billion": 1e9,
    "trillion": 1e12,
}

NUMBER_RE = re.compile(
    r"(?P<sign>[-+(])?\s*\$?\s*(?P<num>\d[\d,]*(?:\.\d+)?)\s*(?P<magnitude>hundred|thousand|million|billion|trillion)?\s*(?P<pct>%)?",
    re.IGNORECASE,
)

# A gold answer counts as numeric only when the answer *is* a number, not merely a
# sentence containing one ("Pepsico raised guidance by 1 percentage point" is a textual
# answer, and scoring it numerically would inflate the numeric-match rate).
NUMERIC_ANSWER_RE = re.compile(
    r"^[-(]?\s*\$?\s*-?\d[\d,]*(?:\.\d+)?\s*"
    r"(?:%|hundred|thousand|million|billion|trillion)?\s*\)?$",
    re.IGNORECASE,
)


# ─────────────────────────────────────────────────────────────────────────────
# Normalization + scoring
# ─────────────────────────────────────────────────────────────────────────────


def normalize(text: str) -> str:
    """Lowercase, drop currency/percent symbols and thousands separators, collapse
    whitespace, and strip trailing punctuation. Used by every lexical metric."""
    if text is None:
        return ""
    text = text.replace("−", "-").replace("–", "-").replace("—", "-")
    text = text.replace(" ", " ")
    text = text.lower()
    text = text.replace("$", "").replace("€", "").replace("£", "").replace("%", "")
    text = re.sub(r"(?<=\d),(?=\d)", "", text)  # 1,577 -> 1577
    text = re.sub(r"\s+", " ", text)
    return text.strip().strip(".,;:").strip()


def is_numeric_answer(gold: str) -> bool:
    """True when the gold answer is a number (optionally with a currency symbol,
    magnitude word or percent sign) rather than prose that happens to contain one."""
    text = re.sub(r"\s+", " ", (gold or "").strip().lower()).rstrip(".!").strip()
    return bool(text) and NUMERIC_ANSWER_RE.match(text) is not None


def parse_number(text: str) -> float | None:
    """First number in `text`, applying magnitude words ("1.2 billion" -> 1.2e9)
    and parentheses signs ("(1577)" -> -1577). Returns None if there is no number."""
    if not text:
        return None
    match = NUMBER_RE.search(text)
    if not match:
        return None
    try:
        value = float(match.group("num").replace(",", ""))
    except ValueError:
        return None
    magnitude = match.group("magnitude")
    if magnitude:
        value *= MAGNITUDES.get(magnitude.lower(), 1.0)
    if match.group("sign") == "(":
        value = -value
    elif match.group("sign") == "-":
        value = -value
    return value


def tokenize(text: str) -> list[str]:
    return [t for t in re.split(r"\W+", normalize(text)) if t]


def token_f1(gold: str, predicted: str) -> float:
    """SQuAD-style token-level F1 between the gold answer and the predicted answer."""
    gold_tokens, pred_tokens = tokenize(gold), tokenize(predicted)
    if not gold_tokens or not pred_tokens:
        return 0.0
    gold_counts: dict[str, int] = {}
    for token in gold_tokens:
        gold_counts[token] = gold_counts.get(token, 0) + 1
    overlap = 0
    for token in pred_tokens:
        if gold_counts.get(token, 0) > 0:
            gold_counts[token] -= 1
            overlap += 1
    if overlap == 0:
        return 0.0
    precision = overlap / len(pred_tokens)
    recall = overlap / len(gold_tokens)
    return 2 * precision * recall / (precision + recall)


def answer_span(text: str, max_chars: int = 120) -> str:
    """The model's answer as a short span: first non-empty line, else first sentence."""
    if not text:
        return ""
    for line in text.splitlines():
        line = line.strip()
        if line:
            if len(line) <= max_chars:
                return line
            break
    sentence = re.split(r"(?<=[.!?])\s+", text.strip(), maxsplit=1)[0]
    return sentence[:max_chars].strip()


def is_refusal(text: str) -> bool:
    lowered = (text or "").lower()
    return any(re.search(pattern, lowered) for pattern in REFUSAL_PATTERNS)


def numbers_match(gold: str, response: str, tolerance: float) -> bool:
    """True when the gold number is present in the response within `tolerance`
    (relative for values >= 1, absolute otherwise, so partial credit follows the
    magnitude of the figure being asked about)."""
    gold_value = parse_number(gold)
    if gold_value is None:
        return False
    for match in NUMBER_RE.finditer(response or ""):
        candidate = parse_number(match.group(0))
        if candidate is None:
            continue
        if gold_value == 0:
            if abs(candidate) < tolerance:
                return True
        elif abs(candidate - gold_value) <= abs(gold_value) * tolerance:
            return True
    return False


def score(question: dict, response_text: str, response_error: str | None) -> dict:
    """All deterministic QA signals for one FinanceBench question."""
    gold = str(question.get("answer", "") or "")
    span = answer_span(response_text)
    gold_norm, span_norm, resp_norm = normalize(gold), normalize(span), normalize(response_text)
    numeric_gold = is_numeric_answer(gold)
    return {
        "gold": gold,
        "gold_is_numeric": numeric_gold,
        "predicted_span": span,
        "exact_match": bool(gold_norm) and gold_norm == span_norm,
        "contains": bool(gold_norm) and gold_norm in resp_norm,
        "numeric_match": numeric_gold and numbers_match(gold, response_text, 1e-3),
        "token_f1": token_f1(gold, span),
        "refusal": is_refusal(response_text),
        "error": response_error,
        "empty_response": not normalize(response_text),
    }


# ── MMLU: extract the chosen option letter ──────────────────────────────────

# Letter at the very start: "B", "B.", "(B)", "B) 42", "**B**", "Answer: B".
LEADING_LETTER_RE = re.compile(r"^\W*(?:answer\s*(?:is)?\s*[:\-]?\s*)?\(?\s*([ABCD])\s*(?:\)|\.|:|,|\*|$|\s)", re.IGNORECASE)
# "The answer is C", "correct option: (D)", "choice B".
PHRASED_LETTER_RE = re.compile(r"\b(?:answer|option|choice)\s*(?:is|would be|:)?\s*\(?\s*([ABCD])\b", re.IGNORECASE)


def extract_choice(text: str, choices: list[str]) -> tuple[int | None, str]:
    """Index (0-3) of the option the model chose, and how it was found.

    Tries, in order: a leading option letter, a phrased letter ("the answer is C"),
    then an exact match of exactly one option's text. Anything else is `None`
    (an invalid answer, scored as wrong)."""
    stripped = (text or "").strip()
    if not stripped:
        return None, "empty"
    match = LEADING_LETTER_RE.match(stripped)
    if match:
        return LETTERS.index(match.group(1).upper()), "leading letter"
    match = PHRASED_LETTER_RE.search(stripped)
    if match:
        return LETTERS.index(match.group(1).upper()), "phrased letter"
    response_norm = normalize(stripped)
    hits = [i for i, choice in enumerate(choices) if normalize(str(choice)) and normalize(str(choice)) == response_norm]
    if not hits:
        hits = [i for i, choice in enumerate(choices)
                if len(normalize(str(choice))) >= 3 and normalize(str(choice)) in response_norm]
    if len(hits) == 1:
        return hits[0], "option text"
    return None, "unparseable"


def score_mmlu(question: dict, response_text: str, response_error: str | None) -> dict:
    """Multiple-choice scoring: correct when the extracted option equals the gold index."""
    choices = list(question.get("choices") or [])
    gold_index = int(question.get("answer"))
    predicted, method = extract_choice(response_text, choices)
    return {
        "gold_index": gold_index,
        "gold_letter": LETTERS[gold_index],
        "gold": choices[gold_index] if gold_index < len(choices) else "",
        "predicted_index": predicted,
        "predicted_letter": LETTERS[predicted] if predicted is not None else None,
        "extraction": method,
        "correct": predicted is not None and predicted == gold_index,
        "invalid": predicted is None,
        "refusal": is_refusal(response_text),
        "error": response_error,
        "empty_response": not normalize(response_text),
    }


# ─────────────────────────────────────────────────────────────────────────────
# HTTP helpers
# ─────────────────────────────────────────────────────────────────────────────


def http_json(url: str, payload: dict | None = None, timeout: int = 60, headers: dict | None = None):
    """Returns (status, parsed_json_or_text, response_headers). Raises on transport failure.

    Response header keys are lower-cased: hyper (the proxy) emits lower-case names, and a
    case-sensitive lookup would silently miss X-ControlPlane-* headers.
    """
    body = json.dumps(payload).encode() if payload is not None else None
    request = urllib.request.Request(url, data=body, method="POST" if body else "GET")
    request.add_header("Content-Type", "application/json")
    for key, value in (headers or {}).items():
        request.add_header(key, value)
    with urllib.request.urlopen(request, timeout=timeout) as response:
        raw = response.read().decode("utf-8", "replace")
        try:
            parsed = json.loads(raw)
        except json.JSONDecodeError:
            parsed = raw
        return response.status, parsed, {k.lower(): v for k, v in response.headers.items()}


def load_dataset(limit_needed: int, cache_dir: Path) -> tuple[list[dict], str]:
    """Load the FinanceBench merged JSONL, caching it locally. Falls back to the
    HuggingFace datasets-server rows API if the direct file is unavailable."""
    cache_dir.mkdir(parents=True, exist_ok=True)
    cached = cache_dir / "financebench_merged.jsonl"
    rows: list[dict] = []

    if cached.exists():
        try:
            with cached.open(encoding="utf-8") as handle:
                rows = [json.loads(line) for line in handle if line.strip()]
        except (OSError, json.JSONDecodeError):
            rows = []
        if rows:
            return rows, "cache"

    try:
        with urllib.request.urlopen(JSONL_URL, timeout=120) as response:
            raw = response.read().decode("utf-8", "replace")
        cached.write_text(raw, encoding="utf-8")
        rows = [json.loads(line) for line in raw.splitlines() if line.strip()]
        if rows:
            return rows, "huggingface.co (merged jsonl)"
    except (urllib.error.URLError, OSError, json.JSONDecodeError) as error:
        print(f"[WARN] Direct dataset download failed ({error}); trying datasets-server API", flush=True)

    offset = 0
    while offset < max(limit_needed, 100):
        url = f"{ROWS_API}?dataset={DATASET_ID}&config=default&split=train&offset={offset}&length=100"
        try:
            _, payload, _ = http_json(url, timeout=120)
        except (urllib.error.URLError, OSError) as error:
            raise SystemExit(f"[FAIL] Could not fetch FinanceBench: {error}") from error
        batch = payload.get("rows", [])
        if not batch:
            break
        rows.extend(entry["row"] for entry in batch)
        offset += 100
        if len(rows) >= payload.get("num_rows_total", 0):
            break
    if not rows:
        raise SystemExit("[FAIL] FinanceBench returned no rows")
    return rows, "datasets-server API"


def load_mmlu(cache_dir: Path) -> tuple[list[dict], str]:
    """Load the full MMLU test split (14,042 rows) via the datasets-server rows API,
    100 rows per page, caching it as JSONL so later runs are offline and identical."""
    cache_dir.mkdir(parents=True, exist_ok=True)
    cached = cache_dir / f"mmlu_{MMLU_CONFIG}_{MMLU_SPLIT}.jsonl"
    if cached.exists():
        try:
            with cached.open(encoding="utf-8") as handle:
                rows = [json.loads(line) for line in handle if line.strip()]
            if rows:
                return rows, "cache"
        except (OSError, json.JSONDecodeError):
            pass

    # The rows API rate-limits (HTTP 429) long downloads, so progress is appended to a
    # partial file as it goes: a rerun resumes where the last one stopped.
    partial = cache_dir / f"mmlu_{MMLU_CONFIG}_{MMLU_SPLIT}.jsonl.partial"
    rows: list[dict] = []
    if partial.exists():
        with partial.open(encoding="utf-8") as handle:
            rows = [json.loads(line) for line in handle if line.strip()]
        if rows:
            print(f"  resuming MMLU download at row {len(rows)}", flush=True)
    offset, total = len(rows), None
    attempts = 10
    while total is None or offset < total:
        url = (f"{ROWS_API}?dataset={MMLU_DATASET_ID}&config={MMLU_CONFIG}"
               f"&split={MMLU_SPLIT}&offset={offset}&length=100")
        for attempt in range(attempts):
            try:
                _, payload, _ = http_json(url, timeout=120)
                break
            except urllib.error.HTTPError as error:
                if error.code not in (429, 500, 502, 503, 504) or attempt == attempts - 1:
                    raise SystemExit(f"[FAIL] Could not fetch MMLU (offset {offset}): {error}; "
                                     f"rerun to resume from row {offset}") from error
                retry_after = error.headers.get("Retry-After") if error.headers else None
                wait = float(retry_after) if retry_after and retry_after.isdigit() else min(60, 5 * 2 ** attempt)
            except (urllib.error.URLError, OSError) as error:
                if attempt == attempts - 1:
                    raise SystemExit(f"[FAIL] Could not fetch MMLU (offset {offset}): {error}; "
                                     f"rerun to resume from row {offset}") from error
                wait = min(60, 5 * 2 ** attempt)
            print(f"  rate-limited/unavailable at row {offset}; retrying in {wait:.0f}s", flush=True)
            time.sleep(wait)
        total = payload.get("num_rows_total", 0)
        batch = [entry["row"] for entry in payload.get("rows", [])]
        if not batch:
            break
        for index, row in enumerate(batch):
            row["mmlu_index"] = offset + index
        rows.extend(batch)
        with partial.open("a", encoding="utf-8") as handle:
            for row in batch:
                handle.write(json.dumps(row) + "\n")
        offset += len(batch)
        if offset % 2000 < 100:
            print(f"  downloaded {offset}/{total} MMLU rows...", flush=True)
        time.sleep(0.5)  # stay under the rows API rate limit
    if not rows:
        raise SystemExit("[FAIL] MMLU returned no rows")
    with cached.open("w", encoding="utf-8") as handle:
        for row in rows:
            handle.write(json.dumps(row) + "\n")
    partial.unlink(missing_ok=True)
    return rows, "datasets-server API"


def evidence_text(question: dict, max_chars: int) -> str:
    """Join the gold evidence passages. Passing gold evidence is an ORACLE-CONTEXT
    setup: it measures extraction, not retrieval."""
    raw = question.get("evidence")
    if isinstance(raw, str):
        try:
            raw = json.loads(raw)
        except json.JSONDecodeError:
            raw = [{"evidence_text": raw}]
    if not isinstance(raw, list):
        return ""
    parts = []
    for item in raw:
        text = item.get("evidence_text", "") if isinstance(item, dict) else str(item)
        if text:
            parts.append(text.strip())
    return "\n".join(parts)[:max_chars]


def build_prompt(question: dict, args) -> str:
    context = evidence_text(question, args.context_chars) if args.context == "oracle" else ""
    context_block = f"CONTEXT FROM THE FILING:\n{context}\n\n" if context else ""
    return (
        f"{context_block}"
        f"QUESTION: {question.get('question', '').strip()}\n\n"
        "Respond with the value or short phrase that answers the question. "
        "Do not explain your reasoning."
    )


def build_mmlu_prompt(question: dict) -> str:
    """Zero-shot multiple-choice prompt in the standard MMLU layout."""
    subject = str(question.get("subject", "")).replace("_", " ")
    options = "\n".join(f"{LETTERS[i]}. {choice}" for i, choice in enumerate(question.get("choices") or []))
    return (
        f"The following is a multiple choice question about {subject}.\n\n"
        f"{str(question.get('question', '')).strip()}\n{options}\n\n"
        "Answer with only the letter (A, B, C or D) of the correct option."
    )


def call_proxy(prompt: str, session_id: str, args) -> dict:
    """One question through the ControlPlane proxy; returns the transport fields."""
    payload = {
        "model": args.model,
        "app_id": args.app_id,
        "session_id": session_id,
        "messages": [{"role": "user", "content": prompt}],
        "max_tokens": args.max_tokens,
    }
    started = time.monotonic()
    record = {
        "http_status": None,
        "error": None,
        "blocked_by_policy": False,
        "response_text": "",
        "correlation_id": None,
        "fast_path_ms": None,
        "prompt_tokens": None,
        "completion_tokens": None,
        "latency_ms": None,
    }
    try:
        status, body, headers = http_json(f"{args.proxy_url}/v1/messages", payload, timeout=args.timeout)
        record["http_status"] = status
        record["correlation_id"] = headers.get("x-controlplane-correlation-id")
        header_latency = headers.get("x-controlplane-latency-ms")
        if header_latency is not None:
            record["fast_path_ms"] = int(header_latency)
        if isinstance(body, dict):
            choices = body.get("choices") or []
            if choices:
                record["response_text"] = (choices[0].get("message") or {}).get("content", "") or ""
            usage = body.get("usage") or {}
            record["prompt_tokens"] = usage.get("prompt_tokens")
            record["completion_tokens"] = usage.get("completion_tokens")
    except urllib.error.HTTPError as error:
        # The proxy returns 403 with the policy reason and correlation_id in the body
        record["http_status"] = error.code
        record["blocked_by_policy"] = error.code == 403
        try:
            detail = json.loads(error.read().decode("utf-8", "replace"))
            record["error"] = detail.get("error", {}).get("message", f"HTTP {error.code}")
            record["correlation_id"] = detail.get("error", {}).get("correlation_id")
        except (json.JSONDecodeError, OSError, AttributeError):
            record["error"] = f"HTTP {error.code}"
    except (urllib.error.URLError, TimeoutError, OSError) as error:
        record["error"] = f"{type(error).__name__}: {error}"
    record["latency_ms"] = int((time.monotonic() - started) * 1000)
    return record


def ask_question(row: dict, args) -> dict:
    """One FinanceBench question through the ControlPlane proxy."""
    record = {
        "financebench_id": row.get("financebench_id"),
        "company": row.get("company"),
        "doc_name": row.get("doc_name"),
        "question_type": row.get("question_type"),
        "question_reasoning": row.get("question_reasoning"),
        "question": row.get("question"),
        "gold": row.get("answer"),
    }
    record.update(call_proxy(build_prompt(row, args), f"financebench-{row.get('financebench_id', 'unknown')}", args))
    record.update(score(row, record["response_text"], record["error"]))
    return record


def ask_mmlu_question(row: dict, args) -> dict:
    """One MMLU question through the ControlPlane proxy."""
    record = {
        "mmlu_index": row.get("mmlu_index"),
        "subject": row.get("subject"),
        "category": SUBJECT_TO_CATEGORY.get(row.get("subject"), "Other"),
        "question": row.get("question"),
        "choices": row.get("choices"),
    }
    record.update(call_proxy(build_mmlu_prompt(row), f"mmlu-{row.get('mmlu_index', 'unknown')}", args))
    record.update(score_mmlu(row, record["response_text"], record["error"]))
    return record


def fetch_verdicts(record: dict, args) -> None:
    """Join the governance outcome for a call via the dashboard API."""
    call_id = record.get("correlation_id")
    if not call_id or args.no_verdicts:
        return
    try:
        _, body, _ = http_json(f"{args.api_url}/api/v1/requests/{call_id}", timeout=30)
    except (urllib.error.URLError, urllib.error.HTTPError, OSError):
        record["verdicts"] = []
        return
    if not isinstance(body, dict):
        record["verdicts"] = []
        return
    verdicts = body.get("verdicts") or []
    record["verdicts"] = [
        {
            "axis": v.get("axis"),
            "path": v.get("path"),
            "outcome": v.get("outcome"),
            "confidence": v.get("confidence"),
            "check_name": v.get("check_name"),
            "reason": v.get("reason"),
        }
        for v in verdicts
    ]
    call = body.get("call") or {}
    if record.get("fast_path_ms") is None and call.get("fast_path_latency_ms") is not None:
        record["fast_path_ms"] = call["fast_path_latency_ms"]
    # Exact (microsecond) fast-path time, when the gateway records it.
    if call.get("fast_path_latency_us") is not None:
        record["fast_path_us"] = call["fast_path_latency_us"]
    if call.get("token_count_output") is not None:
        record["completion_tokens"] = call["token_count_output"]
    runs = call.get("shadow_check_runs") or []
    record["shadow_runs"] = {r.get("check_name"): r.get("status") for r in runs if isinstance(r, dict)}
    record["has_escalation"] = bool(body.get("escalation"))


# ─────────────────────────────────────────────────────────────────────────────
# Reporting
# ─────────────────────────────────────────────────────────────────────────────


def percentile(values: list[float], pct: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    index = int((len(ordered) - 1) * pct / 100)
    return ordered[index]


def ratio(numerator: int, denominator: int) -> str:
    if denominator <= 0:
        return "n/a"
    return f"{numerator / denominator * 100:.1f}% ({numerator}/{denominator})"


def print_latency_and_governance(records: list[dict]) -> dict:
    """Sections shared by every dataset: latency, governance overhead, verdicts, tokens."""
    latencies = [r["latency_ms"] for r in records if r["latency_ms"] is not None]
    print("--- 3. LATENCY (end-to-end, includes model inference + queue) ---")
    if latencies:
        print(f"  p50 / p95 / p99:                {percentile(latencies, 50)} / {percentile(latencies, 95)} / {percentile(latencies, 99)} ms")
        print(f"  Mean:                           {statistics.fmean(latencies):.0f} ms")
    fast_path_us = [r["fast_path_us"] for r in records if r.get("fast_path_us") is not None]
    fast_path = [r["fast_path_ms"] for r in records if r.get("fast_path_ms") is not None]
    if fast_path_us:
        ms = [us / 1000 for us in fast_path_us]
        print(f"  Governance overhead (fast-path) p50 / p95 / p99: "
              f"{percentile(ms, 50):.3f} / {percentile(ms, 95):.3f} / {percentile(ms, 99):.3f} ms  (microsecond timing)")
        print(f"    (contract: <10ms p50, <25ms p99)")
    elif fast_path:
        # Older gateways record whole milliseconds, so 0 means "below 1 ms", not "no work".
        resolution = " (<1 ms: recorded at 1 ms resolution, checks run in microseconds)" if max(fast_path) == 0 else ""
        print(f"  Governance overhead (fast-path) p50 / p95 / p99: {percentile(fast_path, 50)} / {percentile(fast_path, 95)} / {percentile(fast_path, 99)} ms{resolution}")
        print(f"    (contract: <10ms p50, <25ms p99)")
    else:
        print("  Governance overhead:            not reported by the proxy (see --no-verdicts)")
    print()

    verdict_counts: dict[tuple[str, str], int] = {}
    path_counts: dict[str, int] = {}
    for record in records:
        for verdict in record.get("verdicts", []):
            key = (verdict.get("axis") or "?", verdict.get("outcome") or "?")
            verdict_counts[key] = verdict_counts.get(key, 0) + 1
            path_counts[verdict.get("path") or "?"] = path_counts.get(verdict.get("path") or "?", 0) + 1
    escalations = sum(1 for r in records if r.get("has_escalation"))

    check_counts: dict[str, int] = {}
    for record in records:
        for verdict in record.get("verdicts", []):
            if verdict.get("outcome") != "pass" and verdict.get("check_name") != "fast-path-summary":
                name = verdict.get("check_name") or "?"
                check_counts[name] = check_counts.get(name, 0) + 1

    print("--- 4. GOVERNANCE OUTCOMES (verdicts recorded by the pipeline) ---")
    if verdict_counts:
        print("  By axis / outcome:")
        for (axis, outcome), count in sorted(verdict_counts.items()):
            print(f"    {axis:<16} {outcome:<10} {count}")
        print("  By path: " + ", ".join(f"{path}={count}" for path, count in sorted(path_counts.items())))
        if check_counts:
            print("  Non-pass verdicts by check:")
            for name, count in sorted(check_counts.items(), key=lambda kv: -kv[1]):
                print(f"    {name:<28} {count}")
        print(f"  Escalation cases created:       {escalations}")
        print("  Note: shadow-path verdicts are advisory and arrive after the response is")
        print("        delivered, so a shadow 'block' does not appear as an HTTP 403 above.")
    else:
        print("  No verdicts retrieved (is the dashboard API up? use --no-verdicts to silence)")

    prompt_tokens = [r["prompt_tokens"] for r in records if r.get("prompt_tokens")]
    completion_tokens = [r["completion_tokens"] for r in records if r.get("completion_tokens")]
    if prompt_tokens or completion_tokens:
        print(f"  Prompt tokens (total):          {sum(prompt_tokens)}")
        print(f"  Completion tokens (total):      {sum(completion_tokens)}")
        if completion_tokens:
            print(f"  Mean completion tokens/answer:  {statistics.fmean(completion_tokens):.1f}")
    print()

    ms_values = [us / 1000 for us in fast_path_us] if fast_path_us else fast_path
    return {
        "latencies": latencies,
        "fast_path_ms_values": ms_values,
        "verdict_counts": verdict_counts,
        "check_counts": check_counts,
        "escalations": escalations,
    }


def print_report(records: list[dict], args, dataset_size: int, sample_kind: str, source: str) -> dict:
    asked = len(records)
    blocked = [r for r in records if r["blocked_by_policy"]]
    failed = [r for r in records if r["error"] and not r["blocked_by_policy"]]
    delivered = [r for r in records if not r["error"] and not r["empty_response"]]
    scorable = [r for r in delivered if not r["refusal"]]
    numeric_gold = [r for r in scorable if r["gold_is_numeric"]]

    print()
    print("=" * 78)
    print("FINANCEBENCH x CONTROLPLANE.ai - BENCHMARK REPORT")
    print("=" * 78)
    print(f"Dataset:            {DATASET_ID} ({dataset_size} questions, source: {source})")
    print(f"Sample:             {asked} of {dataset_size} questions ({sample_kind})")
    print(f"Model:              {args.model} (via proxy {args.proxy_url})")
    print(f"Context mode:       {'ORACLE (gold evidence passages supplied)' if args.context == 'oracle' else 'CLOSED-BOOK (question only)'}")
    print(f"App / policy:       {args.app_id}")
    print(f"Max tokens:         {args.max_tokens}")
    print(f"Concurrency:        {args.concurrency}")
    print(f"Run at:             {time.strftime('%Y-%m-%d %H:%M:%S')}")
    print()

    print("--- 1. QUESTION COVERAGE ---")
    print(f"  Asked:                          {asked}")
    print(f"  Answered (non-empty):           {len(delivered)}   {ratio(len(delivered), asked)}")
    print(f"  Blocked by policy (HTTP 403):   {len(blocked)}   {ratio(len(blocked), asked)}")
    print(f"  Transport / upstream failures:  {len(failed)}   {ratio(len(failed), asked)}")
    print()

    print(f"--- 2. ANSWER QUALITY (over {len(scorable)} non-refusal answers) ---")
    if scorable:
        print(f"  Exact match (normalized):       {ratio(sum(r['exact_match'] for r in scorable), len(scorable))}")
        print(f"  Gold contained in response:     {ratio(sum(r['contains'] for r in scorable), len(scorable))}")
        print(f"  Numeric match (+/-0.1%):        {ratio(sum(r['numeric_match'] for r in numeric_gold), len(numeric_gold))}  [numeric-gold subset]")
        print(f"  Mean token F1:                  {statistics.fmean(r['token_f1'] for r in scorable):.3f}")
    else:
        print("  No scorable answers")
    print(f"  Refusals/abstentions:           {ratio(sum(r['refusal'] for r in delivered), len(delivered))}  [over delivered answers]")
    print()

    shared = print_latency_and_governance(records)
    latencies, fast_path = shared["latencies"], shared["fast_path_ms_values"]

    print("--- 5. METRIC DEFINITIONS (what the numbers above mean) ---")
    print("  Normalization:      lowercase; currency/percent symbols and thousands")
    print("                      separators removed; whitespace collapsed; trailing")
    print("                      punctuation stripped.")
    print("  Exact match:        normalized gold == normalized predicted answer, where the")
    print("                      predicted answer is the first non-empty line (<=120 chars).")
    print("  Gold contained:     normalized gold is a substring of the full response.")
    print("  Numeric match:      gold parses as a number and a number in the response is")
    print("                      within 0.1% of it, after applying magnitude words")
    print("                      (\"1.2 billion\" -> 1.2e9). Graded on numeric-gold rows only.")
    print("  Token F1:           SQuAD-style overlap between gold tokens and the predicted")
    print("                      answer span.")
    print("  Refusal:            response matches abstention phrasing (e.g. \"I don't know\",")
    print("                      \"not enough information\", \"the context does not mention\").")
    print("  Governance overhead: the pipeline's own latency for the call (fast-path checks),")
    print("                      excluding model inference.")
    print()

    print("--- 6. HOW TO STATE THESE NUMBERS HONESTLY ---")
    print("  * FinanceBench's official metric is GPT-4-as-judge answer correctness over the")
    print("    full 10-K PDFs. This harness reports deterministic lexical/numeric metrics")
    print("    instead, so its QA numbers are NOT comparable to published leaderboard scores.")
    if args.context == "oracle":
        print("  * Context mode is ORACLE: the dataset's gold evidence passage is pasted into")
        print("    the prompt. This measures extraction from a known passage, not retrieval.")
    else:
        print("  * Context mode is CLOSED-BOOK: no filing text is supplied, so the model can")
        print("    only answer from memorized knowledge. Expect low accuracy by construction.")
    print("  * Blocked answers (HTTP 403) are excluded from QA metrics and counted in")
    print("    section 1, because the pipeline deliberately withheld the model's answer.")
    print("  * End-to-end latency is dominated by CPU model inference (single inference slot,")
    print("    OLLAMA_NUM_PARALLEL=1), not by the governance layer.")
    print("=" * 78)
    print()

    return {
        "dataset": DATASET_ID,
        "asked": asked,
        "answered": len(delivered),
        "blocked": len(blocked),
        "failed": len(failed),
        "scorable": len(scorable),
        "exact_match_rate": (sum(r["exact_match"] for r in scorable) / len(scorable)) if scorable else None,
        "contains_rate": (sum(r["contains"] for r in scorable) / len(scorable)) if scorable else None,
        "numeric_match_rate": (sum(r["numeric_match"] for r in numeric_gold) / len(numeric_gold)) if numeric_gold else None,
        "mean_token_f1": statistics.fmean(r["token_f1"] for r in scorable) if scorable else None,
        "refusal_rate": (sum(r["refusal"] for r in delivered) / len(delivered)) if delivered else None,
        "latency_p50_ms": percentile(latencies, 50) if latencies else None,
        "latency_p95_ms": percentile(latencies, 95) if latencies else None,
        "latency_p99_ms": percentile(latencies, 99) if latencies else None,
        "fast_path_p50_ms": percentile(fast_path, 50) if fast_path else None,
        "fast_path_p99_ms": percentile(fast_path, 99) if fast_path else None,
        "escalations": shared["escalations"],
        "verdicts_by_axis_outcome": {f"{a}/{o}": c for (a, o), c in shared["verdict_counts"].items()},
        "context_mode": args.context,
        "sample_of": dataset_size,
    }


def print_mmlu_report(records: list[dict], args, dataset_size: int, sample_kind: str, source: str) -> dict:
    asked = len(records)
    blocked = [r for r in records if r["blocked_by_policy"]]
    failed = [r for r in records if r["error"] and not r["blocked_by_policy"]]
    delivered = [r for r in records if not r["error"] and not r["empty_response"]]
    valid = [r for r in delivered if not r["invalid"]]
    correct = [r for r in records if r["correct"]]

    def acc(rows: list[dict]) -> float | None:
        return (sum(r["correct"] for r in rows) / len(rows)) if rows else None

    print()
    print("=" * 78)
    print("MMLU x CONTROLPLANE.ai - BENCHMARK REPORT")
    print("=" * 78)
    print(f"Dataset:            {MMLU_DATASET_ID} (config '{MMLU_CONFIG}', split '{MMLU_SPLIT}', "
          f"{dataset_size} questions, source: {source})")
    print(f"Sample:             {asked} of {dataset_size} questions ({sample_kind})")
    print(f"Model:              {args.model} (via proxy {args.proxy_url})")
    print(f"Prompting:          ZERO-SHOT, 4 lettered options, 'answer with only the letter'")
    print(f"App / policy:       {args.app_id}")
    print(f"Max tokens:         {args.max_tokens}")
    print(f"Concurrency:        {args.concurrency}")
    print(f"Run at:             {time.strftime('%Y-%m-%d %H:%M:%S')}")
    print()

    print("--- 1. QUESTION COVERAGE ---")
    print(f"  Asked:                          {asked}")
    print(f"  Answered (non-empty):           {len(delivered)}   {ratio(len(delivered), asked)}")
    print(f"  Valid option extracted:         {len(valid)}   {ratio(len(valid), asked)}")
    print(f"  Blocked by policy (HTTP 403):   {len(blocked)}   {ratio(len(blocked), asked)}")
    print(f"  Transport / upstream failures:  {len(failed)}   {ratio(len(failed), asked)}")
    print()

    print("--- 2. ANSWER QUALITY ---")
    print(f"  Accuracy (all asked):           {ratio(len(correct), asked)}   <- headline; blocked/failed/invalid = wrong")
    print(f"  Accuracy (valid answers only):  {ratio(sum(r['correct'] for r in valid), len(valid))}")
    print(f"  Random-guess baseline:          25.0%")
    print(f"  Invalid / unparseable answers:  {ratio(sum(r['invalid'] for r in delivered), len(delivered))}  [over delivered answers]")
    methods: dict[str, int] = {}
    for r in delivered:
        methods[r["extraction"]] = methods.get(r["extraction"], 0) + 1
    print("  Answer extraction:              " + ", ".join(f"{m}={c}" for m, c in sorted(methods.items(), key=lambda kv: -kv[1])))
    predicted_dist = {letter: 0 for letter in LETTERS}
    gold_dist = {letter: 0 for letter in LETTERS}
    for r in records:
        gold_dist[r["gold_letter"]] += 1
        if r["predicted_letter"]:
            predicted_dist[r["predicted_letter"]] += 1
    print("  Predicted letter distribution:  " + "  ".join(f"{k}={v}" for k, v in predicted_dist.items()))
    print("  Gold letter distribution:       " + "  ".join(f"{k}={v}" for k, v in gold_dist.items()))
    print()

    by_category: dict[str, list[dict]] = {}
    by_subject: dict[str, list[dict]] = {}
    for r in records:
        by_category.setdefault(r["category"], []).append(r)
        by_subject.setdefault(r["subject"], []).append(r)
    print("  By category (MMLU paper grouping):")
    for category in list(MMLU_CATEGORIES):
        rows = by_category.get(category, [])
        print(f"    {category:<16} {ratio(sum(r['correct'] for r in rows), len(rows))}")
    subject_acc = {s: acc(rows) for s, rows in by_subject.items()}
    macro = statistics.fmean(v for v in subject_acc.values() if v is not None) if subject_acc else None
    print(f"  Macro-average over {len(subject_acc)} subjects: {macro * 100:.1f}%" if macro is not None else "")
    enough = {s: rows for s, rows in by_subject.items() if len(rows) >= args.min_subject_n}
    if enough:
        ranked = sorted(enough, key=lambda s: (subject_acc[s], -len(enough[s])))
        print(f"  Weakest subjects (n >= {args.min_subject_n}):")
        for s in ranked[:5]:
            print(f"    {s:<40} {ratio(sum(r['correct'] for r in enough[s]), len(enough[s]))}")
        print(f"  Strongest subjects (n >= {args.min_subject_n}):")
        for s in list(reversed(ranked))[:5]:
            print(f"    {s:<40} {ratio(sum(r['correct'] for r in enough[s]), len(enough[s]))}")
    print()

    shared = print_latency_and_governance(records)
    latencies, fast_path = shared["latencies"], shared["fast_path_ms_values"]

    print("--- 5. METRIC DEFINITIONS (what the numbers above mean) ---")
    print("  Answer extraction:  the chosen option is read from the response in this order:")
    print("                      a leading letter (\"B\", \"(B)\", \"B. ...\", \"Answer: B\"), a")
    print("                      phrased letter (\"the answer is C\"), or the text of exactly one")
    print("                      option. Anything else is INVALID and scored as wrong.")
    print("  Accuracy (all asked): correct / asked. Blocked, failed and invalid answers count")
    print("                      as wrong, so the pipeline's own interventions are visible.")
    print("  Accuracy (valid):   correct / answers with a valid extracted option.")
    print("  Category:           STEM / Humanities / Social sciences / Other, as grouped in")
    print("                      the MMLU paper (health subjects fall under Other).")
    print("  Macro-average:      mean of per-subject accuracies (each subject weighted equally).")
    print("  Governance overhead: the pipeline's own latency for the call (fast-path checks),")
    print("                      excluding model inference.")
    print()

    print("--- 6. HOW TO STATE THESE NUMBERS HONESTLY ---")
    print("  * Published MMLU scores are usually 5-shot (5 worked examples in the prompt) and")
    print("    compare option log-likelihoods. This harness is ZERO-SHOT and scores the letter")
    print("    the model writes, through a chat endpoint, so its accuracy is NOT directly")
    print("    comparable to leaderboard numbers (typically lower for small models).")
    print(f"  * The sample is {asked} of {dataset_size} questions ({sample_kind}); per-subject")
    print(f"    numbers rest on ~{asked // max(1, len(by_subject))} questions each and are noisy.")
    print("  * Blocked answers (HTTP 403) are counted as wrong in the headline accuracy and")
    print("    shown in section 1, because the pipeline deliberately withheld the answer.")
    print("  * End-to-end latency is dominated by CPU model inference (single inference slot,")
    print("    OLLAMA_NUM_PARALLEL=1), not by the governance layer.")
    print("=" * 78)
    print()

    return {
        "dataset": MMLU_DATASET_ID,
        "asked": asked,
        "answered": len(delivered),
        "valid": len(valid),
        "blocked": len(blocked),
        "failed": len(failed),
        "correct": len(correct),
        "accuracy_all_asked": acc(records),
        "accuracy_valid": acc(valid),
        "invalid_rate": (sum(r["invalid"] for r in delivered) / len(delivered)) if delivered else None,
        "accuracy_by_category": {c: acc(rows) for c, rows in by_category.items()},
        "accuracy_by_subject": {s: {"accuracy": subject_acc[s], "n": len(by_subject[s])} for s in by_subject},
        "macro_subject_accuracy": macro,
        "predicted_letter_distribution": predicted_dist,
        "latency_p50_ms": percentile(latencies, 50) if latencies else None,
        "latency_p95_ms": percentile(latencies, 95) if latencies else None,
        "latency_p99_ms": percentile(latencies, 99) if latencies else None,
        "fast_path_p50_ms": percentile(fast_path, 50) if fast_path else None,
        "fast_path_p99_ms": percentile(fast_path, 99) if fast_path else None,
        "escalations": shared["escalations"],
        "verdicts_by_axis_outcome": {f"{a}/{o}": c for (a, o), c in shared["verdict_counts"].items()},
        "non_pass_verdicts_by_check": shared["check_counts"],
        "sample_of": dataset_size,
    }


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Ask FinanceBench or MMLU questions through the ControlPlane proxy and report metrics.",
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    parser.add_argument("--dataset", choices=["financebench", "mmlu"], default="financebench")
    parser.add_argument("--limit", type=int, default=None,
                        help="questions to ask (0 = all; default 50 for financebench, 1000 for mmlu)")
    parser.add_argument("--sample", choices=["random", "head"], default="random", help="how to pick the sample")
    parser.add_argument("--seed", type=int, default=42, help="seed for --sample random")
    parser.add_argument("--context", choices=["oracle", "none"], default="oracle",
                        help="financebench only: oracle = include gold evidence passages; none = question only")
    parser.add_argument("--context-chars", type=int, default=1200, help="evidence characters included per question")
    parser.add_argument("--concurrency", type=int, default=4, help="questions asked in parallel")
    parser.add_argument("--max-tokens", type=int, default=None,
                        help="max_tokens per question (default 64 for financebench, 8 for mmlu)")
    parser.add_argument("--min-subject-n", type=int, default=10, help="mmlu: min questions for a subject to be ranked")
    parser.add_argument("--model", default=DEFAULT_MODEL)
    parser.add_argument("--app-id", default=DEFAULT_APP_ID)
    parser.add_argument("--proxy-url", default="http://localhost:8900")
    parser.add_argument("--api-url", default="http://localhost:8080", help="dashboard API for governance verdicts")
    parser.add_argument("--timeout", type=int, default=240, help="per-question timeout in seconds")
    parser.add_argument("--cache-dir", default=str(Path(tempfile.gettempdir()) / "controlplane-financebench"))
    parser.add_argument("--json-out", default=None, help="write per-question results to this path")
    parser.add_argument("--no-verdicts", action="store_true", help="skip the governance verdict join")
    parser.add_argument("--quiet", action="store_true", help="hide per-question progress lines")
    args = parser.parse_args()

    is_mmlu = args.dataset == "mmlu"
    if args.limit is None:
        args.limit = 1000 if is_mmlu else 50
    if args.max_tokens is None:
        args.max_tokens = 8 if is_mmlu else 64

    if is_mmlu:
        rows, source = load_mmlu(Path(args.cache_dir))
        label, ask, report = "MMLU", ask_mmlu_question, print_mmlu_report
    else:
        rows, source = load_dataset(args.limit, Path(args.cache_dir))
        label, ask, report = "FinanceBench", ask_question, print_report
    dataset_size = len(rows)
    if args.limit and args.limit < dataset_size:
        if args.sample == "random":
            rows = random.Random(args.seed).sample(rows, args.limit)
            sample_kind = f"random, seed={args.seed}"
        else:
            rows = rows[: args.limit]
            sample_kind = "first N rows"
    else:
        sample_kind = "all questions"

    print(f"{label}: {dataset_size} questions loaded from {source}")
    print(f"Asking {len(rows)} questions ({sample_kind}) via {args.proxy_url} at concurrency {args.concurrency}...")
    print("Note: CPU-only inference with a single model slot - a large run takes a while.")
    print()

    records: list[dict] = []
    started = time.monotonic()
    with ThreadPoolExecutor(max_workers=max(1, args.concurrency)) as pool:
        futures = [pool.submit(ask, row, args) for row in rows]
        for index, future in enumerate(futures, start=1):
            record = future.result()
            records.append(record)
            if not args.quiet:
                status = record["http_status"] or "ERR"
                flag = "BLOCKED" if record["blocked_by_policy"] else ("ok" if not record["error"] else "failed")
                ident = record.get("financebench_id") or f"mmlu#{record.get('mmlu_index')} {record.get('subject')}"
                extra = ""
                if is_mmlu and not record["error"]:
                    extra = f" pred={record['predicted_letter'] or '-'} gold={record['gold_letter']}" + (" OK" if record["correct"] else "")
                print(f"  [{index}/{len(rows)}] {ident} HTTP {status} {flag}{extra} ({record['latency_ms']}ms)", flush=True)

    if not args.no_verdicts:
        print("\nJoining governance verdicts (letting shadow-path verdicts settle)...")
        time.sleep(5)
        with ThreadPoolExecutor(max_workers=min(8, max(1, args.concurrency))) as pool:
            list(pool.map(lambda record: fetch_verdicts(record, args), records))

    summary = report(records, args, dataset_size, sample_kind, source)
    print(f"Wall clock: {time.monotonic() - started:.0f}s")

    if args.json_out:
        output = Path(args.json_out)
        output.write_text(
            json.dumps({"summary": summary, "results": records}, indent=2, default=str),
            encoding="utf-8",
        )
        print(f"Per-question results written to {output}")

    # Exit non-zero only if nothing worked at all, so CI can distinguish
    # "benchmark ran, scored badly" from "benchmark could not run".
    return 0 if summary["answered"] > 0 or summary["blocked"] > 0 else 1


if __name__ == "__main__":
    sys.exit(main())
