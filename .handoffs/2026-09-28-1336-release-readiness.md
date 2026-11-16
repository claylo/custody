# Handoff: Release readiness for 0.1.0

**Date:** 2026-09-28
**Branch:** main, clean and synced (`e5c6edc`)
**State:** CI green on macOS. One secret-name blocker stands between here and tagging 0.1.0.

## Where things stand (verified 13:36 ET)

- The repo moved from `receipts` to `custody` because of a crates.io name clash. A
  `custody` 0.0.1 name-claim stub is on crates.io, owned by claylo and index-verified.
  Trusted publishing points at claylo/custody, `cd.yml`, and environment `release`.
- CI (`.github/workflows/ci.yml`) runs on **macos-15** with `brew update && brew install
  just taplo cargo-nextest cargo-deny mupdf tesseract`. `just doctor` runs first, then
  `just ci`. First green run was 36442018890 (235/235 tests, mutool 1.28.5, tesseract
  5.5.3). Before 9/28, `just deny` failed first on every run, so **the tests had never run
  in CI.**
- Crate packaging: Cargo.toml drops `publish = false` and adds `homepage`, `readme`, and
  an anchored `include` (`/src/**`, `/README.md`, `/LICENSE-*`, `/custody.yaml`). The
  package holds 29 files, and `cargo publish --dry-run` builds from the tarball.
- Test fixtures and tests now use neutral surveying text instead of published-paper
  excerpts. Swaps kept the same length wherever byte offsets matter (the hand-built PDF in
  `tests/extract.rs` has a fixed `/Length 170`).
- `xtask/` (a port of bito's: completions, man, install) is a non-default workspace
  member, and `cargo xtask` works through `.cargo/config.toml`. It renders 9 man pages
  plus bash, zsh, fish, and PowerShell completions from librebar's augmented command
  tree. Canon `cd.yml` requires it.
- `gh coda setup` applied `.github/coda.yml`. Variables: PROJECT_NAME and
  CARGO_PUBLISH_PACKAGES = custody; CRATES_IO, HOMEBREW (tap claylo/homebrew-tap),
  GPG_SIGNING, and SBOM enabled; NPM, DEB, and RPM disabled. It also set the topics, and
  the `release` environment exists.
- `.github/formula.rb.tmpl`: `depends_on "mupdf"` and `"tesseract"`, installs the
  tarball's man pages and completions, and its test uses unescaped `#{bin}`.
- The baseline `.gitattributes` (fleet canon) and `.config/scrat.toml` are present.

## Decisions made

- **macOS + Homebrew for CI:** Ubuntu 24.04 apt ships MuPDF 1.23 and Tesseract 5.3, both
  outside the supported ranges. fleet.yaml exempts `ci.yml` from fleet-sync and records
  that reason.
- **No `taiki-e/install-action`** (fleet policy): its "latest" comes from a frozen
  manifest. Action pins come from actionista; check each SHA against GitHub.
- **Channels:** crates.io plus the brew tap. homebrew-core rejects self-submitted
  formulae below 225 stars (brew's audit uses 3× the 75-star threshold for authors). No
  npm, because no machine installs custody. No deb or rpm, because distro PDF packages are
  out of range.
- Handoffs stay committed (building in the open). They're only excluded from the crate
  package, via `include`.

## What's next

1. **Fix the GPG secret name (blocker).** `gh coda setup` installed `GPG_PRIVATE_KEY`,
   but canon `cd.yml` reads `secrets.GPG_RELEASE_KEY` (bito and colophon have that name).
   With `GPG_SIGNING_ENABLED=true`, "Sign release (GPG)" would decode an empty secret and
   `gpg --import` would fail, which fails `publish-binaries` and with it every publish
   job. Pick one: rename the 1Password item to `GPG_RELEASE_KEY` and run
   `gh coda set-secrets`, set the secret by hand from 1Password, or set
   `GPG_SIGNING_ENABLED=false` for 0.1.0. The value must be base64, since `cd.yml`
   pipes it through `base64 --decode`.
2. README "Install" still says `cargo install --path .`. Change it to
   `cargo install custody` and `brew install claylo/tap/custody` (which pulls in mupdf
   and tesseract).
3. Decide whether to rewrite the public history so the pre-9/28 fixture text drops out
   of old commits. The current tree and the crate tarball are clean either way. This is
   Clay's call.
4. Run `scrat ship --dry-run` and confirm it proposes 0.1.0 (Cargo.toml already says
   0.1.0; never bump by hand). Then `scrat ship`. The global `pre_ship` runs
   `ship-gate`, which checks `just check` plus CI green on main.

## Landmines

- **The MuPDF/Tesseract upper bounds** (`<1.29`, `<5.6`): brew has no pinned `mupdf@1.28`.
  When homebrew-core moves to 1.29, the CI `just doctor` step and every brew install fail
  together until the range widens. CI is the early warning.
- **librebar 0.7** (`feat/clispec-0-3`, unmerged since 8/17) turns `OutputField::new` into
  a typed `FieldType`, which will break `schema_metadata()` in `src/cli.rs`. Ship 0.1.0 on
  0.6; migrate later. The root package and xtask both pin `librebar = "=0.6.0"`. Canon's
  cli chunk expects `[workspace.dependencies]` inheritance so the two can't skew; custody
  hasn't adopted it yet.
- `cargo publish --dry-run` warns "ignoring test …" for each `tests/*.rs`. That's expected,
  since `include` leaves out tests.
- `.gitattributes` export-ignores `justfile`, which only affects `git archive`, not
  `cargo publish`.
