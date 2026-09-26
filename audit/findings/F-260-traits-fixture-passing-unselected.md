# F-260: `typing/valid/traits.hd` passes but is not selected
Severity: note
Area: coverage
Evidence: audit/evidence/02-coverage/unselected.tsv row typing/valid/traits.hd (PASS under `hd check`)
Effect: A passing chapter-09 case is not protected by the portable gate; a regression would go unnoticed.
Recommendation: implementation change: add `typing/valid/traits.hd	type` to test/portable/cases.tsv.
