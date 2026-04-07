#!/bin/bash
set -e
cd "$(dirname "$0")"

if ! command -v python3 >/dev/null 2>&1; then
    echo "Python 3 is required."
    exit 1
fi

if [ ! -d "venv" ]; then
    echo "Setting up virtual environment..."
    python3 -m venv venv
fi

source venv/bin/activate
pip install -q --upgrade pip
pip install -q -r requirements.txt

PORT="${PORT:-8899}"
export PORT

echo ""
echo "  ClipHarbor is running at http://localhost:$PORT"
echo ""
python3 app.py
