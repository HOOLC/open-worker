#!/usr/bin/env bash
set -euo pipefail

# Build admin-ui on the host and copy it to .data/ui. Control serves that
# directory over the existing /data mount. Does not rebuild the image.

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"

vp run --filter @zork/admin-ui build
rm -rf .data/ui
mkdir -p .data
cp -R apps/admin-ui/dist .data/ui
echo "wrote .data/ui"
