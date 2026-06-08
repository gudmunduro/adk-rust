#!/bin/zsh
# Publish all workspace crates to crates.io in correct dependency order.
# Tiers generated from `cargo metadata` dependency graph (no dev-deps).
# Waits for crates.io indexing between publishes.
#
# Usage:
#   ./publish.sh          # publish all
#   ./publish.sh --resume # skip already-published crates

set -euo pipefail

CRATES=(
  # Tier 1: no internal library deps
  adk-core
  adk-anthropic
  adk-deploy
  adk-enterprise
  adk-rust-macros
  adk-telemetry
  awp-types

  # Tier 2: depends on Tier 1
  adk-action
  adk-artifact
  adk-awp
  adk-browser
  adk-gemini
  adk-guardrail
  adk-memory
  adk-mistralrs
  adk-plugin
  adk-sandbox
  adk-session

  # Tier 3: depends on Tier 1-2
  adk-code
  adk-graph
  adk-model
  adk-rag
  adk-realtime
  adk-retry-reflect
  adk-skill

  # Tier 4: depends on Tier 1-3
  adk-agent
  adk-audio
  adk-runner
  adk-tool

  # Tier 5: depends on Tier 1-4
  adk-acp
  adk-eval
  adk-managed
  adk-server

  # Tier 6: depends on Tier 1-5
  adk-auth
  adk-bench
  adk-cli

  # Tier 7: depends on Tier 1-6
  adk-payments
  cargo-adk

  # Tier 8: umbrella (depends on everything)
  adk-rust
)

echo "=== Publishing ADK-Rust ==="
echo "Total crates: ${#CRATES[@]}"
echo ""

PUBLISHED=0
SKIPPED=0
FAILED=0
FAILED_CRATES=()

for crate in "${CRATES[@]}"; do
  echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
  echo "📦 [$((PUBLISHED + SKIPPED + FAILED + 1))/${#CRATES[@]}] Publishing: $crate"
  echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

  OUTPUT=$(cargo publish -p "$crate" 2>&1)
  STATUS=$?

  echo "$OUTPUT"
  echo ""

  if echo "$OUTPUT" | grep -q "already exists\|already uploaded"; then
    echo "⏭  Already published"
    SKIPPED=$((SKIPPED + 1))
    sleep 1
  elif [ $STATUS -eq 0 ]; then
    echo "✅ Published"
    PUBLISHED=$((PUBLISHED + 1))
    echo "⏳ Waiting for crates.io indexing..."
    sleep 15
  else
    echo "❌ FAILED (exit $STATUS)"
    FAILED=$((FAILED + 1))
    FAILED_CRATES+=("$crate")
  fi

  echo ""
done

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "=== SUMMARY ==="
echo "✅ Published: $PUBLISHED"
echo "⏭  Skipped:   $SKIPPED"
echo "❌ Failed:    $FAILED"

if [ ${#FAILED_CRATES[@]} -gt 0 ]; then
  echo ""
  echo "Failed crates:"
  for c in "${FAILED_CRATES[@]}"; do
    echo "- $c"
  done
fi
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
