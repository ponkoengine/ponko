### Fixed
- A failed asset group goes on taking the assets it had started before it
  failed, as they finish, and counts them in its progress; `finish_uploads`
  waits for them. Before, a failed group's `loaded` stopped where the failure
  came back, so it could be short of what had in fact finished, and differed
  from run to run under load. The C API test of a restarted group checks the
  exact count again.
