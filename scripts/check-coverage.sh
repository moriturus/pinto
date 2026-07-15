#!/bin/sh
set -eu

coverage_file=${1:-coverage.xml}
minimum=${2:-0.95}

if [ ! -f "$coverage_file" ]; then
    echo "coverage report not found: $coverage_file" >&2
    exit 1
fi

line_rate=$(awk '
    match($0, /line-rate="[^"]+"/) {
        print substr($0, RSTART + 11, RLENGTH - 12)
        exit
    }
' "$coverage_file")

if [ -z "$line_rate" ]; then
    echo "Cobertura report does not contain a line-rate attribute: $coverage_file" >&2
    exit 1
fi

if ! awk -v actual="$line_rate" -v minimum="$minimum" '
    BEGIN {
        valid_number = "^[0-9]+([.][0-9]+)?$"
        if (actual !~ valid_number || minimum !~ valid_number || actual + 0 < minimum + 0) {
            exit 1
        }
    }
'; then
    printf 'Cobertura line coverage %.2f%% is below the %.2f%% minimum\n' \
        "$(awk -v value="$line_rate" 'BEGIN { print value * 100 }')" \
        "$(awk -v value="$minimum" 'BEGIN { print value * 100 }')" >&2
    exit 1
fi

printf 'Cobertura line coverage: %.2f%% (minimum %.2f%%)\n' \
    "$(awk -v value="$line_rate" 'BEGIN { print value * 100 }')" \
    "$(awk -v value="$minimum" 'BEGIN { print value * 100 }')"
