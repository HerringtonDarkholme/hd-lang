#!/bin/sh
# Trivial second "implementation" for demonstrating cross-impl mode: forwards
# every command to the real compiler. Any disagreement it shows is fuzzer noise.
here=$(cd "$(dirname "$0")/../.." && pwd)
exec node --experimental-strip-types "$here/bin/hd.js" "$@"
