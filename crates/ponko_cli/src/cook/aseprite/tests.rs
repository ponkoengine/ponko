//! Tests of Aseprite cooking, on `.aseprite` files written here (the
//! format is Aseprite's documented `ase-file-specs.md`).

use super::*;

/// A layer of a file [`write`] makes.
pub struct Layer {
    pub name: &'static str,
    pub visible: bool,
    pub group: bool,
    pub level: u16,
    pub opacity: u8,
    /// Aseprite's blend mode number (0 normal, 1 multiply, ...).
    pub blend: u16,
}

impl Layer {
    pub fn normal(name: &'static str) -> Layer {
        Layer {
            name,
            visible: true,
            group: false,
            level: 0,
            opacity: 255,
            blend: 0,
        }
    }
}

/// A cel: a layer's pixels in one frame, or a link to another frame's.
pub enum Cel {
    Image {
        layer: u16,
        x: i16,
        y: i16,
        opacity: u8,
        w: u16,
        h: u16,
        rgba: Vec<u8>,
    },
    /// Aseprite writes a linked cel at its frame's cel's position.
    Linked { layer: u16, x: i16, y: i16, frame: u16 },
}

/// A solid `w` x `h` cel of `color` at (x, y).
pub fn solid(layer: u16, (x, y): (i16, i16), (w, h): (u16, u16), color: [u8; 4]) -> Cel {
    Cel::Image {
        layer,
        x,
        y,
        opacity: 255,
        w,
        h,
        rgba: color.repeat(usize::from(w) * usize::from(h)),
    }
}

/// A tag: frames, direction (0 forward, 1 reverse, 2 ping-pong, 3
/// ping-pong reverse), repeat count (0: forever) and name.
pub struct Tag(pub u16, pub u16, pub u8, pub u16, pub &'static str);

/// A slice with one key on frame 0: bounds, an optional nine-slice center
/// and pivot.
pub struct Slice {
    pub name: &'static str,
    pub bounds: (i32, i32, u32, u32),
    pub center: Option<(i32, i32, u32, u32)>,
    pub pivot: Option<(i32, i32)>,
}

fn string(out: &mut Vec<u8>, s: &str) {
    out.extend((s.len() as u16).to_le_bytes());
    out.extend(s.as_bytes());
}

fn chunk(out: &mut Vec<u8>, kind: u16, body: &[u8]) {
    out.extend(((body.len() + 6) as u32).to_le_bytes());
    out.extend(kind.to_le_bytes());
    out.extend(body);
}

/// An RGBA `.aseprite` file: `w` x `h`, the layers (bottom first), and per
/// frame its duration and cels; the tags and slices are in frame 0.
pub fn write(w: u16, h: u16, layers: &[Layer], frames: &[(u16, Vec<Cel>)], tags: &[Tag], slices: &[Slice]) -> Vec<u8> {
    let mut body = Vec::new();
    for (f, (duration, cels)) in frames.iter().enumerate() {
        let mut chunks = Vec::new();
        let mut count = 0u32;
        if f == 0 {
            for l in layers {
                let mut c = Vec::new();
                c.extend((u16::from(l.visible) | 2).to_le_bytes());
                c.extend(u16::from(l.group).to_le_bytes());
                c.extend(l.level.to_le_bytes());
                c.extend([0u8; 4]);
                c.extend(l.blend.to_le_bytes());
                c.push(l.opacity);
                c.extend([0u8; 3]);
                string(&mut c, l.name);
                chunk(&mut chunks, 0x2004, &c);
                count += 1;
            }
            if !tags.is_empty() {
                let mut c = Vec::new();
                c.extend((tags.len() as u16).to_le_bytes());
                c.extend([0u8; 8]);
                for Tag(from, to, direction, repeat, name) in tags {
                    c.extend(from.to_le_bytes());
                    c.extend(to.to_le_bytes());
                    c.push(*direction);
                    c.extend(repeat.to_le_bytes());
                    c.extend([0u8; 6]);
                    c.extend([255, 0, 0, 0]);
                    string(&mut c, name);
                }
                chunk(&mut chunks, 0x2018, &c);
                count += 1;
            }
            for s in slices {
                let mut c = Vec::new();
                c.extend(1u32.to_le_bytes());
                let flags = u32::from(s.center.is_some()) | (u32::from(s.pivot.is_some()) << 1);
                c.extend(flags.to_le_bytes());
                c.extend(0u32.to_le_bytes());
                string(&mut c, s.name);
                c.extend(0u32.to_le_bytes());
                let (x, y, bw, bh) = s.bounds;
                c.extend(x.to_le_bytes());
                c.extend(y.to_le_bytes());
                c.extend(bw.to_le_bytes());
                c.extend(bh.to_le_bytes());
                if let Some((x, y, cw, ch)) = s.center {
                    c.extend(x.to_le_bytes());
                    c.extend(y.to_le_bytes());
                    c.extend(cw.to_le_bytes());
                    c.extend(ch.to_le_bytes());
                }
                if let Some((x, y)) = s.pivot {
                    c.extend(x.to_le_bytes());
                    c.extend(y.to_le_bytes());
                }
                chunk(&mut chunks, 0x2022, &c);
                count += 1;
            }
        }
        for cel in cels {
            let mut c = Vec::new();
            match cel {
                Cel::Image {
                    layer,
                    x,
                    y,
                    opacity,
                    w,
                    h,
                    rgba,
                } => {
                    c.extend(layer.to_le_bytes());
                    c.extend(x.to_le_bytes());
                    c.extend(y.to_le_bytes());
                    c.push(*opacity);
                    c.extend(2u16.to_le_bytes());
                    c.extend(0i16.to_le_bytes());
                    c.extend([0u8; 5]);
                    c.extend(w.to_le_bytes());
                    c.extend(h.to_le_bytes());
                    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
                    std::io::Write::write_all(&mut z, rgba).unwrap();
                    c.extend(z.finish().unwrap());
                }
                Cel::Linked { layer, x, y, frame } => {
                    c.extend(layer.to_le_bytes());
                    c.extend(x.to_le_bytes());
                    c.extend(y.to_le_bytes());
                    c.push(255);
                    c.extend(1u16.to_le_bytes());
                    c.extend(0i16.to_le_bytes());
                    c.extend([0u8; 5]);
                    c.extend(frame.to_le_bytes());
                }
            }
            chunk(&mut chunks, 0x2005, &c);
            count += 1;
        }
        body.extend(((chunks.len() + 16) as u32).to_le_bytes());
        body.extend(0xF1FAu16.to_le_bytes());
        body.extend((count.min(0xFFFF) as u16).to_le_bytes());
        body.extend(duration.to_le_bytes());
        body.extend([0u8; 2]);
        body.extend(count.to_le_bytes());
        body.extend(chunks);
    }
    let mut out = Vec::new();
    out.extend(((body.len() + 128) as u32).to_le_bytes());
    out.extend(0xA5E0u16.to_le_bytes());
    out.extend((frames.len() as u16).to_le_bytes());
    out.extend(w.to_le_bytes());
    out.extend(h.to_le_bytes());
    out.extend(32u16.to_le_bytes());
    out.extend(1u32.to_le_bytes()); // layer opacity is valid
    out.extend(100u16.to_le_bytes());
    out.extend([0u8; 8]);
    out.push(0);
    out.extend([0u8; 3]);
    out.extend(0u16.to_le_bytes());
    out.extend([1u8, 1]);
    out.extend(0i16.to_le_bytes());
    out.extend(0i16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend([0u8; 84]);
    assert_eq!(out.len(), 128);
    out.extend(body);
    out
}

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];

/// A 7 x 5 hero: a body layer that moves, an eye layer above it (linked
/// in frame 2), a hidden layer and a hidden group with a layer inside;
/// three frames, two tags, two slices.
pub fn hero() -> Vec<u8> {
    let layers = [
        Layer::normal("body"),
        Layer::normal("eye"),
        Layer {
            visible: false,
            ..Layer::normal("sketch")
        },
        Layer {
            visible: false,
            group: true,
            ..Layer::normal("old")
        },
        Layer {
            level: 1,
            ..Layer::normal("old body")
        },
    ];
    let frames = vec![
        (
            100,
            vec![
                solid(0, (0, 0), (3, 5), RED),
                solid(1, (1, 1), (1, 1), BLUE),
                solid(2, (0, 0), (7, 5), GREEN),
                solid(4, (0, 0), (7, 5), GREEN),
            ],
        ),
        (150, vec![solid(0, (2, 0), (3, 5), RED), solid(1, (3, 1), (1, 1), BLUE)]),
        (
            200,
            vec![
                solid(0, (4, 0), (3, 5), RED),
                Cel::Linked {
                    layer: 1,
                    x: 3,
                    y: 1,
                    frame: 1,
                },
            ],
        ),
    ];
    let tags = [Tag(0, 2, 0, 0, "Walk"), Tag(1, 2, 2, 3, "Wave")];
    let slices = [
        Slice {
            name: "Hitbox",
            bounds: (1, 0, 5, 5),
            center: None,
            pivot: Some((2, 4)),
        },
        Slice {
            name: "Panel",
            bounds: (0, 0, 7, 5),
            center: Some((1, 1, 5, 3)),
            pivot: None,
        },
    ];
    write(7, 5, &layers, &frames, &tags, &slices)
}

fn pixel(png: &[u8], x: u32, y: u32) -> [u8; 4] {
    let image = image::load_from_memory(png).unwrap().into_rgba8();
    image.get_pixel(x, y).0
}

#[test]
fn an_aseprite_file_becomes_the_sheet_and_json_of_its_export() {
    let sheet = export("sprites/hero.aseprite", &hero(), false).unwrap();
    let json: Value = serde_json::from_slice(&sheet.json).unwrap();
    assert_eq!(json["meta"]["image"], "hero.png");
    let frames = json["frames"].as_array().unwrap();
    let durations: Vec<u64> = frames.iter().map(|f| f["duration"].as_u64().unwrap()).collect();
    assert_eq!(durations, [100, 150, 200]);
    assert!(frames.iter().all(|f| f["sourceSize"] == json!({"w": 7, "h": 5})));
    // Frame 0: the body on the left, the eye on it, hidden layers and the
    // hidden group's layer left out.
    let at = |f: usize| {
        let r = &frames[f]["frame"];
        (r["x"].as_u64().unwrap() as u32, r["y"].as_u64().unwrap() as u32)
    };
    let (x, y) = at(0);
    assert_eq!(pixel(&sheet.png, x, y), RED);
    assert_eq!(pixel(&sheet.png, x + 1, y + 1), BLUE);
    assert_eq!(pixel(&sheet.png, x + 5, y + 2), [0, 0, 0, 0], "hidden layers stay out");
    // Frame 2: the body moved, the eye linked to frame 1's.
    let (x, y) = at(2);
    assert_eq!(pixel(&sheet.png, x + 4, y), RED);
    assert_eq!(pixel(&sheet.png, x + 3, y + 1), BLUE);
    assert_eq!(pixel(&sheet.png, x, y), [0, 0, 0, 0]);
    let tags = &json["meta"]["frameTags"];
    assert_eq!(
        tags[0],
        json!({"name": "Walk", "from": 0, "to": 2, "direction": "forward"})
    );
    assert_eq!(
        tags[1],
        json!({"name": "Wave", "from": 1, "to": 2, "direction": "pingpong", "repeat": "3"})
    );
    let slices = &json["meta"]["slices"];
    assert_eq!(slices[0]["name"], "Hitbox");
    assert_eq!(slices[0]["keys"][0]["bounds"], json!({"x": 1, "y": 0, "w": 5, "h": 5}));
    assert_eq!(slices[0]["keys"][0]["pivot"], json!({"x": 2, "y": 4}));
    assert_eq!(slices[1]["keys"][0]["center"], json!({"x": 1, "y": 1, "w": 5, "h": 3}));
    assert!(json["meta"].get("layers").is_none());
    assert_eq!(sheet.note, "3 frames, 2 tags, 2 slices, 7 x 5");
    // The same bytes every time.
    let again = export("sprites/hero.aseprite", &hero(), false).unwrap();
    assert_eq!((again.json, again.png), (sheet.json, sheet.png));
}

#[test]
fn split_layers_name_each_frame_frame_colon_layer() {
    let sheet = export("hero.ase", &hero(), true).unwrap();
    let json: Value = serde_json::from_slice(&sheet.json).unwrap();
    assert_eq!(json["meta"]["layers"], json!([{"name": "body"}, {"name": "eye"}]));
    let names: Vec<&str> = json["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["filename"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["0:body", "1:body", "2:body", "0:eye", "1:eye", "2:eye"]);
    // The eye layer's frame 0 has only the eye.
    let r = &json["frames"][3]["frame"];
    let (x, y) = (r["x"].as_u64().unwrap() as u32, r["y"].as_u64().unwrap() as u32);
    assert_eq!(pixel(&sheet.png, x + 1, y + 1), BLUE);
    assert_eq!(pixel(&sheet.png, x, y), [0, 0, 0, 0]);
}

#[test]
fn layer_and_cel_opacity_and_blend_modes_composite_as_aseprite_does() {
    let layers = [
        Layer::normal("base"),
        Layer {
            opacity: 128,
            ..Layer::normal("half")
        },
        Layer {
            blend: 1,
            ..Layer::normal("multiply")
        },
        Layer {
            opacity: 128,
            ..Layer::normal("quarter")
        },
    ];
    let mut faint = solid(3, (1, 0), (1, 1), [255, 255, 255, 255]);
    if let Cel::Image { opacity, .. } = &mut faint {
        *opacity = 128;
    }
    let frames = vec![(
        100,
        vec![
            solid(0, (0, 0), (4, 1), [0, 0, 0, 255]),
            solid(1, (0, 0), (1, 1), [255, 255, 255, 255]),
            faint,
            solid(2, (3, 0), (1, 1), [128, 128, 128, 255]),
        ],
    )];
    let file = write(4, 1, &layers, &frames, &[], &[]);
    let sheet = export("x.aseprite", &file, false).unwrap();
    let half = pixel(&sheet.png, 0, 0)[0];
    assert!((126..=130).contains(&half), "layer opacity 128 over black: {half}");
    let quarter = pixel(&sheet.png, 1, 0)[0];
    assert!(
        (62..=66).contains(&quarter),
        "layer 128 x cel 128 over black: {quarter}"
    );
    assert_eq!(
        pixel(&sheet.png, 3, 0)[..3],
        [0, 0, 0],
        "multiply over black stays black"
    );
}

#[test]
fn broken_and_tilemap_files_fail_naming_the_file() {
    let err = export("bad.aseprite", b"not an aseprite file at all", false).unwrap_err();
    assert!(err.starts_with("bad.aseprite: not a readable Aseprite file"), "{err}");
    // Two visible layers of one name can't be split.
    let twins = write(
        2,
        2,
        &[Layer::normal("a"), Layer::normal("a")],
        &[(100, vec![solid(0, (0, 0), (1, 1), RED)])],
        &[],
        &[],
    );
    let err = export("twins.aseprite", &twins, true).unwrap_err();
    assert!(err.contains("named \"a\""), "{err}");
    assert!(export("twins.aseprite", &twins, false).is_ok());
}
