### Changed

- The project's home is GitHub again: `https://github.com/ponkoengine/ponko`
  (issues, pull requests, releases). The crates' `repository` fields, the
  READMEs, the guide, the templates and the web gallery link there, and
  `scripts/project-url.py check` now refuses links to the old GitLab home.
- There is no CI any more: nothing runs on a pull request or on `main` by
  itself. Contributors and maintainers run the checks locally. The list of
  checks moved from `.github/workflows/ci.yml` to `scripts/checks.yml`, a
  document no service runs, which `scripts/check-feature-coverage.py` still
  reads. The leak check runs as the local commit hook
  (`scripts/leak-check.sh install`), and a release is built and published by a
  maintainer from the tag (CONTRIBUTING.md, "Releasing", lists the steps).
- `scripts/semver-checks.py run` works on a local checkout: `--base BRANCH`
  names the branch the change is for, `--breaking` says the pull request has
  the `breaking` label.

### Removed

- The GitHub Actions workflows (checks, benchmarks and their comments, the
  dependency audit, fuzzing, the release, the release audit, pull request
  events), the shared actions and the Dependabot configuration.
- Scripts that only worked inside Actions: `scripts/bench-history-push.sh`,
  `scripts/audit-issue.py`, `scripts/ci-apt-install.sh`,
  `scripts/ci-retry.sh`, the `ci` command of `scripts/release-check.py` and
  the `--review` label gate of `scripts/check-reference-images.py`.
