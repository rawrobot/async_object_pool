#!/bin/bash
set +e

OUTPUT_DIR="coverage"

echo "Running workspace test coverage..."

# Clean and setup
rm -rf "$OUTPUT_DIR"
mkdir -p "$OUTPUT_DIR"

# Check if tarpaulin is available
if ! command -v cargo-tarpaulin &> /dev/null; then
    echo "Installing cargo-tarpaulin..."
    cargo install cargo-tarpaulin
fi

echo "========================================="
echo "Running workspace tests with coverage..."
echo "========================================="

# Run workspace coverage only
if cargo tarpaulin \
    --workspace \
    --out Html \
     --out Xml \
    --output-dir "$OUTPUT_DIR" \
    --skip-clean \
    --timeout 600 \
    --no-fail-fast; then

    echo "✅ Workspace coverage completed successfully"

    # Check if reports exist
    if [ -f "$OUTPUT_DIR/tarpaulin-report.html" ]; then
        echo "📊 HTML report: $OUTPUT_DIR/tarpaulin-report.html"

        # Show coverage summary if JSON exists
        if [ -f "$OUTPUT_DIR/tarpaulin-report.json" ]; then
            echo "📊 JSON report: $OUTPUT_DIR/tarpaulin-report.json"

            # Extract and display coverage stats
            total_lines=$(jq -r '.files | map(.lines) | add' "$OUTPUT_DIR/tarpaulin-report.json" 2>/dev/null || echo "unknown")
            covered_lines=$(jq -r '.files | map(.covered) | add' "$OUTPUT_DIR/tarpaulin-report.json" 2>/dev/null || echo "unknown")

            if [ "$total_lines" != "unknown" ] && [ "$covered_lines" != "unknown" ] && [ "$total_lines" -gt 0 ]; then
                coverage_percent=$(echo "scale=2; $covered_lines * 100 / $total_lines" | bc -l 2>/dev/null || echo "unknown")
                echo "📈 Coverage: $covered_lines/$total_lines lines (${coverage_percent}%)"
            fi
        fi

        echo "🌐 Open in browser: $OUTPUT_DIR/tarpaulin-report.html"
        echo "🎉 Coverage analysis complete!"
        exit 0
    else
        echo "❌ Coverage completed but HTML report not found"
        echo "Available files:"
        ls -la "$OUTPUT_DIR/"
        exit 1
    fi
else
    echo "❌ Workspace coverage failed"
    exit 1
fi
