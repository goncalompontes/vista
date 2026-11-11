#!/usr/bin/env bash
# Build, install, launch, and inspect Vista in a vphone-linux iOS VM.
# The iOS guest needs to be already running and its vphoned control channel ready.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
vphone="${VPHONE_HOME:-$HOME/projects/vphone-linux}"
cid="${VPHONE_CID:-3}"
bundle_id="dev.vista.client"
ipa="$here/xtool/Vista.ipa"

usage() {
  echo "Usage: $0 {check|build|install|run|logs|crashes}"
  echo "  check     Check xtool and the guest connection"
  echo "  build     Create the native iOS IPA using xtool"
  echo "  install   Install a previously built IPA into the guest"
  echo "  run       Build, install and launch Vista"
  echo "  logs      Read the last 30 seconds of Vista logs"
  echo "  crashes   List Vista crash reports"
  echo "Environment: VPHONE_HOME (default $HOME/projects/vphone-linux), VPHONE_CID (default 3)"
}

require_vphone() {
  if [[ ! -x "$vphone/tools/vphoned.sh" || ! -f "$vphone/tools/install-ipa.py" ]]; then
    echo "vphone-linux tooling not found at $vphone; set VPHONE_HOME." >&2
    exit 1
  fi
}
require_xtool() {
  if ! command -v xtool >/dev/null 2>&1; then
    echo "xtool is missing. See ../README.md (Native iOS on Linux)." >&2
    exit 1
  fi
}
require_ipa() {
  if [[ ! -f "$ipa" ]]; then
    echo "Missing $ipa. Run $0 build first." >&2
    exit 1
  fi
}
install() {
  require_vphone
  require_ipa
  python3 "$vphone/tools/install-ipa.py" "$ipa" --launch "$bundle_id" --cid "$cid"
}

case "${1:-}" in
  check)
    require_xtool
    require_vphone
    xtool --version
    if ! command -v swift >/dev/null 2>&1; then
      echo 'Swift is not installed or is missing from PATH.' >&2
      exit 1
    fi
    if ! xtool sdk status | tee /dev/stderr | grep -q 'Darwin SDK is installed'; then
      echo 'Install an Apple SDK using xtool sdk install before building.' >&2
      exit 1
    fi
    if ! command -v socat >/dev/null 2>&1; then
      echo 'socat is required to communicate with the running vphone guest.' >&2
      exit 1
    fi
    "$vphone/tools/vphoned.sh" -c "$cid" GET /v1/health
    ;;
  build)
    require_xtool
    cd "$here"
    xtool dev build --ipa
    require_ipa
    ;;
  install) install ;;
  run)
    "$0" build
    install
    ;;
  logs)
    require_vphone
    python3 "$vphone/tools/app-logs.py" Vista --subsystem "$bundle_id" --seconds 30 --cid "$cid"
    ;;
  crashes)
    require_vphone
    "$vphone/tools/vphoned.sh" -c "$cid" POST /v1/rpc "{\"method\":\"logs.crashes\",\"params\":{\"bundle_id\":\"$bundle_id\"},\"id\":1}"
    ;;
  *) usage; exit 2 ;;
esac
