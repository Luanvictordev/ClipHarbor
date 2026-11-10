FROM python:3.12-slim

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    ffmpeg \
    && rm -rf /var/lib/apt/lists/*

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY . .

EXPOSE 8899
ENV HOST=0.0.0.0
ENV PORT=8899

CMD ["sh", "-c", "gunicorn app:app --bind 0.0.0.0:${PORT:-8899} --workers ${WEB_CONCURRENCY:-1} --timeout 900 --keep-alive 5"]
