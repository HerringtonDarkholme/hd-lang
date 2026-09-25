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

for file in "$spec_dir"/*.md "$spec_dir"/conformance/*.md; do
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
        parse|type|runtime) ;;
        *) fail "unknown phase '$phase' for $path" ;;
    esac

    case "$expectation" in
        accept|reject:*|warn:*|panic:*) ;;
        *) fail "unknown expectation '$expectation' for $path" ;;
    esac

    spec_file=${section%%#*}
    [ -f "$spec_dir/$spec_file" ] || fail "missing specification $spec_file for $path"

    case "$expectation" in
        reject:*)
            code=${expectation#reject:}
            if [ "$(sed -n '1p' "$spec_dir/conformance/$path")" != "# expect-error: $code" ]; then
                grep -Fq "# diagnostic: $code" "$spec_dir/conformance/$path" ||
                    fail "fixture $path does not declare diagnostic $code"
            fi
            grep -Fq "\`$code\`" "$spec_dir/README.md" ||
                fail "fixture $path uses unknown error category $code"
            ;;
        warn:*)
            code=${expectation#warn:}
            if [ "$(sed -n '1p' "$spec_dir/conformance/$path")" != "# expect-warning: $code" ]; then
                grep -Fq "# warning: $code" "$spec_dir/conformance/$path" ||
                    fail "fixture $path does not declare warning $code"
            fi
            grep -Fq "\`$code\`" "$spec_dir/README.md" ||
                fail "fixture $path uses unknown warning category $code"
            ;;
        panic:*)
            code=${expectation#panic:}
            if [ "$(sed -n '1p' "$spec_dir/conformance/$path")" != "# expect-panic: $code" ]; then
                grep -Fq "# panic: $code" "$spec_dir/conformance/$path" ||
                    fail "fixture $path does not declare panic $code"
            fi
            grep -Fq "\`$code\`" "$spec_dir/06-control-flow.md" ||
                fail "fixture $path uses unknown panic category $code"
            ;;
    esac
done

find "$spec_dir/conformance" -type f -name '*.hd' | sort | while IFS= read -r file; do
    relative=${file#"$spec_dir/conformance/"}
    count=$(awk -F "$tab" -v path="$relative" 'NR > 1 && $1 == path { count += 1 } END { print count + 0 }' "$manifest")
    [ "$count" -eq 1 ] || fail "fixture $relative has $count manifest entries"
done

python3 "$spec_dir/reference_parser.py" "$manifest" "$spec_dir/conformance"
python3 "$spec_dir/check_spec_anchors.py" "$spec_dir" "$manifest"

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

for file in "$spec_dir"/[0-9][0-9]-*.md; do
    name=${file#"$spec_dir/"}
    blocks=$(awk '/^```text/{ count += 1 } END { print count + 0 }' "$file")
    indexed=$(awk -F "$tab" -v name="$name" 'NR > 1 && $1 == name { count += 1 } END { print count + 0 }' "$examples")
    [ "$blocks" -eq "$indexed" ] ||
        fail "$name has $blocks text examples but $indexed inventory entries"
done

grep -Fq '```ebnf' "$spec_dir/02-grammar.md" ||
    fail "02-grammar.md does not contain consolidated EBNF"

for production in data_decl use_decl requirement_clause context_scope \
    annotation_decl enum_variant generic_parameter annotation_runtime_access; do
    grep -Eq "^${production}[[:space:]]*=" "$spec_dir/02-grammar.md" ||
        fail "02-grammar.md is missing $production"
done

if grep -Eq '^struct_decl[[:space:]]*=' "$spec_dir/02-grammar.md"; then
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
    "$repo_dir/future-work/RUNTIME_AND_LIBRARY.md" \
    "$repo_dir/SYNTAX_NOTES.md" \
    --include='*.md'; then
    fail "versioned or provisional language labels found"
fi

if grep -n -E '^## (Open|Unresolved)|remain(s)? (open|unresolved)|not yet specified' \
    "$spec_dir"/[0-9][0-9]-*.md; then
    fail "numbered specification chapter contains an unresolved design marker"
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

python3 "$spec_dir/check_example_overlap.py" "$spec_dir" "$examples"

if grep -R -n -E 'let[[:space:]]+mut([[:space:]]|$)|fn [A-Za-z_][A-Za-z0-9_!]*\([^)]*mut [a-z_][A-Za-z0-9_]*:' \
    "$spec_dir" \
    "$repo_dir/guide/OVERVIEW.md" \
    "$repo_dir/guide/LANGUAGE_TOUR.md" \
    "$repo_dir/future-work/RUNTIME_AND_LIBRARY.md" \
    --include='*.md' --include='*.hd'; then
    fail "obsolete mutability syntax found"
fi

git -C "$repo_dir" diff --check
printf '%s\n' "spec check passed"
