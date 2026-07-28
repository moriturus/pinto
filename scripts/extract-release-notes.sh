#!/bin/sh
set -eu

usage() {
    echo "usage: $0 RELEASE_TAG [--root PATH]" >&2
    exit 2
}

if [ "$#" -eq 1 ]; then
    release_tag=$1
    root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
elif [ "$#" -eq 3 ] && [ "$2" = "--root" ]; then
    release_tag=$1
    root=$(CDPATH='' cd -- "$3" && pwd)
else
    usage
fi

version=$(printf '%s\n' "$release_tag" | sed 's/^v//')
if ! printf '%s\n' "$version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    echo "release notes: tag $release_tag is not a semantic version" >&2
    exit 2
fi

changelog="$root/CHANGELOG.md"
if [ ! -f "$changelog" ]; then
    echo "release notes: CHANGELOG.md was not found under $root" >&2
    exit 1
fi

if ! awk -v version="$version" '
    /^## \[/ {
        heading = $0
        sub(/^## \[/, "", heading)
        sub(/\].*$/, "", heading)
        if (heading == version) {
            found = 1
            next
        }
        if (found) {
            exit
        }
    }
    found { print }
    END {
        if (!found) {
            exit 1
        }
    }
' "$changelog"; then
    echo "release notes: no CHANGELOG section matches release $version" >&2
    exit 1
fi
