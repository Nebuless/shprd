#!/bin/bash
set -euo pipefail
if [[ "${1:-}" == "--" ]]; then shift; fi
case "${1:-all}" in
  report) exec python -m unittest discover -s tests -p 'test_report*.py' ;;
  all) python -m unittest discover -s tests -p 'test_*.py'; exec node --test tests/skills-schema.test.mjs tests/apply-safety.test.mjs ;;
  *) exec python -m unittest discover -s tests -p "test_${1//-/_}*.py" ;;
esac
