### Changed
- Replication resends a record at once when the client acknowledges
  packets sent after the one that carried it but not that one, instead of
  waiting for the resend timeout. On a link that reorders packets, the
  server learns how late packets arrive and waits that long before calling
  one lost. With five changes in a lost packet on a link of 8.6 ticks round
  trip, they went again after 9 ticks instead of 12; on a link losing 10%
  each way, the slowest 5% of changes reached the client in 18.2 ticks on
  average instead of 20.0, for the same bytes.
- While a client acknowledges nothing, each replication resend timeout
  doubles the next one, up to 4 s; the first acknowledgement brings it
  back and everything overdue goes at once. Over a link cut for 9 s, the server
  sent 35 records instead of 300 for five changes.
