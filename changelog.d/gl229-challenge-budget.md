### Security
- A UDP message host's cap on challenges and version refusals is no
  longer one budget shared by every stranger: each network (IPv4 /24,
  IPv6 /56) gets 16 answers a second of its own, under the global 256, so
  a flood from one network no longer delays joins from others. The
  per-network budgets are a fixed table of 64 entries inside the host
  (no allocation, nothing kept longer than a second); while it is full, a
  new network has the global budget only. Always on; C and C++ need no
  new call (book/src/security.md, "The challenge budget").
- A host still sends nothing to a client of message protocol 1: such a
  client could only report a refusal as "host is full", which would be
  wrong, so its join times out (book/src/networking.md).
