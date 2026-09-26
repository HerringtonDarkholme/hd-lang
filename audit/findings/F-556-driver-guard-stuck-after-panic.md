# F-556: after a panic inside a drive, later drives report `suspension-competing-driver`
Severity: note
Area: runtime
Evidence: audit/evidence/05-requirements/susp-probes.md "Driver guard" (instance D rows), audit/probes/arch/suspension/guard.hd
Effect: once any panic escapes a drive, `$hd.driver-active` stays 1. Every later suspending entry on that instance fails with `suspension-competing-driver`. Non-suspending exports keep running. A host that ignores the spec's "discard the poisoned instance" rule gets a misleading panic code, not a poisoned-instance error.
Recommendation: OPEN_ISSUES question: should the runtime enforce poisoning, for example with a `$hd.poisoned` global checked by every export, or is discarding purely a host obligation?
