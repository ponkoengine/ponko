fn update(x, heading, speed, dt) {
    if wall_ahead(x, heading) || !floor_ahead(x, heading) { heading = -heading; }
    [x + heading * speed * dt, heading]
}
