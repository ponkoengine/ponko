<!-- area: Guide and examples -->
### Added
- Maturity levels: every guide chapter opens with its level (proven, tested
  or experimental) and what it rests on (the reference game, the platforms
  and the graphics backends the list of checks covers), and the rustdoc of
  every top-level module repeats it. The guide's new "Maturity levels" page
  lists them all; `STABILITY.md` defines the levels and what each promises,
  and names the experimental items that will get `unstable_` names (#529).
  `python3 scripts/maturity.py` fails for a chapter or module without a
  level and regenerates the page.

### Changed
- The README's capability list and comparison table give each feature's
  maturity level instead of listing what is implemented.
