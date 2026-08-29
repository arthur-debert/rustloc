#!/usr/bin/env bash
# Prove that `cargo install rustloc` builds when Cargo resolves the dependency
# graph from scratch — the path every user takes, and the one the checked-in
# Cargo.lock hides from the workspace build and from `cargo install --locked`.
#
# rustloc 0.22.0 shipped broken on that path: `ruff_python_ast` derives
# `get_size2::GetSize` on syntax nodes holding a `compact_str 0.9::CompactString`
# while requiring only `get-size2 = "0.10.0"`, and get-size2 0.10.2 moved its
# optional `compact_str` to 0.10 — so a fresh resolution picked 0.10.3, put two
# `compact_str` versions in the graph, and rustc rejected the derive (issue
# #154). crates/rustloclib/Cargo.toml now pins `get-size2 = "=0.10.1"`; this
# check is what notices if that pin stops working or is removed.
#
# The check builds in the debug profile: it is the resolution and the trait
# lookup that can fail here, and neither depends on optimisation level, so the
# cheaper profile buys the same verdict for a CI lane that runs on every push.
set -euo pipefail

expected_get_size2="0.10.1"
expected_compact_str_line="0.9"

repo_root="$(cd -P -- "$(dirname -- "${BASH_SOURCE[0]:-$0}")/.." && pwd)"
workspace_version="$(awk -F'"' '/^version = /{print $2; exit}' "$repo_root/Cargo.toml")"

# A copy WITHOUT Cargo.lock, so resolution starts from the manifests alone.
# Copying also keeps the run from touching the repo's own lockfile.
work="$(mktemp -d "${TMPDIR:-/tmp}/rustloc-unlocked-install.XXXXXX")"
trap 'rm -rf "$work"' EXIT
cp -R \
	"$repo_root/Cargo.toml" \
	"$repo_root/crates" \
	"$repo_root/README.md" \
	"$repo_root/LICENSE" \
	"$work/"

# Compiled artifacts survive the temporary source copy, so a repeat run — a
# laptop iterating on the pin, a CI job with a warm Rust cache — is incremental.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$repo_root/target}/unlocked-install"

cd "$work"

# Every version of PKG in the freshly written lockfile, one per line. Cargo
# writes `name` before `version` in each [[package]] block.
locked_versions() {
	awk -v pkg="\"$1\"" '
		$1 == "name" { name = $3 }
		$1 == "version" && name == pkg { print $3 }
	' Cargo.lock | tr -d '"'
}

echo "==> resolving crates/rustloc and its dependencies without a lockfile"
cargo generate-lockfile

get_size2_versions="$(locked_versions get-size2)"
if [ "$get_size2_versions" != "$expected_get_size2" ]; then
	echo "FAIL: fresh resolution chose get-size2 [$get_size2_versions], expected exactly $expected_get_size2." >&2
	echo "      See the get-size2 pin in crates/rustloclib/Cargo.toml." >&2
	exit 1
fi

while read -r version; do
	case "$version" in
	"$expected_compact_str_line".*) ;;
	*)
		echo "FAIL: fresh resolution pulled compact_str $version alongside ruff's ${expected_compact_str_line}.x." >&2
		echo "      Two compact_str versions in one graph is the state that breaks the GetSize derive." >&2
		exit 1
		;;
	esac
done <<<"$(locked_versions compact_str)"

echo "==> installing from that resolution (no --locked)"
cargo install --debug --path crates/rustloc --root "$work/install"

reported="$("$work/install/bin/rustloc" --version)"
case "$reported" in
*"$workspace_version"*) ;;
*)
	echo "FAIL: installed binary reports '$reported', expected version $workspace_version." >&2
	exit 1
	;;
esac

echo "OK: unlocked install builds and reports '$reported' (get-size2 $get_size2_versions)."
