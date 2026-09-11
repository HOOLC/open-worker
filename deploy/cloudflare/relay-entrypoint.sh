#!/bin/sh
set -eu
# Keys are public endpoint identities, never private keys or invitation secrets.
keys=${ALLOWED_KEYS:-}
list=''
old_ifs=$IFS
IFS=,
for key in $keys; do
  case "$key" in *[!0-9a-f]*|'') exit 1 ;; esac
  [ "${#key}" -eq 64 ] || exit 1
  list="$list\"$key\","
done
IFS=$old_ifs
cat > /tmp/relay.toml <<EOF
enable_relay = true
http_bind_addr = "0.0.0.0:8080"
enable_quic_addr_discovery = false
enable_metrics = true
metrics_bind_addr = "0.0.0.0:9090"
access.allowlist = [$list]
[limits.client_rx]
bytes_per_second = 4194304
max_burst_bytes = 8388608
EOF
exec /usr/local/bin/iroh-relay --config-path /tmp/relay.toml
