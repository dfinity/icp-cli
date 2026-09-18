#!/usr/bin/env bash
#
# Fails if `icp-project`'s core reaches for something only a host has.
#
# Building the crate for `wasm32-unknown-unknown` does not prove this on its
# own: that target ships a full `std` whose `fs`, `process` and `env` backends
# compile and fail only at runtime, so a bare `std::fs::read_to_string` passes
# a build that was meant to reject it. What the build does leave behind is one
# relocatable wasm object per codegen unit, and there every symbol the crate
# calls but does not define is an import from the `env` module. So read those
# imports back and fail on any that names a corner of `std` only a host serves.
#
# This sees what `icp-project` itself calls, including whatever a dependency's
# generics monomorphise into it. What it cannot see is a dependency's own
# non-generic code, compiled once into that dependency's objects: a call that
# only arrives at the host a frame or two inside one of those is named here
# after the dependency rather than after `std`, and matches nothing in `std`.
# `camino`'s `Utf8Path::is_file` is such a front door, and the one this crate
# would reach for by accident, so those are denied by name too.

set -euo pipefail

cd "$(dirname "$0")/.."

# Paths in `std` that only a host can serve, written as `llvm-nm` demangles
# them. `std::sys` is std's own platform layer, listed because a call that
# arrives there from an inlined front door would otherwise go unseen.
denied='std::(fs|net|os|env|process|time)::'
denied+='|std::io::stdio::'
denied+='|std::sys::(args|env|fd|fs|net|os|pal|process|stdio|thread|time)'

# A dependency's own front door onto one of those. Found by reading these same
# imports out of every rlib in the `--no-default-features` closure and keeping
# the crates whose own objects reach a host corner of `std`: `camino`, `glob`,
# `handlebars`, `candid_parser`, `icp-canister-interfaces` and `snafu`. Every
# other crate there reaches the host only where the build already rejects it,
# gating the code on `cfg(unix)`/`cfg(windows)` — as `tokio`'s `process` and
# `signal` do, and as `rand`'s entropy source does. `snafu` earns no entry: its
# one reach is the `RUST_LIB_BACKTRACE` a captured `Backtrace` reads, and no
# error in this repo carries one.
denied+='|<(std::path::Path|camino::Utf8Path|camino::Utf8DirEntry)>::'
denied+='(try_exists|exists|is_file|is_dir|is_symlink|metadata|symlink_metadata'
denied+='|canonicalize|read_dir|read_link|file_type)'
denied+='|<camino::ReadDirUtf8'
denied+='|glob::glob'
denied+='|handlebars::.*register_template'
denied+='|candid_parser::.*check_file'
denied+='|icp_canister_interfaces::engine_canister::engine_canister_id'

# Not a host reach: `abort` is how a panic while unwinding gives up, and wasm
# has the instruction for it.
allowed='std::process::abort'

rlib=$(
  cargo build -p icp-project --no-default-features \
    --target wasm32-unknown-unknown --message-format=json |
    jq -r 'select(.reason == "compiler-artifact" and .target.name == "icp_project")
             | .filenames[] | select(endswith(".rlib"))' |
    tail -1
)
if [[ -z $rlib ]]; then
  echo "::error::cargo produced no icp-project rlib to inspect" >&2
  exit 1
fi

nm=$(find "$(rustc --print sysroot)" -name 'llvm-nm*' -type f -print -quit)
if [[ -z $nm ]]; then
  echo "::error::llvm-nm not found; run 'rustup component add llvm-tools'" >&2
  exit 1
fi

# An rlib also carries cargo's metadata members, which `llvm-nm` reports as
# holding no symbols; that is expected, so keep it out of the log.
symbols=$("$nm" --demangle --undefined-only "$rlib" 2> >(grep -v ': no symbols$' >&2))

reaches=$(printf '%s\n' "$symbols" | sed 's/^ *U //' | sort -u |
  grep -E "$denied" | grep -vxE "$allowed" || true)

if [[ -n $reaches ]]; then
  echo "::error::icp-project's core reaches the host; it must ask through a seam instead"
  echo "$reaches" | sed 's/^/  /'
  exit 1
fi

echo "icp-project's core reaches no host"
