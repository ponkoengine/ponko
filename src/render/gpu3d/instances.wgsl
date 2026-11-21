// Culls a page's static batch views with stable compaction in four stages.
//
// `data` holds a pool's batches as vec4s: per batch a block of a header
// (instances, cells, the widest instance's reach for the probes' size cut),
// six vec4s per instance and two per cell (its box with its first instance
// and count). `indices` is a page: its root,
// this frame's jobs, and the visible instance list of each of its views.
// `args` holds the views' indirect draw arguments (five words per mesh
// part). `depth` holds what all pages share this frame: the occlusion
// grid's header and depths, and the views (frustum planes and eye).
// A job culls one batch for one view and distance level; its cells take
// `cells` consecutive workgroups starting at `first`.
//
//   header (depth[0..24]): the grid's width, height, camera distance, on,
//                          its view-projection, where the views start,
//                          where the probes start, how many, the size cut
//   root (indices[0..8]): jobs, table, cell workgroups, side, scan groups, 0...
//   job (12 words): block, visibility, arguments, parts,
//                   first, cells, view, flags, near, far (as f32 bits),
//                   first scan group, scan groups
//   view (28 floats from depth[depth[20] as bits]): six planes, (eye, 0)
//
// `block` indexes `data`; `visibility` indexes `indices` (the list starts
// one past it, then N local ranks, C cell offsets, ceil(C/64) group offsets);
// `arguments` indexes `args`.
// Flags: 1 = one mesh with a maximum distance to the instance's sphere,
// 2 = test against the depth grid, 4 = a face view of global illumination's
// probes: leave out instances under one of their pixels (see `probe_sized`),
// 8 = with 4, leave out whole cells too far from every probe for any of their
// instances to be a pixel wide (see `probe_near`), 16 = an instance cell's
// view drawn with its batch's other cells: its list holds indices into the
// whole of `data` (the block starts on a record, at `block / 6`) and its
// parts' draw arguments are `flags >> 8` draws apart (else one).
@group(0) @binding(0) var<storage,read> data:array<vec4<f32>>;
@group(0) @binding(1) var<storage,read_write> indices:array<u32>;
@group(0) @binding(2) var<storage,read_write> args:array<atomic<u32>>;
@group(0) @binding(3) var<storage,read> depth:array<f32>;
fn hidden(center:vec3<f32>, radius:f32) -> bool {
    if depth[3] == 0.0 {return false;}
    let vp=mat4x4<f32>(vec4<f32>(depth[4],depth[5],depth[6],depth[7]),vec4<f32>(depth[8],depth[9],depth[10],depth[11]),vec4<f32>(depth[12],depth[13],depth[14],depth[15]),vec4<f32>(depth[16],depth[17],depth[18],depth[19]));
    let extent=vec3<f32>(radius+depth[2]);
    // Reverse-Z: nearer is greater, the grid keeps each tile's smallest.
    var lo=vec2<f32>(1e30); var hi=vec2<f32>(-1e30); var nearest=-1e30;
    for(var i=0u;i<8u;i++) {
        let sign=vec3<f32>(select(-1.0,1.0,(i&1u)!=0u),select(-1.0,1.0,(i&2u)!=0u),select(-1.0,1.0,(i&4u)!=0u));
        let clip=vp*vec4<f32>(center+extent*sign,1.0);
        if clip.w <= 1e-5 {return false;}
        lo=min(lo,clip.xy/clip.w); hi=max(hi,clip.xy/clip.w); nearest=max(nearest,clip.z/clip.w);
    }
    if any(lo < vec2<f32>(-1.0)) || any(hi > vec2<f32>(1.0)) || nearest > 1.0 {return false;}
    let w=u32(depth[0]); let h=u32(depth[1]);
    let a=min(vec2<u32>((vec2<f32>(lo.x,-hi.y)*0.5+0.5)*vec2<f32>(f32(w),f32(h))),vec2<u32>(w-1u,h-1u));
    let b=min(vec2<u32>((vec2<f32>(hi.x,-lo.y)*0.5+0.5)*vec2<f32>(f32(w),f32(h))),vec2<u32>(w-1u,h-1u));
    for(var y=a.y;y<=b.y;y++) {for(var x=a.x;x<=b.x;x++) {if nearest >= depth[24u+y*w+x] {return false;}}}
    return true;
}
fn word(i:u32) -> vec4<f32> {return vec4<f32>(depth[i],depth[i+1u],depth[i+2u],depth[i+3u]);}
// Flag 4: whether the sphere is at least a pixel wide seen from one of
// this frame's probes (`radius >= cut * distance`), as the CPU path keeps
// an instance in one probe's own view. A face view sees for all of them.
fn probe_sized(center:vec3<f32>, radius:f32) -> bool {
    let first=bitcast<u32>(depth[21]); let count=bitcast<u32>(depth[22]);
    if count==0u {return true;}
    for(var k=0u;k<count;k++) {
        if radius >= depth[23]*distance(center,word(first+k*4u).xyz) {return true;}
    }
    return false;
}
// Flag 8: whether the batch's widest instance (`reach`), in this cell, can
// be a pixel wide seen from one of this frame's probes: from the box's
// nearest point. A little margin keeps rounding on the safe side.
fn probe_near(lo:vec3<f32>, hi:vec3<f32>, reach:f32) -> bool {
    let first=bitcast<u32>(depth[21]); let count=bitcast<u32>(depth[22]);
    if count==0u {return true;}
    for(var k=0u;k<count;k++) {
        let p=word(first+k*4u).xyz;
        let near=max(max(lo-p,p-hi),vec3<f32>(0.0));
        if reach >= depth[23]*0.999*length(near) {return true;}
    }
    return false;
}
// `view`: the index in `depth` of the view's first plane; `far`: the
// maximum distance to the sphere (flag 1 only).
fn visible(center:vec3<f32>, radius:f32, view:u32, flags:u32, far:f32) -> bool {
    if (flags & 1u) != 0u && distance(center,word(view+24u).xyz)-radius > far {return false;}
    for (var i=0u;i<6u;i++) {let p=word(view+i*4u); if dot(p.xyz,center)+p.w < -radius {return false;}}
    return (flags & 2u) == 0u || !hidden(center,radius);
}
// Each cell writes local ranks, groups prefix cell counts, jobs prefix group
// totals, then cells scatter. The final list follows source order, independent of scheduling.
fn job_at(slot:u32, column:u32) -> u32 {
    let table=indices[1];
    var lo=0u; var hi=indices[0];
    while hi-lo > 1u {
        let mid=(lo+hi)/2u;
        if indices[table+mid*12u+column] <= slot {lo=mid;} else {hi=mid;}
    }
    return table+lo*12u;
}
fn job_for(slot:u32) -> u32 {return job_at(slot,4u);}
var<workgroup> sums:array<u32,64>;
@compute @workgroup_size(64)
fn main(@builtin(workgroup_id) group:vec3<u32>, @builtin(local_invocation_index) lane:u32) {
    let side=indices[3];
    let slot=group.x+(group.y+group.z*side)*side;
    var job=0u; var cell=0u; var block=0u; var list=0u;
    var start=0u; var count=0u; var total=0u; var cells=0u;
    var view=0u; var flags=0u; var band=vec2<f32>(0.0); var eye=vec3<f32>(0.0);
    var eligible=false;
    let valid=slot < indices[2];
    if valid {
        job=job_for(slot); block=indices[job]; list=indices[job+1u];
        cell=slot-indices[job+4u]; cells=indices[job+5u];
        total=u32(data[block].x);
        view=bitcast<u32>(depth[20])+indices[job+6u]*28u;
        flags=indices[job+7u];
        band=vec2<f32>(bitcast<f32>(indices[job+8u]),bitcast<f32>(indices[job+9u]));
        eye=word(view+24u).xyz;
        let base=block+1u+total*6u+cell*2u;
        let cell_lo=data[base]; let cell_hi=data[base+1u];
        start=bitcast<u32>(cell_lo.w); count=bitcast<u32>(cell_hi.w);
        eligible=visible((cell_lo.xyz+cell_hi.xyz)*0.5,length(cell_hi.xyz-cell_lo.xyz)*0.5,view,flags,band.y);
        eligible=eligible && ((flags & 8u) == 0u || probe_near(cell_lo.xyz,cell_hi.xyz,data[block].z));
        if (flags & 1u) == 0u {
            let near=max(max(cell_lo.xyz-eye,eye-cell_hi.xyz),vec3<f32>(0.0));
            let far=max(abs(cell_lo.xyz-eye),abs(cell_hi.xyz-eye));
            eligible=eligible && dot(near,near) < band.y*band.y && dot(far,far) >= band.x*band.x;
        }
    }
    var ranks:array<u32,4>;
    var found=0u;
    for(var k=0u;k<4u;k++) {
        let local=lane*4u+k;
        var yes=false;
        if eligible && local<count {
            let i=start+local;
            let sphere=data[block+1u+i*6u+5u];
            yes=visible(sphere.xyz,sphere.w,view,flags,band.y);
            yes=yes && ((flags & 4u) == 0u || probe_sized(sphere.xyz,sphere.w));
            if (flags & 1u) == 0u {
                let d=distance(data[block+1u+i*6u+3u].xyz,eye);
                yes=yes && d>=band.x && d<band.y;
            }
        }
        ranks[k]=select(0xffffffffu,found,yes);
        found+=select(0u,1u,yes);
    }
    sums[lane]=found;
    workgroupBarrier();
    if lane==0u {
        var offset=0u;
        for(var k=0u;k<64u;k++) {
            let count=sums[k]; sums[k]=offset; offset+=count;
        }
        if valid {indices[list+1u+2u*total+cell]=offset;}
    }
    workgroupBarrier();
    if valid {
        for(var k=0u;k<4u;k++) {
            let local=lane*4u+k;
            if local<count {
                let rank=ranks[k];
                indices[list+1u+total+start+local]=select(0xffffffffu,sums[lane]+rank,rank!=0xffffffffu);
            }
        }
    }
}
// Prefix groups of 64 cell counts independently. Keeping the serial work
// bounded avoids a long single-lane scan of a million-instance job.
@compute @workgroup_size(1)
fn scan(@builtin(workgroup_id) group:vec3<u32>) {
    let side=indices[3]; let slot=group.x+(group.y+group.z*side)*side;
    if slot>=indices[4] {return;}
    let job=job_at(slot,10u); let chunk=slot-indices[job+10u];
    let block=indices[job]; let list=indices[job+1u];
    let total=u32(data[block].x); let cells=indices[job+5u];
    var found=0u;
    for(var cell=chunk*64u;cell<min((chunk+1u)*64u,cells);cell++) {
        let at=list+1u+2u*total+cell;
        let count=indices[at]; indices[at]=found; found+=count;
    }
    indices[list+1u+2u*total+cells+chunk]=found;
}
// One lane per job: prefix the chunk totals, not the population.
@compute @workgroup_size(64)
fn prefix(@builtin(global_invocation_id) id:vec3<u32>) {
    if id.x >= indices[0] {return;}
    let job=indices[1]+id.x*12u;
    let block=indices[job]; let list=indices[job+1u];
    let total=u32(data[block].x); let cells=indices[job+5u];
    var found=0u;
    for(var chunk=0u;chunk<indices[job+11u];chunk++) {
        let at=list+1u+2u*total+cells+chunk;
        let count=indices[at]; indices[at]=found; found+=count;
    }
    let arguments=indices[job+2u]; let parts=indices[job+3u];
    let stride=max(indices[job+7u]>>8u,1u)*5u;
    for(var part=0u;part<parts;part++) {atomicStore(&args[arguments+part*stride+1u],found);}
}
@compute @workgroup_size(64)
fn scatter(@builtin(workgroup_id) group:vec3<u32>, @builtin(local_invocation_index) lane:u32) {
    let side=indices[3]; let slot=group.x+(group.y+group.z*side)*side;
    if slot >= indices[2] {return;}
    let job=job_for(slot); let block=indices[job]; let list=indices[job+1u];
    let cell=slot-indices[job+4u]; let total=u32(data[block].x);
    let base=block+1u+total*6u+cell*2u;
    let start=bitcast<u32>(data[base].w); let count=bitcast<u32>(data[base+1u].w);
    let cells=indices[job+5u];
    let offset=indices[list+1u+2u*total+cell]+indices[list+1u+2u*total+cells+cell/64u];
    let first=select(0u,block/6u,(indices[job+7u]&16u)!=0u);
    for(var i=start+lane;i<start+count;i+=64u) {
        let rank=indices[list+1u+total+i];
        if rank!=0xffffffffu {indices[list+1u+offset+rank]=first+i;}
    }
}
