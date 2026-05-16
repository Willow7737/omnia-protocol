#!/usr/bin/env bash
set -euo pipefail

# Generate Software Bill of Materials (SBOM) for all workspace crates.
# Outputs CycloneDX format (JSON + XML) to sbom/ directory.

echo "=== Generating SBOM for Omnia Protocol ==="

# Install cargo-cyclonedx if not present
if ! command -v cargo-cyclonedx &> /dev/null; then
    echo "Installing cargo-cyclonedx..."
    cargo install cargo-cyclonedx
fi

# Clean and generate
rm -rf sbom/
mkdir -p sbom/

echo "Generating CycloneDX SBOM..."
cargo cyclonedx --all --format json --output-path sbom/
cargo cyclonedx --all --format xml --output-path sbom/

echo ""
echo "=== SBOM Generated ==="
ls -la sbom/
echo ""
echo "Commit the sbom/ directory to the repository for distribution."
