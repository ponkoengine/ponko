function update(x, heading, speed, dt)
  if wall_ahead(x, heading) or not floor_ahead(x, heading) then heading = -heading end
  return x + heading * speed * dt, heading
end
