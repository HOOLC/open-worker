#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../.." && pwd)
output=${1:-"$repo_root/target/deepswe/zork-agent-linux-amd64"}
build_output=$(mktemp -d "${TMPDIR:-/tmp}/zork-deepswe-build.XXXXXX")
trap 'rm -rf "$build_output"' EXIT

mkdir -p "$(dirname "$output")"
docker buildx build \
  --platform linux/amd64 \
  --file "$script_dir/Dockerfile.zork-agent" \
  --target export \
  --output "type=local,dest=$build_output" \
  "$repo_root"
install -m 755 "$build_output/zork-agent" "$output"
file "$output"
shasum -a 256 "$output"
