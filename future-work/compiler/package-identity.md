# Package Identity In The Driver (M14, 2026-10-10, #279)

Two packages with one manifest name collide with `duplicate-module-name`
today: two compatibility lines of one host path (`json` + `json_v1`,
`module.dep.two-lines`) and a fetched version beside a same-named
workspace member (`module.workspace.fetched-member.two`). Identity
information already exists upstream (host path + line, canonical dir,
fetched key) but is dropped at the driver boundary, where everything is
name-only. This designs keying packages by identity instead. No code
changes.

## Identity

A package's identity is what already distinguishes the two cases, and
what the spec already names (`module.dep.identity`): for a fetched
package, host path + compatibility line (`HostPath` + `line`,
`manifest.rs`, `select.rs:line_key`); for a local/workspace member, its
canonical directory. `manifest.rs:identity()` (`{host}@{line}` or
`path {dir}`) is the display form; `hd_deps` `line_key` /
`tree_key(HOST@VERSION)` is the key form. What changes: `PackageIn`,
`Dependency`, and `ModuleTable.packages` carry `(name, identity)` where
they carry `name` today (`hd_project/src/lib.rs`, `hd_driver/src/lib.rs:
496-524`); `disk.rs` stops discarding the `fetched` key when re-reading
sources (`disk.rs:278-346` is collision point (a)).

## Module paths

`module_path(package, file)` starts from `package_ident(name)` today, so
same-named packages share every `ident.below` string and collide in the
global `by_path` map (`hd_project/src/lib.rs:530-552`). With identity,
the path root becomes identity-derived: fetched packages keep a
human-readable root with the line disambiguated (`json` vs `json_v1` —
i.e. the manifest *key*, not the manifest *name*, which is exactly what
`two-lines` already requires: two deps with two keys); same-key
collisions (member vs fetched, case (b)) get a suffix from the identity
(`pkg` vs `pkg@<shorthash>` or `pkg+fetched`). `dep.KEY` resolution
(`absolute_with`, first match wins today) then resolves by (KEY,
identity): each requiring package's `requires` edge already knows which
selection index it points at (`disk.rs:edges`), so lookup becomes exact
instead of first-match. Display paths (`<pkg>/{path}`) use the same
disambiguated root, so diagnostics stay readable.

## Cache keys

`package_key(name)` (`hd_cache`) becomes `package_key(identity)`; every
key built on it (`iface`, `hdr`, `check`, `root_package_key`,
`prog_key` via entry paths) separates automatically. The latent
`check_key` anomaly (driver passes the root key for every module) must
be fixed in the same change, or same-named dependency modules share
check entries. `parse`/`init`/`prog` keys, which rely on module-path
strings, separate via the disambiguated roots. Key hygiene is unchanged
(content-derived identities only: host path, line, canonical dir —
never mtimes or checkout paths beyond canonicalization).

## Diagnostics that name a package

Selection problems already name `HOST@VERSION` (`select.rs`, `fetch.rs`);
keep that form and extend it to the driver: the cycle diagnostic
(`packages depend on each other`, names only today) prints
`name@identity` when two names in the cycle are equal, and the
`duplicate-module-name` message — which must become unreachable for
distinct identities — otherwise quotes the disambiguated roots. `dep.KEY`
stays the import root spelling (`dep.json`, `dep.json_v1`); KEYs are
already distinct in case (a) by `one-key-per-line`, and in case (b) the
member is reached by path/name while the fetched copy is reached only
through its requirer's KEY edge.

## Price

Small-medium (one task, M): the identity type plus threading it through
`PackageIn`/`Dependency`/`ModuleTable`/keys/diagnostics; no algorithm
changes (selection already distinguishes; resolution already edges).
Risk is churn, not design: module-path strings appear in diagnostics,
file display, `use` roots, and folder strings, all keyed off one
function (`module_path`/`package_ident`), so the change is wide but
mechanical. Do it together with the `check_key` root-key fix and a
conformance probe per colliding pair (`two-lines`, `fetched-member.two`
fixtures exist as spec targets).

## Questions with a recommendation

- **Q1. KEY vs NAME as the human root?** KEY (recommended): `two-lines`
  already mandates distinct KEYs; NAME is what collides.
- **Q2. Suffix form for same-KEY collisions?** Short identity hash
  (recommended): stable, content-derived, no new naming rules.
- **Q3. Fix the `check_key` root-key anomaly in the same change?**
  Yes (recommended): otherwise the new keys still collide where it
  matters most.
- **Q4. Canonical dir vs `path {dir}` display for locals?** Keep
  manifest name for unique names (recommended): identity surfaces only
  where it disambiguates, so single-package output is byte-identical.

Recommendation: implement identity-keyed packages with KEY-derived
roots as one task, with the two spec-target fixtures as the acceptance
probes.
