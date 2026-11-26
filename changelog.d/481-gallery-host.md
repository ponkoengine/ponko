### Changed
- The web gallery is published on GitHub Pages, by pushing the `gh-pages`
  branch by hand (the decision and the commands are in `web/gallery/README.md`
  and CONTRIBUTING.md, "Releasing"). The gallery's tooling and
  `scripts/release-check.py` read the project's URL from `PROJECT_URL` and
  derive the Pages address from it, and `scripts/project-url.py check` covers
  them.
