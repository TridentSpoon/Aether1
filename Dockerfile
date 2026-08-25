FROM python:3.11-slim

WORKDIR /app

# Install system audio and build tools
RUN apt-get update && apt-get install -y --no-install-recommends \
    gcc \
    libasound2-dev \
    libgirepository1.0-dev \
    libcairo2-dev \
    pkg-config \
    python3-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy backend requirements and install
COPY backend/requirements.txt /app/backend/requirements.txt
RUN pip install --no-cache-dir -r /app/backend/requirements.txt

# Copy application files
COPY backend /app/backend
COPY frontend /app/frontend
COPY desktop /app/desktop

# Generate icons
RUN python desktop/generate_icons.py

EXPOSE 8378

ENV PYTHONUNBUFFERED=1

CMD ["uvicorn", "backend.main:app", "--host", "0.0.0.0", "--port", "8378"]
