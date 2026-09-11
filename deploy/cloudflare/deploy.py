#!/usr/bin/env python3
"""Deploy with a private config and short-lived, isolated Docker registry credentials."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config", default="wrangler.local.json")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent
    config_path = (root / args.config).resolve()
    config = json.loads(config_path.read_text())
    if not config.get("account_id") or not config.get("vars", {}).get("ALLOWED_KEYS"):
        parser.error("config must set account_id and a nonempty ALLOWED_KEYS allowlist")
    env = os.environ.copy()
    host = env.get("DOCKER_HOST") or subprocess.check_output(
        ["docker", "context", "inspect", "--format", "{{.Endpoints.docker.Host}}"], text=True
    ).strip()
    original_dir = Path(env.get("DOCKER_CONFIG", str(Path.home() / ".docker")))
    original_file = original_dir / "config.json"
    original = json.loads(original_file.read_text()) if original_file.exists() else {}
    with tempfile.TemporaryDirectory(prefix="zork-network-docker-") as directory:
        # A nonempty auths map suppresses Docker's automatic keychain discovery.
        # Only the temporary Cloudflare registry token is stored here, then deleted.
        config_file = Path(directory) / "config.json"
        config_file.write_text(json.dumps({
            "auths": {"https://index.docker.io/v1/": {}},
            "credsStore": "",
            "cliPluginsExtraDirs": [str(original_dir / "cli-plugins"), *original.get("cliPluginsExtraDirs", [])],
        }))
        config_file.chmod(0o600)
        env.update(DOCKER_CONFIG=directory, DOCKER_HOST=host)
        env.pop("DOCKER_CONTEXT", None)
        return subprocess.call(
            ["npx", "--yes", "pnpm@10.33.0", "exec", "wrangler", "deploy", "--config", str(config_path)],
            cwd=root, env=env,
        )


if __name__ == "__main__":
    raise SystemExit(main())
