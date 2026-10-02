#!/usr/bin/env bash
# VoiceKey – scripts/download_model.sh
# Downloads a ggml Whisper model from Hugging Face and places it in models/.
#
# Usage:
#   ./scripts/download_model.sh [tiny|base|small|medium]
#
# Requires: curl

set -euo pipefail

MODEL="${1:-base}"
MODELS_DIR="$(dirname "$0")/../models"
BASE_URL="https://huggingface.co/ggerganov/whisper.cpp/resolve/main"

case "$MODEL" in
  tiny)   FILE="ggml-tiny.en.bin"   ;;
  base)   FILE="ggml-base.en.bin"   ;;
  small)  FILE="ggml-small.en.bin"  ;;
  medium) FILE="ggml-medium.en.bin" ;;
  *)
    echo "Unknown model: $MODEL. Choose one of: tiny base small medium"
    exit 1
    ;;
esac

mkdir -p "$MODELS_DIR"
TARGET="$MODELS_DIR/$FILE"

if [[ -f "$TARGET" ]]; then
  echo "Model already present: $TARGET"
  exit 0
fi

echo "Downloading $FILE (~$(
  case "$MODEL" in
    tiny)   echo "75 MB"  ;;
    base)   echo "142 MB" ;;
    small)  echo "466 MB" ;;
    medium) echo "1.5 GB" ;;
  esac
))…"

curl -L --progress-bar -o "$TARGET" "$BASE_URL/$FILE"

echo "✓ Saved to $TARGET"
