#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"

package() {
    if [ "${ALLOW_DIRTY:-0}" = "1" ]; then
        cargo package --all-features --locked --allow-dirty "$@"
    else
        cargo package --all-features --locked "$@"
    fi
}

temporary_dir=$(mktemp -d "${TMPDIR:-/tmp}/pinto-package.XXXXXX")
package_list="$temporary_dir/package-files.txt"
baseline_without_sources="$temporary_dir/baseline-without-sources.txt"
package_without_sources="$temporary_dir/package-without-sources.txt"
package_sources="$temporary_dir/package-sources.txt"
trap 'rm -rf "$temporary_dir"' EXIT HUP INT TERM

package --list >"$package_list"
# `src/**` is an intentional recursive Cargo include. Compare the committed baseline exactly for
# every other package path, then verify the complete current source tree separately. This keeps the
# baseline useful for repository/runtime boundaries without making every new Rust module require a
# manually synchronized snapshot entry.
sed '/^src\//d' "$root/release/package-files.txt" >"$baseline_without_sources"
sed '/^src\//d' "$package_list" >"$package_without_sources"
if ! diff -u "$baseline_without_sources" "$package_without_sources"; then
    echo "package file baseline differs outside src/; update release/package-files.txt for an intentional package change" >&2
    exit 1
fi

find src -type f -print | LC_ALL=C sort >"$package_sources"
missing_source=0
while IFS= read -r source; do
    if ! grep -Fqx "$source" "$package_list"; then
        echo "source file is not included in the crate package: $source; update Cargo.toml include rules" >&2
        missing_source=1
    fi
done <"$package_sources"
if [ "$missing_source" -ne 0 ]; then
    exit 1
fi

package_marker="$temporary_dir/package-start"
touch "$package_marker"
package
package_file=$(find "$root/target/package" -maxdepth 1 -type f -name 'pinto-cli-*.crate' -newer "$package_marker" -print | sort | tail -n 1)
if [ -z "$package_file" ]; then
    echo "cargo package did not produce a .crate archive" >&2
    exit 1
fi

tar -xzf "$package_file" -C "$temporary_dir"
packaged_root=$(find "$temporary_dir" -mindepth 1 -maxdepth 1 -type d -print | head -n 1)
if [ -z "$packaged_root" ]; then
    echo "could not find extracted package root" >&2
    exit 1
fi

cargo test --all-features --locked --manifest-path "$packaged_root/Cargo.toml"
