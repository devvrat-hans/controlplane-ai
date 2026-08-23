"""
Guardrails Microservice — PII (Presidio) + Toxicity & Bias (transformers)

Exposes REST endpoints consumed by the Rust shadow-analysis service.
Each endpoint accepts text and returns structured scan results.
"""

import logging
import os
import time
from typing import List, Optional

from fastapi import FastAPI
from pydantic import BaseModel

logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(levelname)s] %(message)s")
logger = logging.getLogger("guardrails")

app = FastAPI(title="ControlPlane Guardrails", version="0.5.0")

# ─── Lazy-loaded engines (heavy imports deferred to first call) ───────────────

_presidio_analyzer = None
_presidio_anonymizer = None
_toxicity_pipeline = None
_bias_pipeline = None

TOXICITY_THRESHOLD = float(os.environ.get("TOXICITY_THRESHOLD", "0.5"))
BIAS_THRESHOLD = float(os.environ.get("BIAS_THRESHOLD", "0.66"))


def get_presidio_analyzer():
    global _presidio_analyzer
    if _presidio_analyzer is None:
        from presidio_analyzer import AnalyzerEngine
        _presidio_analyzer = AnalyzerEngine()
        logger.info("Presidio AnalyzerEngine initialized")
    return _presidio_analyzer


def get_presidio_anonymizer():
    global _presidio_anonymizer
    if _presidio_anonymizer is None:
        from presidio_anonymizer import AnonymizerEngine
        _presidio_anonymizer = AnonymizerEngine()
        logger.info("Presidio AnonymizerEngine initialized")
    return _presidio_anonymizer


def get_toxicity_pipeline():
    global _toxicity_pipeline
    if _toxicity_pipeline is None:
        try:
            from transformers import pipeline
            _toxicity_pipeline = pipeline(
                "text-classification",
                model="unitary/unbiased-toxic-roberta",
                top_k=None,
                truncation=True,
                max_length=512,
            )
            logger.info("Toxicity pipeline initialized (unitary/unbiased-toxic-roberta)")
        except Exception as e:
            logger.warning(f"Failed to init Toxicity pipeline: {e}")
    return _toxicity_pipeline


def get_bias_pipeline():
    global _bias_pipeline
    if _bias_pipeline is None:
        try:
            from transformers import pipeline
            _bias_pipeline = pipeline(
                "text-classification",
                model="valurank/distilroberta-bias",
                truncation=True,
                max_length=512,
            )
            logger.info("Bias pipeline initialized (valurank/distilroberta-bias)")
        except Exception as e:
            logger.warning(f"Failed to init Bias pipeline: {e}")
    return _bias_pipeline


# ─── Request/Response Models ──────────────────────────────────────────────────


class ScanRequest(BaseModel):
    text: str
    prompt: Optional[str] = None


class PIIEntity(BaseModel):
    entity_type: str
    start: int
    end: int
    score: float
    text: str


class PIIResponse(BaseModel):
    entities: List[PIIEntity]
    anonymized_text: str
    has_pii: bool
    duration_ms: float


class ToxicityResponse(BaseModel):
    is_toxic: bool
    score: float
    sanitized_text: str
    duration_ms: float


class BiasResponse(BaseModel):
    is_biased: bool
    score: float
    sanitized_text: str
    duration_ms: float


class HallucinationResponse(BaseModel):
    is_hallucinated: bool
    score: float
    reason: str
    duration_ms: float


class HealthResponse(BaseModel):
    status: str
    presidio: bool
    toxicity: bool
    bias: bool
    deepeval: bool


# ─── Endpoints ────────────────────────────────────────────────────────────────


@app.get("/health", response_model=HealthResponse)
def health():
    try:
        import deepeval
        deepeval_available = True
    except ImportError:
        deepeval_available = False

    return HealthResponse(
        status="ok",
        presidio=_presidio_analyzer is not None,
        toxicity=_toxicity_pipeline is not None,
        bias=_bias_pipeline is not None,
        deepeval=deepeval_available,
    )


@app.post("/scan/pii", response_model=PIIResponse)
def scan_pii(req: ScanRequest):
    start = time.time()
    analyzer = get_presidio_analyzer()
    anonymizer = get_presidio_anonymizer()

    results = analyzer.analyze(
        text=req.text,
        language="en",
        entities=None,
    )

    # Only flag genuinely sensitive PII types with high confidence
    SENSITIVE_PII_TYPES = {
        "PERSON", "EMAIL_ADDRESS", "PHONE_NUMBER", "US_SSN", "CREDIT_CARD",
        "US_BANK_NUMBER", "IBAN_CODE", "US_PASSPORT", "US_DRIVER_LICENSE",
        "IP_ADDRESS", "MEDICAL_LICENSE", "US_ITIN",
    }
    high_confidence_results = [
        r for r in results
        if r.score >= 0.7 and r.entity_type in SENSITIVE_PII_TYPES
    ]

    entities = []
    for r in high_confidence_results:
        entities.append(PIIEntity(
            entity_type=r.entity_type,
            start=r.start,
            end=r.end,
            score=r.score,
            text=req.text[r.start:r.end],
        ))

    anonymized = anonymizer.anonymize(text=req.text, analyzer_results=high_confidence_results)
    duration_ms = (time.time() - start) * 1000

    return PIIResponse(
        entities=entities,
        anonymized_text=anonymized.text,
        has_pii=len(entities) > 0,
        duration_ms=round(duration_ms, 2),
    )


@app.post("/scan/toxicity", response_model=ToxicityResponse)
def scan_toxicity(req: ScanRequest):
    start = time.time()
    pipe = get_toxicity_pipeline()

    if pipe is None:
        return ToxicityResponse(
            is_toxic=False,
            score=0.0,
            sanitized_text=req.text,
            duration_ms=0.0,
        )

    results = pipe(req.text)
    duration_ms = (time.time() - start) * 1000

    toxic_labels = {"toxic", "severe_toxic", "obscene", "threat", "insult",
                    "identity_hate", "sexual_explicit"}
    max_toxic_score = 0.0
    for label_result in results[0] if isinstance(results[0], list) else results:
        label = label_result["label"].lower()
        score = label_result["score"]
        if label in toxic_labels:
            max_toxic_score = max(max_toxic_score, score)

    is_toxic = max_toxic_score >= TOXICITY_THRESHOLD

    return ToxicityResponse(
        is_toxic=is_toxic,
        score=round(max_toxic_score, 4),
        sanitized_text=req.text,
        duration_ms=round(duration_ms, 2),
    )


@app.post("/scan/bias", response_model=BiasResponse)
def scan_bias(req: ScanRequest):
    start = time.time()
    pipe = get_bias_pipeline()

    if pipe is None:
        return BiasResponse(
            is_biased=False,
            score=0.0,
            sanitized_text=req.text,
            duration_ms=round((time.time() - start) * 1000, 2),
        )

    results = pipe(req.text)
    duration_ms = (time.time() - start) * 1000

    bias_score = 0.0
    for result in results if isinstance(results, list) else [results]:
        if isinstance(result, list):
            for item in result:
                if item["label"].lower() in ("biased", "bias", "label_1", "1"):
                    bias_score = max(bias_score, item["score"])
        elif isinstance(result, dict):
            if result["label"].lower() in ("biased", "bias", "label_1", "1"):
                bias_score = result["score"]

    is_biased = bias_score >= BIAS_THRESHOLD

    return BiasResponse(
        is_biased=is_biased,
        score=round(bias_score, 4),
        sanitized_text=req.text,
        duration_ms=round(duration_ms, 2),
    )


@app.post("/scan/hallucination", response_model=HallucinationResponse)
def scan_hallucination(req: ScanRequest):
    """
    Detect hallucinations using DeepEval's LLM-as-a-judge approach.
    Falls back to a simple NLI heuristic if DeepEval is not installed.
    
    The prompt field should contain the context/grounding document.
    The text field should contain the model's response to evaluate.
    """
    start = time.time()
    response_text = req.text
    context_text = req.prompt or ""

    # Try DeepEval first
    try:
        from deepeval import evaluate
        from deepeval.metrics import HallucinationMetric
        from deepeval.test_case import LLMTestCase

        if not context_text:
            # No context to compare against — can't assess hallucination
            return HallucinationResponse(
                is_hallucinated=False,
                score=0.0,
                reason="No context provided for hallucination check",
                duration_ms=round((time.time() - start) * 1000, 2),
            )

        test_case = LLMTestCase(
            actual_output=response_text,
            retrieval_context=[context_text],
        )

        metric = HallucinationMetric(threshold=0.5)
        metric.measure(test_case)

        score = metric.score
        is_hallucinated = metric.is_successful() is False
        reason = f"DeepEval hallucination score: {score:.4f}"

        logger.info(f"DeepEval hallucination check: score={score:.4f}, hallucinated={is_hallucinated}")

        return HallucinationResponse(
            is_hallucinated=is_hallucinated,
            score=round(score, 4),
            reason=reason,
            duration_ms=round((time.time() - start) * 1000, 2),
        )

    except ImportError:
        logger.debug("DeepEval not installed, using heuristic hallucination check")
    except Exception as e:
        logger.warning(f"DeepEval hallucination check failed: {e}, using heuristic")

    # Fallback: Simple heuristic hallucination detection
    # Check if response contains claims that are unsupported by context
    if not context_text:
        return HallucinationResponse(
            is_hallucinated=False,
            score=0.0,
            reason="No context — heuristic skip",
            duration_ms=round((time.time() - start) * 1000, 2),
        )

    # Simple NLI-like check: measure claim overlap between response and context
    response_words = set(response_text.lower().split())
    context_words = set(context_text.lower().split())

    # Remove stop words
    stop_words = {'the', 'a', 'an', 'is', 'are', 'was', 'were', 'be', 'been', 'being',
                  'have', 'has', 'had', 'do', 'does', 'did', 'will', 'would', 'could',
                  'should', 'may', 'might', 'shall', 'can', 'to', 'of', 'in', 'for',
                  'on', 'with', 'at', 'by', 'from', 'as', 'into', 'through', 'during',
                  'before', 'after', 'above', 'below', 'between', 'and', 'but', 'or',
                  'not', 'no', 'nor', 'so', 'yet', 'both', 'either', 'neither', 'each',
                  'every', 'all', 'any', 'few', 'more', 'most', 'other', 'some', 'such',
                  'than', 'too', 'very', 'just', 'about', 'also', 'it', 'its', 'this',
                  'that', 'these', 'those', 'i', 'me', 'my', 'we', 'our', 'you', 'your',
                  'he', 'him', 'his', 'she', 'her', 'they', 'them', 'their'}
    response_content = response_words - stop_words
    context_content = context_words - stop_words

    if not response_content:
        score = 0.0
    else:
        overlap = len(response_content & context_content)
        score = 1.0 - (overlap / len(response_content))

    is_hallucinated = score > 0.7
    reason = f"Heuristic overlap score: {score:.4f} (response words not in context)"

    return HallucinationResponse(
        is_hallucinated=is_hallucinated,
        score=round(score, 4),
        reason=reason,
        duration_ms=round((time.time() - start) * 1000, 2),
    )


if __name__ == "__main__":
    import uvicorn
    port = int(os.environ.get("PORT", "8200"))
    logger.info(f"Starting Guardrails service on port {port}")
    uvicorn.run(app, host="0.0.0.0", port=port)
