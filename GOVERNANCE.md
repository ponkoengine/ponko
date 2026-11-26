# Governance

**Status: draft.** This describes how Ponko is run today and what it intends to
become. It is written for people deciding whether to build on the engine or
contribute to it. Where something still depends on one person, it says so (see
[Continuity](#continuity) and the tracker issue
[#514](https://github.com/ponkoengine/ponko/issues/514)).

Roles, not names: "the maintainers" are the people with write access to the
repository who review and merge pull requests. "Organization owners" are the
people who administer the `ponkoengine` GitHub organization, and who own the
published crates and the domain.

## How decisions are made

- **The maintainers decide** what goes in, what is declined and how the engine's
  API should look. They aim for agreement; when they disagree, the decision
  is made by the organization owners after hearing the arguments in the open.
- **Decisions are recorded where people can find them**: in the issue or pull
  request that raised them, with the reason. A decision about the whole project,
  such as dropping a platform or changing a promise in
  [STABILITY.md](STABILITY.md), is recorded in an issue and linked from the
  document it changes.
- **Release decisions** (what is in a release, when it is cut, whether a
  candidate is good enough) are recorded in the release tracker issue, for
  0.2.0 [#528](https://github.com/ponkoengine/ponko/issues/528). The release
  scope is defined by its issues and milestone; a date is a target.
- **Talk happens in the open.** [GitHub Discussions](https://github.com/ponkoengine/ponko/discussions)
  is the place for questions, ideas and showing what you made; there is no
  separate chat room for now. Maintainers answer new issues and discussions
  within a week. An idea that gets agreement in a discussion becomes an issue,
  and the issue is the decision record.
- **Disagreeing with a decision**: say so on the issue, with evidence (a
  measurement, a reproduction, a use case). Evidence changes decisions; volume
  does not. The [code of conduct](CODE_OF_CONDUCT.md) applies everywhere.

## Who can merge

Only maintainers merge, through a pull request into `main`; nobody pushes to
`main` directly, and history stays linear ([AGENTS.md](AGENTS.md#git-workflow-required)).
A maintainer does not merge a change they could not explain and test. There is
no CI, so the checks listed in [CONTRIBUTING.md](CONTRIBUTING.md#local-checks)
are run locally before merging and the pull request says which ran.

Contributions made with the help of coding agents are reviewed like any other.
The author, a person, answers for the change and for what the agent did on their
behalf ([Using coding agents](CONTRIBUTING.md#using-coding-agents)).

## How someone becomes a maintainer

1. Several accepted pull requests over some months, including at least one that
   changed behavior and one review of someone else's change that found a real
   problem. The area does not matter: docs, tests, one subsystem or tooling all
   count.
2. A maintainer proposes them in a discussion or an issue, with links to that
   work. The existing maintainers agree there, in the open, and the organization
   owners record the result.
3. The organization owners add them to the repository with write access. A
   maintainer who will publish releases also becomes an owner of the crates on
   crates.io ([One-time setup](CONTRIBUTING.md#one-time-setup-a-maintainer-in-settings)).
4. A maintainer who is away for a long time may be set back to contributor, and
   come back the same way they came in: by asking.

Becoming an organization owner is a separate step, for a maintainer who has done
a release from the written steps and whom the existing owners trust with the
accounts, the crates and the domain.

## Continuity

The project must not stop because one person is away for a month. The honest
state, and what is planned:

- **Release steps are written down.** Everything needed to cut a release from a
  clean checkout is in [CONTRIBUTING.md, "Releasing"](CONTRIBUTING.md#releasing):
  the order of the steps, the commands and what to check. A second
  person doing one release from those steps alone, without asking, is the
  test that they are complete.
- **A second owner is needed** of the GitHub organization and of every published
  crate (`cargo owner --add`). Until then, the repository, the crates and
  the release steps depend on one account. This is the biggest open risk.
- **Access to the domain and the website** is written down privately, where a
  second owner can find it, not in this repository.
- **Security reports** reach the repository's administrators through GitHub's
  private vulnerability reporting ([SECURITY.md](SECURITY.md)). The aim is that
  they reach at least two people.
- **If the only maintainer is unavailable**: the repository stays readable and
  forkable (it is MIT or Apache-2.0 licensed), the last release stays on
  crates.io, and anyone may fork and publish under another name. When a second
  owner exists, they take over merging and releasing using the written steps,
  and say so in the release tracker.

What still needs people is listed in
[#514](https://github.com/ponkoengine/ponko/issues/514).

## Changing this document

A pull request, discussed like any other change. The maintainers agree on it,
and the organization owners record that they did.
