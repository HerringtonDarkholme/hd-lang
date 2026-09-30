#!/bin/sh
# Runs the hd_nvim headless test. Needs only nvim (0.10 or later).
set -eu
here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
exec nvim --headless -u NONE -i NONE -n -l "$here/run.lua"
