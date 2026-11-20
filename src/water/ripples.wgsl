// Contact ripples (the water kit's compute pass): the wave equation on a
// grid, one step per frame. Red: the height now; green: one step ago.
// custom[0]: the field's -x, -z corner, its side in metres, damping;
// custom[1] to [3]: contacts (x, z, radius, strength), radius 0 for none.

fn compute(c: Cell) -> vec4<f32> {
    let region = material.custom[0];
    let around = cell_at(c.pixel + vec2<i32>(1, 0)).x + cell_at(c.pixel - vec2<i32>(1, 0)).x
        + cell_at(c.pixel + vec2<i32>(0, 1)).x + cell_at(c.pixel - vec2<i32>(0, 1)).x;
    var next = (around * 0.5 - c.value.y) * region.w;
    // The border swallows what reaches it.
    let edge = min(min(c.pixel.x, c.pixel.y), min(c.size.x - 1 - c.pixel.x, c.size.y - 1 - c.pixel.y));
    if edge < 1 {
        next = 0.0;
    }
    let world = region.xy + c.uv * region.z;
    for (var i = 1; i < 4; i++) {
        let contact = material.custom[i];
        if contact.z > 0.0 {
            let d = distance(world, contact.xy) / contact.z;
            next -= contact.w * max(1.0 - d * d, 0.0);
        }
    }
    return vec4<f32>(next, c.value.x, 0.0, 1.0);
}
