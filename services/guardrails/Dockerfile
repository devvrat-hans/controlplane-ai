FROM python:3.11-slim

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

COPY requirements.txt .

# Install CPU-only PyTorch first (avoids 2GB+ CUDA download), then the rest
RUN pip install --no-cache-dir torch --index-url https://download.pytorch.org/whl/cpu && \
    pip install --no-cache-dir -r requirements.txt

# Download spaCy English model for Presidio NER
RUN python -m spacy download en_core_web_lg

COPY main.py .

# Pre-download HuggingFace models so first request is fast
RUN python -c "\
from transformers import pipeline; \
print('Downloading toxicity model...'); \
pipeline('text-classification', model='unitary/unbiased-toxic-roberta', top_k=None, truncation=True, max_length=512); \
print('Downloading bias model...'); \
pipeline('text-classification', model='valurank/distilroberta-bias', truncation=True, max_length=512); \
print('Models cached.') \
"

EXPOSE 8200

ENV PORT=8200

CMD ["python", "main.py"]
