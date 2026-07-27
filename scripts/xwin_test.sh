#!/bin/sh
# Cross-compile and run the workspace's unit tests for x86-64 Windows under wine.
#
# cargo-xwin supplies the `wine` runner for the Windows target; this points it at the project's
# shared prefix (see `wine_prefix.sh`) rather than whatever `$HOME` happens to hold. Arguments are
# forwarded to `cargo xwin test`.
set -e
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/.." && pwd)
. "$here/wine_prefix.sh"
jc3vrs_ensure_wine_prefix
cd "$repo"

# The workspace's `default-members` is just the injector, so a bare `cargo xwin test` runs the
# injector's (zero) tests and reports a green run that exercised nothing -- including none of the
# payload's. Select the whole workspace unless the caller scopes the run themselves.
case " $* " in
    *" -p "*|*" --package "*|*" --workspace "*|*" --all "*) scope= ;;
    *) scope=--workspace ;;
esac

# Unquoted on purpose: empty means "pass no scope flag".
# shellcheck disable=SC2086
cargo xwin test --xwin-cache-dir .xwin --target x86_64-pc-windows-msvc $scope "$@"
