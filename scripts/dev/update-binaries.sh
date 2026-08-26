#!/usr/bin/env bash
set -euo pipefail

# Compile Linux binaries, atomically install them into .data/bin, then
# perform a controlled restart through `zork update`. Does not
# rebuild Docker images or recreate the container.

root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"

mkdir -p .data/bin

docker compose --profile tools run --rm --no-deps rust-build bash -c '
  set -euo pipefail
  cargo build --release -p zork -p zork-gateway -p zork-agent -p zork-call
  mkdir -p /src/.data/bin
  for bin in zork zork-gateway zork-agent zork-call zork-gh; do
    cp "/src/target/release/${bin}" "/src/.data/bin/${bin}.new"
    chmod +x "/src/.data/bin/${bin}.new"
    mv -f "/src/.data/bin/${bin}.new" "/src/.data/bin/${bin}"
    echo "wrote /src/.data/bin/${bin}"
  done
'
if docker compose ps -q zork | grep -q .; then
  docker compose exec zork /data/bin/zork update --data /data
else
  echo "zork is not running; binaries installed to .data/bin"
fi
