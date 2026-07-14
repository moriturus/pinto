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
trap 'rm -rf "$temporary_dir"' EXIT HUP INT TERM

package --list >"$package_list"
diff -u "$root/release/package-files.txt" "$package_list"

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
