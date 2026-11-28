# Releasing hd

Releases are tagged versions only. There is no nightly channel.
During 0.x, any minor release may break compatibility. Pin an exact
`vX.Y.Z` for a stage0 compiler; `latest` is for consumers that accept updates.

## Cut a version

Start with a clean branch based on current main, with tags fetched, the
repository's Node and Rust toolchains, and an authenticated `gh` command.
Run:

```sh
node scripts/release.ts 0.0.4
```

The command requires a canonical `X.Y.Z` greater than the highest existing
version tag, which must be an ancestor of the branch. It updates the shared
Cargo version, exact workspace dependency pins and Cargo.lock. It prepends
CHANGELOG.md with commit subjects since the last tag, grouped by their
`Fixes #N` issues and current issue titles. The Breaking list comes from
retired rule IDs in the spec tool's inventories, including older spec
layouts. Ungrouped commits remain under Other changes.

It creates the local commit `Release vX.Y.Z` and annotated tag `vX.Y.Z`.
**It never pushes.** Review the manifest, lockfile and changelog before
pushing the commit to main through the normal review process. Once the
main Test workflow is green at that exact commit, a human pushes its tag:

```sh
git show v0.0.4
# Push only after reviewing the local release commit and main's Test result.
git push origin v0.0.4
```

Do not create a release tag on a different commit: the workflow checks the
tag, manifest and the latest main Test run for the same SHA. It fails fast
if that run is missing, unfinished or red; rerun Release after Test passes.
It does not poll CI or accept an older green run at another SHA.

## What the workflow does

`.github/workflows/release.yml` builds the shipped `dist` profile on five
native runners. The targets are Linux x86_64 and aarch64, macOS x86_64 and
aarch64, and Windows x86_64 MSVC. Each runner packs its binary and
`.node-version`, unpacks that archive, and runs `hd --version` and a
hello-world program on Wasmtime. No cross-compilation is used.

The release receives five archives named `hd-TARGET.tar.gz` (`.zip` on
Windows), plus SHA256SUMS. The publisher verifies each transferred archive
against its producing job's SHA-256 before assembling the combined file.
Notes give the commit and the exact version's CHANGELOG.md section.

For a pushed tag, the registry job first runs the package dry run, then
`scripts/publish-crates.ts --publish`. The **owner** provides the repository
secret `CARGO_REGISTRY_TOKEN`. Without it, the job emits a notice and skips
both registry steps. Binary releases still work. No token is needed for a
manual dry run.

The workflow header records runner-minute estimates and hard job budgets.
Build caches are target-specific; every archive still receives its own
native smoke test. GitHub's documented native runner labels are used:
[runner reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).

## Validate with a draft

After the workflow exists on main and main's Test is green, dispatch it:

```sh
gh workflow run release.yml --ref main
```

Manual dispatch takes the same five build, smoke-test, transfer and
checksum steps. It creates a draft named `dry-run-vVERSION-RUN_ID`, with
the source SHA in its notes. It never publishes a version or uploads to
crates.io. A rerun of the same workflow run replaces that draft's assets.
Drafts are not `latest` and are not a substitute for a human pushing a
reviewed version tag.

To try the release-cut command without publishing, use a local test branch,
run it with the next version, and inspect its commit, annotated tag and
CHANGELOG.md. Leave that test version local; do not push its tag.

## Recover a partial release

- A failed version-cut command before committing restores the files and
  index entries it changed. If committing succeeded but tagging failed,
  inspect the release commit and create its annotated tag locally; no
  remote state has changed.
- If main Test was not green yet, rerun Release after it passes at the tag's
  exact SHA.
- If one native build failed, fix the cause before releasing; do not drop a
  target. Rerunning all jobs refreshes the complete five-target artifact set.
- If asset upload stopped, rerun the publisher while all five build
  artifacts remain available, or rerun the whole workflow. It reuses the
  release and replaces assets by name, using GitHub CLI's documented
  [clobber behavior](https://cli.github.com/manual/gh_release_upload).
- If a registry upload stopped, rerun that job after fixing the cause.
  The publisher skips crate versions already on crates.io. It verifies
  packaged sources before uploading; no workspace checkout is published by
  accident. A first publication can wait for crates.io's new-crate rate limit.

Never move a published `vX.Y.Z` tag. A source fix gets a newer release.
Only the setup action's convenience `v1` tag moves as that action is updated.

## Consumers

After the first version release is published, the separate
[`HerringtonDarkholme/setup-hd`](https://github.com/HerringtonDarkholme/setup-hd)
composite action installs a verified prebuilt archive. Consumers choose
`latest` or an exact release tag, and can request Node from the archive's
`.node-version`. hd-skill's check-examples workflow is the first consumer.
Tool-only hd-lang jobs should use the action; compiler-under-test jobs
continue building the sources they test.
