# Joins impl-lines.tsv (first file) with markers-raw.tsv (second file) into markers.tsv rows.
# Usage: awk -F'\t' -f join.awk impl-lines.tsv markers-raw.tsv
BEGIN { OFS = "\t" }
/^#/ { next }
FNR == NR { if ($1 != "path") { sel[$1] = $2; iv[$1] = $8; il[$1] = $7 }; next }
$1 == "path" {
  print "path", "status", "commits", "phase", "expectation", "section", "old_header_code", "marker_code", "marker_line", "impl_lines", "selected", "code_check", "line_check", "body_changed", "verdict", "note"
  next
}
{
  p = $1
  code_ok = ($12 ~ /^true/ && ($13 == "true" || $13 == "n/a")) ? "code-consistent" : "CODE-MISMATCH"
  if ($9 == "-") line = "n/a(accept)"
  else if ($4 == "runtime") line = "unchecked(panic output has no location)"
  else if (iv[p] == "impl-line-matches") line = "matches-impl-span"
  else if (iv[p] == "impl-code-absent") line = "unverified(impl lacks code)"
  else line = iv[p]
  note = ""
  if (p == "typing/valid/requirements-and-suspension.hd") note = "body fix: base version was spec-invalid (readonly Suspend driven; data UserId called positionally); rerun: audit/probes/spec-edits/base-requirements-and-suspension.hd"
  if (p == "runtime/valid/cancellation-runs-defer.hd") note = "body fix: trait Gate made pub; base version leaked a private trait in the pub main row (private-type-leak); rerun: audit/probes/spec-edits/base-cancellation-runs-defer.hd"
  if (p == "typing/invalid/pack-length-mismatch.hd") note = "line is implementation-specific: call spans L8-L12, marker on the extra argument L11"
  if (p == "typing/warnings/requirement-subtract-absent.hd") note = "line choice: call site L17, not the subtracting declaration L10; spec gives no location"
  if (p == "typing/invalid/missing-return-value.hd") note = "line choice: marker on the if header; spec gives no location"
  body = $14
  if (body == "yes" && p != "typing/valid/requirements-and-suspension.hd" && p != "runtime/valid/cancellation-runs-defer.hd") body = "whitespace-only"
  v = (code_ok == "code-consistent") ? "no-weakening" : "FLAG"
  if ($2 == "A") v = v "; new fixture, cited section checked by hand"
  print p, $2, $3, $4, $5, $6, $7, $9, $10, (il[p] == "" ? "-" : il[p]), sel[p], code_ok, line, body, v, note
}
