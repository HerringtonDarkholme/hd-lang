#!/bin/sh

set -eu

spec_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH= cd -- "$spec_dir/.." && pwd)
manifest="$spec_dir/conformance/cases.tsv"
examples="$spec_dir/conformance/examples.tsv"

fail() {
    printf '%s\n' "spec check failed: $*" >&2
    exit 1
}

for file in "$spec_dir"/*.md "$spec_dir"/lang/*.md "$spec_dir"/std/*.md "$spec_dir"/cli/*.md \
    "$spec_dir"/conformance/*.md; do
    fences=$(awk '/^```/{ count += 1 } END { print count + 0 }' "$file")
    [ $((fences % 2)) -eq 0 ] || fail "unbalanced code fence in $file"
done

tab=$(printf '\t')
awk -F "$tab" '
    NR == 1 {
        if ($0 != "path\tphase\texpectation\tspecification") exit 1
        next
    }
    NF != 4 || $1 == "" || $2 == "" || $3 == "" || $4 == "" { exit 1 }
' "$manifest" || fail "malformed conformance/cases.tsv"

tail -n +2 "$manifest" | while IFS="$tab" read -r path phase expectation section; do
    [ -f "$spec_dir/conformance/$path" ] || fail "missing fixture $path"
    marker_count=$(awk '/# (diagnostic|warning|panic): [a-z0-9-]+[[:space:]]*$/ { count += 1 } END { print count + 0 }' "$spec_dir/conformance/$path")

    case "$phase" in
        parse|type|runtime) ;;
        *) fail "unknown phase '$phase' for $path" ;;
    esac

    case "$expectation" in
        accept)
            [ "$marker_count" -eq 0 ] || fail "accept fixture $path declares an error marker"
            ;;
        reject:*|warn:*|panic:*)
            [ "$marker_count" -eq 1 ] || fail "fixture $path must declare exactly one expectation marker"
            ;;
        *) fail "unknown expectation '$expectation' for $path" ;;
    esac

    if grep -q '^# expect-stdout:' "$spec_dir/conformance/$path"; then
        [ "$phase" = runtime ] && [ "$expectation" = accept ] ||
            fail "fixture $path uses # expect-stdout: outside a runtime accept case"
    fi

    spec_file=${section%%#*}
    [ -f "$spec_dir/$spec_file" ] || fail "missing specification $spec_file for $path"

    case "$expectation" in
        reject:*)
            code=${expectation#reject:}
            grep -Fq "# diagnostic: $code" "$spec_dir/conformance/$path" ||
                fail "fixture $path does not declare diagnostic $code"
            grep -Fq "\`$code\`" "$spec_dir/README.md" ||
                fail "fixture $path uses unknown error category $code"
            ;;
        warn:*)
            code=${expectation#warn:}
            grep -Fq "# warning: $code" "$spec_dir/conformance/$path" ||
                fail "fixture $path does not declare warning $code"
            grep -Fq "\`$code\`" "$spec_dir/README.md" ||
                fail "fixture $path uses unknown warning category $code"
            ;;
        panic:*)
            code=${expectation#panic:}
            grep -Fq "# panic: $code" "$spec_dir/conformance/$path" ||
                fail "fixture $path does not declare panic $code"
            grep -Fq "\`$code\`" "$spec_dir/lang/06-control-flow.md" ||
                fail "fixture $path uses unknown panic category $code"
            ;;
    esac
done

if grep -R -n -E '^# expect-(error|warning|panic):' "$spec_dir/conformance" --include='*.hd'; then
    fail "legacy file-level fixture expectations found"
fi

if grep -R -n -E '^# [a-z][a-z-]*:' "$spec_dir/conformance" --include='*.hd' |
    grep -v -E ':# (test|expect|fixture-runtime-profile|fixture-runtime-scenario|fixture-runtime-pending-function|fixture-package-role|fixture-test-layout|fixture-package-tree|expect-stdout): ' |
    grep -v -E ':# expect-stdout:$'; then
    fail "fixture uses a header directive not defined in conformance/README.md"
fi

# Package sources (conformance/packages and conformance/trees) are inputs of
# the package-role and package-tree environments, not cases: no index row, no
# directives, and they must parse.
for packages_dir in "$spec_dir/conformance/packages" "$spec_dir/conformance/trees"; do
if [ -d "$packages_dir" ]; then
    if grep -R -n -E '^# [a-z][a-z-]*:|# (diagnostic|warning|panic): ' "$packages_dir" --include='*.hd'; then
        fail "package sources under ${packages_dir#"$spec_dir/"} must not carry fixture directives"
    fi
    packages_manifest=$(mktemp "${TMPDIR:-/tmp}/hd-spec-packages.XXXXXX")
    {
        printf 'path\tphase\texpectation\tspecification\n'
        find "$packages_dir" -type f -name '*.hd' | sort | while IFS= read -r file; do
            printf '%s\tparse\taccept\t-\n' "${file#"$spec_dir/conformance/"}"
        done
    } > "$packages_manifest"
    if ! node --experimental-strip-types "$spec_dir/reference-parser/index.ts" "$packages_manifest" "$spec_dir/conformance"; then
        rm -f "$packages_manifest"
        fail "a package source under ${packages_dir#"$spec_dir/"} does not parse"
    fi
    rm -f "$packages_manifest"
fi
done

# A package tree header names an existing tree and a path the tree leaves free.
grep -R -l -E '^# fixture-package-tree: ' "$spec_dir/conformance" --include='*.hd' | sort | while IFS= read -r file; do
    value=$(sed -n 's/^# fixture-package-tree: //p' "$file")
    tree=${value%%/*}
    [ -d "$spec_dir/conformance/trees/$tree" ] ||
        fail "fixture ${file#"$spec_dir/conformance/"} names a missing package tree $tree"
    [ ! -e "$spec_dir/conformance/trees/$value" ] ||
        fail "fixture ${file#"$spec_dir/conformance/"} takes $value, which the tree already holds"
done

if grep -R -n -E '^# expect: ' "$spec_dir/conformance" --include='*.hd' |
    grep -v -E ':# expect: (parse|accept|test)[[:space:]]*$'; then
    fail "fixture uses an undefined # expect: value"
fi

find "$spec_dir/conformance" \( -path "$spec_dir/conformance/packages" -o -path "$spec_dir/conformance/trees" \) -prune -o -type f -name '*.hd' -print | sort | while IFS= read -r file; do
    relative=${file#"$spec_dir/conformance/"}
    count=$(awk -F "$tab" -v path="$relative" 'NR > 1 && $1 == path { count += 1 } END { print count + 0 }' "$manifest")
    [ "$count" -eq 1 ] || fail "fixture $relative has $count manifest entries"
done

node --experimental-strip-types "$spec_dir/reference-parser/index.ts" "$manifest" "$spec_dir/conformance"
node --experimental-strip-types "$spec_dir/check-spec-anchors.ts" "$spec_dir" "$manifest"
# Dead rule citations (tools/README.md): citing a rule ID the chapters no
# longer define fails in spec/, fixtures, guide/, and lib/std, unless the line
# records history; records and src/ comments only warn.
node --experimental-strip-types "$spec_dir/tools/spec.ts" refs --dead --brief ||
    fail "a dead rule citation in spec/, a fixture, guide/, or lib/std (pnpm run spec refs --dead)"
# Tiers (conformance/README.md): a language-tier fixture imports no item that
# conformance/stdlib-items.tsv lists, except as conformance/tier-crossings.tsv
# records; each crossing row must still hold.
node --experimental-strip-types "$spec_dir/check-spec-tiers.ts" "$spec_dir" ||
    fail "a language-tier fixture uses a stdlib-tier item, or tier-crossings.tsv is stale"
# Style lint (spec/STYLE.md): long paragraphs and sentences only warn; rule ID
# syntax, placement, prefixes, and uniqueness fail.
node --experimental-strip-types "$spec_dir/check-spec-style.ts" "$spec_dir"

# Fuzzer (spec/tools/fuzz): the import gate, then a seeded smoke run whose only
# oracles are the reference parser and the spec inventory. No implementation
# is invoked here; implementation smoke runs live in `pnpm run fuzz:smoke`.
fuzz_dir="$spec_dir/tools/fuzz"
node --experimental-strip-types "$fuzz_dir/check-imports.ts" ||
    fail "spec/tools/fuzz imports something outside Node built-ins and spec/"
fuzz_tmp=$(mktemp -d "${TMPDIR:-/tmp}/hd-spec-fuzz.XXXXXX")
trap 'rm -rf "$fuzz_tmp"' EXIT
node --experimental-strip-types "$fuzz_dir/grammar-check.ts" --seed smoke --cases 200 --show 0 ||
    fail "grammar-check smoke run crashed"
node --experimental-strip-types "$fuzz_dir/fuzz.ts" --reference-only --fail-on all \
    --seed smoke --cases 200 --out "$fuzz_tmp/out" --work "$fuzz_tmp/work" \
    ${HD_SPEC_JOBS:+--jobs "$HD_SPEC_JOBS"} ||
    fail "reference-only fuzz smoke run found a signature"

awk -F "$tab" '
    NR == 1 {
        if ($0 != "specification\tblock\tclassification\tfixture") exit 1
        next
    }
    NF != 4 || $1 == "" || $2 !~ /^[1-9][0-9]*$/ || $3 == "" || $4 == "" {
        exit 1
    }
    seen[$1 SUBSEP $2]++ { exit 1 }
' "$examples" || fail "malformed or duplicate conformance/examples.tsv entry"

# Chapters: the numbered language chapters in lang/, the stdlib chapters in
# std/ (std/README.md), and the CLI chapters in cli/ (cli/README.md). Each
# chapter's text examples are inventoried.
for file in "$spec_dir"/lang/[0-9][0-9]-*.md "$spec_dir"/std/*.md "$spec_dir"/cli/*.md; do
    name=${file#"$spec_dir/"}
    case $name in std/README.md | cli/README.md) continue ;; esac
    blocks=$(awk '/^```text/{ count += 1 } END { print count + 0 }' "$file")
    indexed=$(awk -F "$tab" -v name="$name" 'NR > 1 && $1 == name { count += 1 } END { print count + 0 }' "$examples")
    [ "$blocks" -eq "$indexed" ] ||
        fail "$name has $blocks text examples but $indexed inventory entries"
done

grep -Fq '```ebnf' "$spec_dir/lang/02-grammar.md" ||
    fail "02-grammar.md does not contain consolidated EBNF"

for production in data_decl use_decl requirement_clause context_scope \
    decorated_decl enum_variant generic_parameter; do
    grep -Eq "^${production}[[:space:]]*=" "$spec_dir/lang/02-grammar.md" ||
        fail "02-grammar.md is missing $production"
done

if grep -Eq '^struct_decl[[:space:]]*=' "$spec_dir/lang/02-grammar.md"; then
    fail "02-grammar.md still defines the old struct declaration"
fi

[ ! -d "$spec_dir/provisional" ] ||
    fail "accepted language chapters must not remain under spec/provisional"
[ ! -d "$spec_dir/conformance/provisional" ] ||
    fail "accepted conformance fixtures must not remain provisional"

if grep -R -n -E '(^|[^[:alnum:]_])(v1|MVP|provisional)([^[:alnum:]_]|$)' \
    "$spec_dir" \
    "$repo_dir/future-work/OPEN_ISSUES.md" \
    "$repo_dir/guide/OVERVIEW.md" \
    "$repo_dir/guide/LANGUAGE_TOUR.md" \
    "$repo_dir/guide/LEARN_IN_10_MINUTES.md" \
    "$repo_dir/SYNTAX_NOTES.md" \
    --include='*.md'; then
    fail "versioned or provisional language labels found"
fi

if grep -n -E '^## (Open|Unresolved)|remain(s)? (open|unresolved)|not yet specified' \
    "$spec_dir"/lang/[0-9][0-9]-*.md "$spec_dir"/std/*.md "$spec_dir"/cli/*.md; then
    fail "specification chapter contains an unresolved design marker"
fi

tail -n +2 "$examples" | while IFS="$tab" read -r specification block classification fixtures; do
    [ -f "$spec_dir/$specification" ] ||
        fail "missing example specification $specification"

    case "$classification" in
        accept|mixed)
            printf '%s\n' "$fixtures" | tr '|' '\n' | while IFS= read -r fixture; do
                [ -f "$spec_dir/conformance/$fixture" ] ||
                    fail "missing example fixture $fixture for $specification block $block"
            done
            ;;
        lexical-inventory|type-relation|type-fragment|pattern-fragment|expression-fragment|filesystem-layout|illustrative-pseudocode)
            [ "$fixtures" = "-" ] ||
                fail "$classification entry must use '-' for $specification block $block"
            ;;
        *) fail "unknown example classification '$classification'" ;;
    esac
done

node --experimental-strip-types "$spec_dir/check-example-overlap.ts" "$spec_dir" "$examples"

# `let mut` is valid again as a mutability inference helper (OPEN_ISSUES,
# 2026-09-29); only the old `mut name:` parameter spelling is obsolete.
if grep -R -n -E 'fn [A-Za-z_][A-Za-z0-9_!]*\([^)]*mut [a-z_][A-Za-z0-9_]*:' \
    "$spec_dir" \
    "$repo_dir/guide/OVERVIEW.md" \
    "$repo_dir/guide/LANGUAGE_TOUR.md" \
    "$repo_dir/guide/LEARN_IN_10_MINUTES.md" \
    --include='*.md' --include='*.hd'; then
    fail "obsolete mutability syntax found"
fi

git -C "$repo_dir" diff --check
printf '%s\n' "spec check passed"
