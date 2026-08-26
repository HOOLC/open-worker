#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../.." && pwd)
if [[ $# -ne 1 ]]; then
  echo "usage: $0 <task-id>" >&2
  exit 2
fi
task_name=$1
task_list="$script_dir/deepswe-seed0-task-ids.txt"
task_is_fixed=false
while IFS= read -r fixed_task; do
  if [[ "$task_name" == "$fixed_task" ]]; then
    task_is_fixed=true
    break
  fi
done < "$task_list"
if [[ "$task_is_fixed" != true ]]; then
  echo "task is not in the fixed DeepSWE seed-0 subset: $task_name" >&2
  exit 2
fi

dataset_root=${ZORK_DEEPSWE_DATASET:-"$repo_root/.data/benchmarks/deep-swe"}
jobs_root=${ZORK_DEEPSWE_JOBS:-"$repo_root/artifacts/deepswe"}
dataset_commit=435ee89ec2f2e2289f33b0da4f992f0b7b7266b9
no_streaming=${ZORK_DEEPSWE_NO_STREAMING:-false}
if [[ "$no_streaming" != true && "$no_streaming" != false ]]; then
  echo "ZORK_DEEPSWE_NO_STREAMING must be true or false" >&2
  exit 2
fi
parallel_tool_calls=${ZORK_DEEPSWE_PARALLEL_TOOL_CALLS:-false}
if [[ "$parallel_tool_calls" != true && "$parallel_tool_calls" != false ]]; then
  echo "ZORK_DEEPSWE_PARALLEL_TOOL_CALLS must be true or false" >&2
  exit 2
fi
context_window_tokens=${ZORK_DEEPSWE_CONTEXT_WINDOW_TOKENS:-}
if [[ -n "$context_window_tokens" && ! "$context_window_tokens" =~ ^[1-9][0-9]*$ ]]; then
  echo "ZORK_DEEPSWE_CONTEXT_WINDOW_TOKENS must be a positive integer" >&2
  exit 2
fi
max_output_tokens=${ZORK_DEEPSWE_MAX_OUTPUT_TOKENS:-}
if [[ -n "$max_output_tokens" && ! "$max_output_tokens" =~ ^[1-9][0-9]*$ ]]; then
  echo "ZORK_DEEPSWE_MAX_OUTPUT_TOKENS must be a positive integer" >&2
  exit 2
fi
profile_temp=
cleanup() {
  if [[ -n "$profile_temp" && -d "$profile_temp" ]]; then
    rm -rf "$profile_temp"
  fi
}
trap cleanup EXIT

if [[ -n "${ZORK_DEEPSWE_PROFILE_FILE:-}" ]]; then
  profile_file=$ZORK_DEEPSWE_PROFILE_FILE
  model=${ZORK_DEEPSWE_MODEL:?ZORK_DEEPSWE_MODEL is required with ZORK_DEEPSWE_PROFILE_FILE}
  thinking=${ZORK_DEEPSWE_THINKING:?ZORK_DEEPSWE_THINKING is required with ZORK_DEEPSWE_PROFILE_FILE}
  auth_domains=${ZORK_DEEPSWE_AUTH_DOMAINS:-}
else
  profile_preset=${ZORK_DEEPSWE_PROFILE_PRESET:-opencode-go}
  profile_temp=$(mktemp -d "${TMPDIR:-/tmp}/zork-deepswe-profile.XXXXXX")
  case "$profile_preset" in
    opencode-go)
      profile_file="$profile_temp/open-code-go.json"
      auth_file=${ZORK_OPENCODE_AUTH_FILE:-"$HOME/.pi/agent/auth.json"}
      model=muse-spark-1.2-contributor
      thinking=xhigh
      auth_domains=
      ;;
    openai-subscription)
      profile_file="$profile_temp/openai-subscription.json"
      auth_file=${ZORK_OPENAI_AUTH_FILE:-"$HOME/.codex/auth.json"}
      model=gpt-5.6-luna
      thinking=max
      auth_domains=auth.openai.com
      ;;
    *)
      echo "unknown DeepSWE profile preset: $profile_preset" >&2
      exit 2
      ;;
  esac
  profile_arguments=(
    --preset "$profile_preset"
    --auth-file "$auth_file"
    --streaming true
    --parallel-tool-calls "$parallel_tool_calls"
  )
  if [[ -n "$context_window_tokens" ]]; then
    profile_arguments+=(--context-window-tokens "$context_window_tokens")
  fi
  if [[ -n "$max_output_tokens" ]]; then
    profile_arguments+=(--max-output-tokens "$max_output_tokens")
  fi
  uv run python "$script_dir/build_deepswe_profile.py" \
    "${profile_arguments[@]}" \
    --output "$profile_file"
fi

if [[ ! -f "$profile_file" ]]; then
  echo "DeepSWE profile file does not exist: $profile_file" >&2
  exit 2
fi

docker compose version >/dev/null
if [[ -n "${ZORK_DEEPSWE_BINARY:-}" ]]; then
  binary=$ZORK_DEEPSWE_BINARY
  if [[ ! -x "$binary" ]]; then
    echo "explicit zork-agent binary is not executable: $binary" >&2
    exit 2
  fi
else
  binary="$repo_root/target/deepswe/zork-agent-linux-amd64"
  docker buildx version >/dev/null
  "$script_dir/build-zork-agent-linux-amd64.sh" "$binary"
fi

if [[ ! -d "$dataset_root/.git" ]]; then
  mkdir -p "$(dirname "$dataset_root")"
  git clone https://github.com/datacurve-ai/deep-swe.git "$dataset_root"
fi
git -C "$dataset_root" fetch origin "$dataset_commit"
git -C "$dataset_root" checkout --detach "$dataset_commit"

cd "$repo_root"
uv run --with datacurve-pier==0.3.1 python \
  scripts/benchmarks/prepare_deepswe.py \
  --tasks-path "$dataset_root/tasks" \
  --n-tasks 10 \
  --sample-seed 0

mkdir -p "$jobs_root"
uv run --with datacurve-pier==0.3.1 --with zstandard pier run \
  --path "$dataset_root/tasks" \
  --include-task-name "$task_name" \
  --agent-import-path scripts.benchmarks.zork_deepswe_agent:ZorkDeepSweAgent \
  --model "$model" \
  --agent-kwarg "zork_binary=$binary" \
  --agent-kwarg "profile_file=$profile_file" \
  --agent-kwarg "thinking=$thinking" \
  --agent-kwarg "auth_domains=$auth_domains" \
  --agent-kwarg "no_streaming=$no_streaming" \
  --n-attempts 4 \
  --n-concurrent 4 \
  --max-retries 0 \
  --jobs-dir "$jobs_root" \
  --yes
