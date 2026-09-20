#!/bin/bash
set -euo pipefail
exec python "$(dirname "$0")/report.py" "$@"
