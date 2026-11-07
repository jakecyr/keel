# Reviewing and releasing Keel

Keel remains experimental. Passing CI does not satisfy the independent review,
fuzzing, compatibility, and other gates in [release gaps](release-gaps.md).

## Pull requests and merge policy

Use squash merges with the PR title as the commit title. Required checks are
`conventional-title` and all four `verify` matrix jobs. Do not merge with failing
or pending checks. Pages builds are additionally checked when website files change.

Titles follow Conventional Commits: `fix(cli): explain missing compiler`,
`feat(project): add scaffolding`, or `feat!: change project schema`. `fix` triggers
a patch; `feat` triggers a minor release; `!` records a breaking change (a minor while pre-1.0). Docs, tests, and
maintenance changes alone do not normally trigger a release. Explain breaking
changes and migration in the PR body.

## One-time automation setup

A repository administrator must configure `RELEASE_PLEASE_TOKEN`, a narrowly
scoped token with repository Contents, Issues, and Pull requests write access.
Do not commit credentials. Authentication is not changed by this setup.
The default Actions token cannot trigger downstream PR/tag workflows; see the
[Release Please credential documentation](https://github.com/googleapis/release-please-action#github-credentials).
Without this secret the preparation workflow fails explicitly; manual versioned
tags remain supported. Permit Actions to create pull requests in repository settings.

## Release flow

1. Conventional squash commits on master update an automated release PR with
   Cargo version/lockfile changes and a changelog. Review the version and notes.
2. Merge the release PR only after all required checks pass. Release Please
   creates a version tag and draft release. The credential above lets that tag
   trigger the existing four-platform verification and packaging workflow.
3. Only after every platform passes, CI uploads SHA-256-checked archives and
   publishes the draft. The installer then sees a complete release. Post-publication
   CI tests downloading, installing, and building a fresh project on all four platforms.
4. Inspect those installation checks before announcing the release. If they fail,
   mark the release as a prerelease and investigate; do not claim successful validation.

The installer supports macOS and Linux, ARM64 and x86_64. It verifies checksums,
uses an isolated download, and replaces an existing binary atomically. A C compiler
is still required. Checksums are integrity checks, not independent signatures.
Windows, signed provenance, sustained external validation, and production assurance
remain open work. No release is published as part of a repository review.
