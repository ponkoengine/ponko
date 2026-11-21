### Added

- `Water::contacts_started`: how many contact ripples have started on a water body since it was made, which the water's update doesn't drain (unlike `Water::waiting_contacts`). C and C++: `ponko_ctx_water_started_contacts`, `ctx.water_started_contacts`, so a test reads it once after the physics step instead of keeping the most of the waiting count over every tick.
