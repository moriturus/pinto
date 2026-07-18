#!/bin/sh
set -eu

root=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
if [ "$#" -eq 0 ]; then
    :
elif [ "$#" -eq 2 ] && [ "$1" = "--root" ]; then
    root=$(CDPATH='' cd -- "$2" && pwd)
else
    echo "usage: $0 [--root PATH]" >&2
    exit 2
fi

failures=0
error() {
    printf 'release metadata: %s\n' "$1" >&2
    failures=1
}

manifest_version=$(awk '
    /^\[package\]$/ { in_package = 1; next }
    in_package && /^\[/ { exit }
    in_package && /^version[[:space:]]*=/ {
        value = $0
        sub(/^[^\"]*\"/, "", value)
        sub(/\".*$/, "", value)
        print value
        exit
    }
' "$root/Cargo.toml" 2>/dev/null || true)
if [ -z "$manifest_version" ]; then
    error "package version: Cargo.toml has no package version"
elif ! printf '%s\n' "$manifest_version" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    error "package version: invalid package version $manifest_version"
fi

lock_version() {
    awk '
        /^\[\[package\]\]$/ { matching = 0 }
        /^name[[:space:]]*=[[:space:]]*"pinto-cli"[[:space:]]*$/ { matching = 1; next }
        matching && /^version[[:space:]]*=/ {
            value = $0
            sub(/^[^\"]*\"/, "", value)
            sub(/\".*$/, "", value)
            print value
            exit
        }
    ' "$1" 2>/dev/null || true
}

lockfiles=$(git -C "$root" ls-files | awk '$0 == "Cargo.lock" || $0 ~ /\/Cargo\.lock$/')
if [ -z "$lockfiles" ]; then
    error "lockfiles: no committed Cargo.lock file was found"
else
    for relative in $lockfiles; do
        lockfile="$root/$relative"
        found=$(lock_version "$lockfile")
        case "$relative" in
            Cargo.lock) label="root lockfile" ;;
            *) label="${relative%/Cargo.lock} lockfile" ;;
        esac
        if [ -z "$found" ]; then
            error "$label: no pinto-cli package entry was found"
        elif [ "$found" != "${manifest_version:-}" ]; then
            error "$label: pinto-cli is $found, expected package version ${manifest_version:-unknown}"
        fi
    done
fi

# git tag is the publication source of truth for the latest released version.
latest_tag=""
for tag in $(git -C "$root" tag --list --sort=-version:refname); do
    normalized=$(printf '%s\n' "$tag" | sed 's/^v//')
    if printf '%s\n' "$normalized" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
        latest_tag=$normalized
        break
    fi
done
if [ -z "$latest_tag" ]; then
    error "release tag: no semantic-version release tag was found"
else
    if [ "$latest_tag" != "${manifest_version:-}" ]; then
        error "release tag: latest tag is $latest_tag, expected package version ${manifest_version:-unknown}"
    fi
fi

has_release_tag() {
    expected=$1
    for tag in $(git -C "$root" tag --list --sort=-version:refname); do
        normalized=$(printf '%s\n' "$tag" | sed 's/^v//')
        if [ "$normalized" = "$expected" ]; then
            return 0
        fi
    done
    return 1
}

readme="$root/README.md"
installation="$root/docs/book/src/installation.md"
if [ -n "$latest_tag" ]; then
    readme_heading='The latest published release is `'$latest_tag'`.'
    install_command="cargo install pinto-cli --version $latest_tag"
    book_heading="Install the latest published $latest_tag binary with Cargo:"
    if [ ! -f "$readme" ] || ! grep -Fq "$readme_heading" "$readme" || ! grep -Fq "$install_command" "$readme"; then
        error "installation example: README.md does not consistently reference published version $latest_tag"
    fi
    if [ ! -f "$installation" ] || ! grep -Fq "$book_heading" "$installation" || ! grep -Fq "$install_command" "$installation"; then
        error "installation example: docs/book/src/installation.md does not consistently reference published version $latest_tag"
    fi
fi

changelog="$root/CHANGELOG.md"
if [ ! -f "$changelog" ] || ! grep -Eq '^## \[Unreleased\]$' "$changelog"; then
    error "changelog heading: CHANGELOG.md must start its release sections with an undated [Unreleased] heading"
fi
release_headings=$(sed -n 's/^## \[\([0-9][0-9]*\.[0-9][0-9]*\.[0-9][0-9]*\)\].*/\1/p' "$changelog" 2>/dev/null || true)
first_release=$(printf '%s\n' "$release_headings" | head -n 1)
if [ -z "$first_release" ]; then
    error "changelog heading: CHANGELOG.md has no dated release heading"
elif [ -n "$latest_tag" ] && [ "$first_release" != "$latest_tag" ]; then
    error "changelog heading: first dated release is $first_release, expected latest tag $latest_tag"
fi
while IFS= read -r heading; do
    if [ -n "$heading" ] && ! has_release_tag "$heading"; then
        error "changelog heading: dated release $heading has no matching release tag"
    fi
done <<EOF
$release_headings
EOF
tag_headings=$(for tag in $(git -C "$root" tag --list --sort=-version:refname); do
    normalized=$(printf '%s\n' "$tag" | sed 's/^v//')
    if printf '%s\n' "$normalized" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
        printf '%s\n' "$normalized"
    fi
done)
for heading in $tag_headings; do
    if ! printf '%s\n' "$release_headings" | grep -Fqx "$heading"; then
        error "release tag: tag $heading has no matching CHANGELOG heading"
    fi
done

compatibility="$root/docs/stability.md"
for section in \
    'SQLite schema v1 to v2 compatibility' \
    'Affected users' \
    'Symptoms' \
    'Back up before upgrading' \
    'Downgrade and recovery'
do
    if [ ! -f "$compatibility" ] || ! grep -Fq "$section" "$compatibility"; then
        error "compatibility documentation: docs/stability.md is missing the $section guidance"
    fi
done

if [ "$failures" -ne 0 ]; then
    echo "release metadata check failed" >&2
    exit 1
fi

printf 'release metadata is consistent for version %s\n' "$manifest_version"
