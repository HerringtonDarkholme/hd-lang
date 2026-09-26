# F-603: Initialization check misses generic-bound and dynamic trait dispatch
Severity: major
Area: correctness
Evidence: audit/evidence/06-compiler/module-initialization.md
  (`hd check` and `hd run audit/probes/compiler/init-dispatch-miss.hd`)
Effect: `bounded := via_bound(Box {...})` and `dynamic := dynamic_value.read()`
  run before `later := 5`, and `Reader.read` returns `later`. The program is
  accepted and `hd run` prints 5. It should print 555 or be rejected. Both
  bindings silently read the zero-initialized global. Static calls and
  interpolation of the same impl are rejected with
  `top-level-read-before-initialization`.
  spec/10-modules.md:228-233 requires "functions reached by trait dispatch"
  in the read set. The visitor follows only `call`, `suspend-construct`,
  `function-value`, and `closure`, not `trait-call` or dictionaries.
Recommendation: implementation change. Treat every implementation reachable
  through a bound or dynamic dictionary as a callee, conservatively.
