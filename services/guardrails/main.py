"""
Guardrails Microservice — PII (Presidio) + Toxicity & Bias (transformers)

Exposes REST endpoints consumed by the Rust shadow-analysis service.
Each endpoint accepts text and returns structured scan results.
"""

import logging
import os
import threading
import time
from contextlib import asynccontextmanager
from typing import List, Optional

from fastapi import FastAPI
from pydantic import BaseModel

logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(levelname)s] %(message)s")
logger = logging.getLogger("guardrails")

# ─── Model engines: loaded once, under a lock ─────────────────────────────────
#
# Endpoints are sync, so FastAPI runs them on a thread pool. The shadow worker fires
# PII + toxicity + input-toxicity + input-bias scans concurrently, so without a lock
# every thread that arrived before the first load finished built its OWN spaCy/Presidio
# analyzer and transformer pipelines. After a restart that burst grew the process to
# ~6 GB and the kernel OOM-killed it (exit 137). One lock + an eager load at startup
# guarantees exactly one copy of each model.
_model_lock = threading.Lock()

_presidio_analyzer = None
_presidio_anonymizer = None
_toxicity_pipeline = None
_bias_pipeline = None

TOXICITY_THRESHOLD = float(os.environ.get("TOXICITY_THRESHOLD", "0.5"))
BIAS_THRESHOLD = float(os.environ.get("BIAS_THRESHOLD", "0.66"))


def get_presidio_analyzer():
    global _presidio_analyzer
    if _presidio_analyzer is None:
        with _model_lock:
            if _presidio_analyzer is None:
                from presidio_analyzer import AnalyzerEngine
                _presidio_analyzer = AnalyzerEngine()
                logger.info("Presidio AnalyzerEngine initialized")
    return _presidio_analyzer


def get_presidio_anonymizer():
    global _presidio_anonymizer
    if _presidio_anonymizer is None:
        with _model_lock:
            if _presidio_anonymizer is None:
                from presidio_anonymizer import AnonymizerEngine
                _presidio_anonymizer = AnonymizerEngine()
                logger.info("Presidio AnonymizerEngine initialized")
    return _presidio_anonymizer


def get_toxicity_pipeline():
    global _toxicity_pipeline
    if _toxicity_pipeline is None:
        with _model_lock:
            if _toxicity_pipeline is None:
                _toxicity_pipeline = _load_toxicity_pipeline()
    return _toxicity_pipeline


def _load_toxicity_pipeline():
    try:
        from transformers import pipeline
        loaded = pipeline(
            "text-classification",
            model="unitary/unbiased-toxic-roberta",
            top_k=None,
            truncation=True,
            max_length=512,
        )
        logger.info("Toxicity pipeline initialized (unitary/unbiased-toxic-roberta)")
        return loaded
    except Exception as e:
        logger.warning(f"Failed to init Toxicity pipeline: {e}")
        return None


def get_bias_pipeline():
    global _bias_pipeline
    if _bias_pipeline is None:
        with _model_lock:
            if _bias_pipeline is None:
                _bias_pipeline = _load_bias_pipeline()
    return _bias_pipeline


def _load_bias_pipeline():
    try:
        from transformers import pipeline
        loaded = pipeline(
            "text-classification",
            model="valurank/distilroberta-bias",
            truncation=True,
            max_length=512,
        )
        logger.info("Bias pipeline initialized (valurank/distilroberta-bias)")
        return loaded
    except Exception as e:
        logger.warning(f"Failed to init Bias pipeline: {e}")
        return None


@asynccontextmanager
async def lifespan(_app):
    # Load every model before the server accepts requests, so no request can race a
    # load. Scans that arrive during startup get "connection refused" and the shadow
    # worker records them as errors (fail open) instead of piling up here.
    started = time.time()
    get_presidio_analyzer()
    get_presidio_anonymizer()
    get_toxicity_pipeline()
    get_bias_pipeline()
    logger.info(f"All guardrails models loaded in {time.time() - started:.1f}s")
    yield


app = FastAPI(title="ControlPlane Guardrails", version="0.7.0", lifespan=lifespan)


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


class HealthResponse(BaseModel):
    status: str
    presidio: bool
    toxicity: bool
    bias: bool


# ─── Endpoints ────────────────────────────────────────────────────────────────


@app.get("/health", response_model=HealthResponse)
def health():
    return HealthResponse(
        status="ok",
        presidio=_presidio_analyzer is not None,
        toxicity=_toxicity_pipeline is not None,
        bias=_bias_pipeline is not None,
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


if __name__ == "__main__":
    import uvicorn
    port = int(os.environ.get("PORT", "8200"))
    logger.info(f"Starting Guardrails service on port {port}")
    uvicorn.run(app, host="0.0.0.0", port=port)
