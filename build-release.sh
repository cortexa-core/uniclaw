#!/bin/bash
set -e

VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
DIST_DIR="dist"

echo "Building UniClaw v${VERSION} release binaries..."
echo ""

# Build web UI
echo "Building web UI..."
if [ -d "web" ] && [ -f "web/package.json" ]; then
    (cd web && npm run build)
    echo "Web UI built to web/dist/"
    echo ""
else
    echo "WARNING: web/ directory not found, skipping web UI build"
    echo ""
fi

# Ensure targets are installed
rustup target add aarch64-unknown-linux-gnu 2>/dev/null || true
rustup target add armv7-unknown-linux-gnueabihf 2>/dev/null || true
rustup target add x86_64-unknown-linux-gnu 2>/dev/null || true

mkdir -p "$DIST_DIR"

TARGETS=(
    "aarch64-unknown-linux-gnu"        # RPi 3/4/5, ARM64 SBCs
    "armv7-unknown-linux-gnueabihf"    # RPi 2, 32-bit ARM boards
    "x86_64-unknown-linux-gnu"         # x86 Linux, mini PCs, VMs
)

for TARGET in "${TARGETS[@]}"; do
    echo "Building for ${TARGET}..."
    cargo zigbuild --target "$TARGET" --release 2>&1 | tail -1

    BINARY="target/${TARGET}/release/uniclaw"
    if [ ! -f "$BINARY" ]; then
        echo "  ERROR: binary not found at ${BINARY}"
        continue
    fi

    SIZE=$(ls -lh "$BINARY" | awk '{print $5}')
    echo "  Binary: ${SIZE}"

    # Package: binary + config + data
    ARCHIVE_NAME="uniclaw-v${VERSION}-${TARGET}"
    STAGING="${DIST_DIR}/${ARCHIVE_NAME}"
    rm -rf "$STAGING"
    mkdir -p "$STAGING"/{config,data/memory,data/sessions,data/skills}

    cp "$BINARY" "$STAGING/"
    cp config/default_config.toml "$STAGING/config/config.toml"
    cp data/SOUL.md "$STAGING/data/"
    cp data/skills/*.md "$STAGING/data/skills/"

    (cd "$DIST_DIR" && tar czf "${ARCHIVE_NAME}.tar.gz" "${ARCHIVE_NAME}")
    TARSIZE=$(ls -lh "${DIST_DIR}/${ARCHIVE_NAME}.tar.gz" | awk '{print $5}')
    echo "  Archive: ${TARSIZE} → ${DIST_DIR}/${ARCHIVE_NAME}.tar.gz"
    rm -rf "$STAGING"
    echo ""
done

echo "Done. Release archives:"
ls -lh "${DIST_DIR}"/uniclaw-v${VERSION}-*.tar.gz
