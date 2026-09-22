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

for file in "$spec_dir"/*.md "$spec_dir"/provisional/*.md "$spec_dir"/conformance/*.md; do
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

    case "$phase" in
        parse|type|runtime|provisional) ;;
        *) fail "unknown phase '$phase' for $path" ;;
    esac

    case "$expectation" in
        accept|illustrative|reject:*|warn:*|panic:*) ;;
        *) fail "unknown expectation '$expectation' for $path" ;;
    esac

    spec_file=${section%%#*}
    [ -f "$spec_dir/$spec_file" ] || fail "missing specification $spec_file for $path"

    case "$expectation" in
        reject:*)
            code=${expectation#reject:}
            grep -Fq "# expect-error: $code" "$spec_dir/conformance/$path" ||
                fail "fixture $path does not declare expect-error $code"
            ;;
        warn:*)
            code=${expectation#warn:}
            grep -Fq "# expect-warning: $code" "$spec_dir/conformance/$path" ||
                fail "fixture $path does not declare expect-warning $code"
            ;;
        panic:*)
            code=${expectation#panic:}
            grep -Fq "# expect-panic: $code" "$spec_dir/conformance/$path" ||
                fail "fixture $path does not declare expect-panic $code"
            ;;
    esac
done

find "$spec_dir/conformance" -type f -name '*.hd' | sort | while IFS= read -r file; do
    relative=${file#"$spec_dir/conformance/"}
    count=$(awk -F "$tab" -v path="$relative" 'NR > 1 && $1 == path { count += 1 } END { print count + 0 }' "$manifest")
    [ "$count" -eq 1 ] || fail "fixture $relative has $count manifest entries"
done

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

for file in "$spec_dir"/0[1-9]-*.md "$spec_dir"/10-modules.md; do
    name=${file#"$spec_dir/"}
    blocks=$(awk '/^```text/{ count += 1 } END { print count + 0 }' "$file")
    indexed=$(awk -F "$tab" -v name="$name" 'NR > 1 && $1 == name { count += 1 } END { print count + 0 }' "$examples")
    [ "$blocks" -eq "$indexed" ] ||
        fail "$name has $blocks text examples but $indexed inventory entries"
done

grep -Fq '```ebnf' "$spec_dir/02-grammar.md" ||
    fail "02-grammar.md does not contain consolidated EBNF"

if grep -n -E '^## (Open|Unresolved)|remain(s)? (open|unresolved)|not yet specified' \
    "$spec_dir"/0[1-9]-*.md "$spec_dir"/10-modules.md; then
    fail "stable core chapter contains an unresolved design marker"
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
        lexical-inventory|type-relation|type-fragment|pattern-fragment|expression-fragment|filesystem-layout)
            [ "$fixtures" = "-" ] ||
                fail "$classification entry must use '-' for $specification block $block"
            ;;
        *) fail "unknown example classification '$classification'" ;;
    esac
done

if grep -R -n -E 'let[[:space:]]+mut([[:space:]]|$)|fn [A-Za-z_][A-Za-z0-9_!]*\([^)]*mut [a-z_][A-Za-z0-9_]*:' \
    "$spec_dir" \
    "$repo_dir/LANGUAGE_IDEA.md" \
    "$repo_dir/LANGUAGE_TOUR.md" \
    "$repo_dir/RUNTIME_AND_LIBRARY.md" \
    "$repo_dir/validation.hd" \
    --include='*.md' --include='*.hd'; then
    fail "obsolete mutability syntax found"
fi

git -C "$repo_dir" diff --check
printf '%s\n' "spec check passed"
