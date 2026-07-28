#!/usr/bin/env bash

set -euo pipefail

output_dir="${1:-target/book-deployment}"
main_ref="${MAIN_REF:-origin/main}"
develop_ref="${DEVELOP_REF:-origin/develop}"

if [[ $# -gt 1 ]]; then
    printf 'usage: %s [output-directory]\n' "$0" >&2
    exit 2
fi

build_book() {
    local ref="$1"
    local destination="$2"
    local source_dir

    if ! git rev-parse --verify --quiet "${ref}^{commit}" >/dev/null; then
        printf 'required Book ref is unavailable: %s\n' "$ref" >&2
        return 1
    fi

    source_dir="$(mktemp -d "${TMPDIR:-/tmp}/pinto-book.XXXXXX")"
    if ! git archive --format=tar "$ref" | tar -xf - -C "$source_dir"; then
        rm -rf "$source_dir"
        return 1
    fi

    if [[ ! -f "$source_dir/book.toml" ]]; then
        printf 'Book configuration is missing from ref: %s\n' "$ref" >&2
        rm -rf "$source_dir"
        return 1
    fi

    printf 'Building Book ref %s at %s\n' "$ref" "$destination"
    if ! mdbook build "$source_dir" --dest-dir "$destination"; then
        rm -rf "$source_dir"
        return 1
    fi
    rm -rf "$source_dir"
}

rm -rf "$output_dir"
mkdir -p "$output_dir"

# Keep the stable Book in one place. The root entry point redirects to it, so
# the main deployment and the latest route cannot drift or duplicate files.
build_book "$main_ref" "$output_dir/latest"
cat > "$output_dir/index.html" <<'EOF'
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta http-equiv="refresh" content="0; url=latest/">
  <link rel="canonical" href="latest/">
  <title>pinto documentation</title>
  <script>window.location.replace("latest/");</script>
</head>
<body>
  <p>Redirecting to the <a href="latest/">latest pinto documentation</a>.</p>
</body>
</html>
EOF

build_book "$develop_ref" "$output_dir/develop"

# Keep every semantic-version tag accepted by the workflow addressable,
# including tags with a prerelease suffix.
while IFS= read -r tag; do
    if [[ "$tag" =~ ^[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]]; then
        build_book "$tag" "$output_dir/$tag"
    fi
done < <(git tag --list)
