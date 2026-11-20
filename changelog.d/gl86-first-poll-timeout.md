### Fixed
- Message endpoints attempt the first hello on their first poll, even after a
  long startup. Polling caps the time charged toward peer silence so one
  stalled frame cannot consume the whole timeout.
