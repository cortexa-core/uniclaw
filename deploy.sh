#!/bin/bash
set -e

RPI_HOST="${RPI_HOST:-rpi4.local}"
RPI_USER="${RPI_USER:-pi}"
RPI_DIR="${RPI_DIR:-/home/pi/uniclaw}"

echo "Building for aarch64..."
cargo zigbuild --target aarch64-unknown-linux-gnu --release

echo "Deploying to ${RPI_USER}@${RPI_HOST}..."
rsync -avz --progress \
  target/aarch64-unknown-linux-gnu/release/uniclaw \
  ${RPI_USER}@${RPI_HOST}:${RPI_DIR}/

rsync -avz --progress \
  config/ ${RPI_USER}@${RPI_HOST}:${RPI_DIR}/config/
rsync -avz --progress \
  data/ ${RPI_USER}@${RPI_HOST}:${RPI_DIR}/data/

echo ""
echo "Done. Run on RPi:"
echo "  ssh ${RPI_USER}@${RPI_HOST} 'cd ${RPI_DIR} && ./uniclaw chat'"
