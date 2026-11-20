
// The water's vertices ride the waves (the water kit's `vertex` hook).
fn vertex(v: Vertex) -> Vertex {
    var out = v;
    let moved = water_displace(v.position.xz, frame.time.x);
    out.position = vec3<f32>(moved.x, v.position.y + moved.y, moved.z);
    // The rest point, for the surface's normal and crest foam.
    out.uv1 = v.position.xz;
    out.normal = vec3<f32>(0.0, 1.0, 0.0);
    return out;
}

