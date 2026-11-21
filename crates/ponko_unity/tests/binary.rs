//! Binary serialized assets: a small Unity 6 style file built here, byte
//! by byte, the way the editor lays one out.

use ponko_unity::*;

/// Offsets into Unity's built-in strings (high bit set).
const fn common(offset: u32) -> u32 {
    0x8000_0000 | offset
}
const ARRAY: u32 = common(49);
const BASE: u32 = common(55);
const INT: u32 = common(222);
const M_NAME: u32 = common(427);
const SINT16: u32 = common(800);
const SINT64: u32 = common(814);
const STRING: u32 = common(840);
const SIZE: u32 = common(795);
const DATA: u32 = common(106);
const VECTOR: u32 = common(981);
const CHAR: u32 = common(81);

/// One type tree node: (level, is array, type, name, size, flags).
type Node = (u8, bool, u32, u32, i32, i32);

struct Strings(Vec<u8>);

impl Strings {
    fn add(&mut self, s: &str) -> u32 {
        let at = self.0.len() as u32;
        self.0.extend_from_slice(s.as_bytes());
        self.0.push(0);
        at
    }
}

/// Runs `parse` and checks it took under 5 s. The inputs here make a parser
/// that walks the counts they claim (or rescans what it read) take minutes;
/// parsing them as intended is milliseconds of work, which even a machine
/// running a dozen threads per core stretches only to a fraction of a
/// second. So the bound holds on a busy machine and fails on the blow-up.
fn quickly<T>(parse: impl FnOnce() -> T) -> T {
    let started = std::time::Instant::now();
    let parsed = parse();
    let took = started.elapsed();
    assert!(took < std::time::Duration::from_secs(5), "took {took:?}");
    parsed
}

fn file() -> Vec<u8> {
    let mut local = Strings(Vec::new());
    let test_object = local.add("TestObject");
    let count = local.add("m_Count");
    let heights = local.add("m_Heights");
    let pptr = local.add("PPtr<Object>");
    let link = local.add("m_Link");
    let file_id = local.add("m_FileID");
    let path_id = local.add("m_PathID");
    const ALIGN: i32 = 0x4000;
    let nodes: Vec<Node> = vec![
        (0, false, test_object, BASE, -1, 0),
        (1, false, STRING, M_NAME, -1, 0),
        (2, true, ARRAY, ARRAY, -1, ALIGN),
        (3, false, INT, SIZE, 4, 0),
        (3, false, CHAR, DATA, 1, 0),
        (1, false, INT, count, 4, 0),
        (1, false, VECTOR, heights, -1, 0),
        (2, true, ARRAY, ARRAY, -1, ALIGN),
        (3, false, INT, SIZE, 4, 0),
        (3, false, SINT16, DATA, 2, 0),
        (1, false, pptr, link, 12, 0),
        (2, false, INT, file_id, 4, 0),
        (2, false, SINT64, path_id, 8, 0),
    ];
    // The object: "Rock" (padded), 7, three heights (padded), a link
    // into external file 1, object 42.
    let mut object = Vec::new();
    object.extend(4i32.to_le_bytes());
    object.extend(b"Rock");
    object.extend(7i32.to_le_bytes());
    object.extend(3i32.to_le_bytes());
    for h in [100i16, -5, 32000] {
        object.extend(h.to_le_bytes());
    }
    object.extend([0, 0]); // to 4 bytes
    object.extend(1i32.to_le_bytes());
    object.extend(42i64.to_le_bytes());
    build(&nodes, &local, &object)
}

/// A file with one type (`nodes`, naming `local` strings) and one object
/// of it (`object`).
fn build(nodes: &[Node], local: &Strings, object: &[u8]) -> Vec<u8> {
    build_objects(nodes, local, object, &[(77, 0, object.len() as u32)])
}

/// [`build`] with a data section `data` and objects (path id, start, size)
/// of the type in it.
fn build_objects(nodes: &[Node], local: &Strings, object: &[u8], objects: &[(i64, i64, u32)]) -> Vec<u8> {
    let mut tree = Vec::new();
    tree.extend((nodes.len() as i32).to_le_bytes());
    tree.extend((local.0.len() as i32).to_le_bytes());
    for (level, array, ty, name, size, flags) in nodes {
        tree.extend(1u16.to_le_bytes());
        tree.push(*level);
        tree.push(u8::from(*array));
        tree.extend(ty.to_le_bytes());
        tree.extend(name.to_le_bytes());
        tree.extend(size.to_le_bytes());
        tree.extend(0i32.to_le_bytes());
        tree.extend(flags.to_le_bytes());
        tree.extend(0u64.to_le_bytes());
    }
    tree.extend(&local.0);

    let mut meta = Vec::new();
    meta.extend(b"6000.0.0f1\0");
    meta.extend((-2i32).to_le_bytes());
    meta.push(1); // type trees
    meta.extend(1i32.to_le_bytes()); // one type
    meta.extend(999i32.to_le_bytes());
    meta.push(0);
    meta.extend((-1i16).to_le_bytes());
    meta.extend([0u8; 32]); // the hashes
    meta.extend(((tree.len() + 8) as u32).to_le_bytes());
    meta.extend(b"mhtt");
    meta.extend(23u32.to_le_bytes());
    meta.extend(&tree);
    meta.extend(0i32.to_le_bytes()); // dependencies
    meta.extend((objects.len() as i32).to_le_bytes());
    while (48 + meta.len()) % 4 != 0 {
        meta.push(0);
    }
    for (path_id, start, size) in objects {
        meta.extend(path_id.to_le_bytes());
        meta.extend(start.to_le_bytes());
        meta.extend(size.to_le_bytes());
        meta.extend(0i32.to_le_bytes()); // type 0
    }
    meta.extend(0i32.to_le_bytes()); // scripts
    meta.extend(1i32.to_le_bytes()); // externals
    meta.push(0);
    meta.extend([0x12, 0xab, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01]);
    meta.extend(2i32.to_le_bytes());
    meta.extend(b"Assets/Other.asset\0");
    meta.push(0); // user information

    let data_offset = (48 + meta.len()).next_multiple_of(16);
    let mut out = Vec::new();
    out.extend(0u32.to_be_bytes());
    out.extend(0u32.to_be_bytes());
    out.extend(23u32.to_be_bytes());
    out.extend(0u32.to_be_bytes());
    out.push(0); // little endian
    out.extend([0, 0, 0]);
    out.extend((meta.len() as u32).to_be_bytes());
    out.extend(((data_offset + object.len()) as u64).to_be_bytes());
    out.extend((data_offset as u64).to_be_bytes());
    out.extend(0u64.to_be_bytes());
    out.extend(&meta);
    out.resize(data_offset, 0);
    out.extend(object);
    out
}

#[test]
fn binary_objects_decode_through_their_type_tree() {
    let parsed = UnityFile::parse_binary(&file()).unwrap();
    let o = &parsed.objects[0];
    assert_eq!((o.class_id, o.file_id, o.type_name.as_str()), (999, 77, "TestObject"));
    assert_eq!(o.body["m_Name"].as_str(), Some("Rock"));
    assert_eq!(o.body["m_Count"].as_i64(), Some(7));
    assert_eq!(o.body["m_Heights"].as_numbers(), Some(&[100.0, -5.0, 32000.0][..]));
    let link = o.body["m_Link"].obj_ref().unwrap();
    assert_eq!(link.file_id, 42);
    assert_eq!(
        link.guid.as_deref(),
        Some("21ba0000000000000000000000000010"),
        "digits swapped per byte"
    );
}

#[test]
fn files_are_read_as_text_or_binary() {
    let dir = std::env::temp_dir().join(format!("ponko_unity_binary_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.asset"), file()).unwrap();
    std::fs::write(
        dir.join("b.asset"),
        "%YAML 1.1\n--- !u!1 &1\nGameObject:\n  m_Name: B\n",
    )
    .unwrap();
    assert_eq!(
        UnityFile::read(dir.join("a.asset")).unwrap().objects[0].type_name,
        "TestObject"
    );
    assert_eq!(
        UnityFile::read(dir.join("b.asset")).unwrap().objects[0].type_name,
        "GameObject"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn truncated_binary_files_are_errors_not_panics() {
    let whole = file();
    for cut in [60, whole.len() / 2, whole.len() - 3] {
        assert!(UnityFile::parse_binary(&whole[..cut]).is_err(), "cut at {cut}");
    }
}

/// Where the type tree's node count sits in [`file`]: the 48-byte header,
/// the version string, platform, the type-tree flag, the type count, the
/// class id, stripped flag, script index, hashes, tree size, `mhtt` and
/// tree version.
const NODE_COUNT_AT: usize = 48 + 11 + 4 + 1 + 4 + 4 + 1 + 2 + 32 + 4 + 4 + 4;

#[test]
fn malformed_node_counts_are_errors_not_panics() {
    let whole = file();
    assert_eq!(&whole[NODE_COUNT_AT..NODE_COUNT_AT + 4], 13i32.to_le_bytes());
    // Negative, huge, more than the file can hold, none at all; then the
    // same for the length of the type tree's strings.
    for at in [NODE_COUNT_AT, NODE_COUNT_AT + 4] {
        for bad in [-1i32, i32::MIN, i32::MAX, 1 << 26, 0] {
            let mut data = whole.clone();
            data[at..at + 4].copy_from_slice(&bad.to_le_bytes());
            if at == NODE_COUNT_AT + 4 && bad == 0 {
                continue; // no local strings: a valid, if odd, file
            }
            assert!(UnityFile::parse_binary(&data).is_err(), "{bad} at byte {at}");
        }
    }
}

#[test]
fn every_corrupted_byte_is_an_error_or_a_result_never_a_panic() {
    let whole = file();
    for at in 0..whole.len() {
        for value in [0x00, 0x7f, 0x80, 0xff] {
            let mut data = whole.clone();
            data[at] = value;
            let outcome = std::panic::catch_unwind(|| {
                let _ = UnityFile::parse_binary(&data);
            });
            assert!(outcome.is_ok(), "byte {at} set to {value:#x} panicked");
        }
    }
}

/// An array of an empty, aligned struct takes no bytes per element, so a
/// corrupt count can't be caught by running out of data. A field follows
/// the array, so a few elements fit in the bytes left.
fn empty_elements(count: i32) -> Vec<u8> {
    let mut local = Strings(Vec::new());
    let holder = local.add("Holder");
    let items = local.add("m_Items");
    let empty = local.add("Empty");
    let tail = local.add("m_Tail");
    const ALIGN: i32 = 0x4000;
    let nodes: Vec<Node> = vec![
        (0, false, holder, BASE, -1, 0),
        (1, false, VECTOR, items, -1, 0),
        (2, true, ARRAY, ARRAY, -1, ALIGN),
        (3, false, INT, SIZE, 4, 0),
        (3, false, empty, DATA, 0, ALIGN),
        (1, false, INT, tail, 4, 0),
    ];
    let mut object = count.to_le_bytes().to_vec();
    object.extend(9i32.to_le_bytes());
    build(&nodes, &local, &object)
}

#[test]
fn arrays_of_empty_elements_with_corrupt_counts_are_errors() {
    let three = UnityFile::parse_binary(&empty_elements(3)).unwrap();
    assert_eq!(three.objects[0].type_name, "Holder");
    assert_eq!(three.objects[0].body["m_Tail"].as_i64(), Some(9));
    for bad in [-1, i32::MIN, 1 << 30, i32::MAX] {
        let err = quickly(|| UnityFile::parse_binary(&empty_elements(bad))).unwrap_err();
        assert!(err.to_string().contains("binary asset"), "{bad}: {err}");
    }
}

/// `m_Outer`: `outer` elements, each an `Inner` holding an array of empty
/// elements whose count is `inner(k)` for element `k`.
fn nested_empty_elements(outer: i32, inner: impl Fn(usize, usize) -> i32) -> Vec<u8> {
    let mut local = Strings(Vec::new());
    let holder = local.add("Holder");
    let outer_name = local.add("m_Outer");
    let inner_ty = local.add("Inner");
    let inner_name = local.add("m_Inner");
    let empty = local.add("Empty");
    const ALIGN: i32 = 0x4000;
    let nodes: Vec<Node> = vec![
        (0, false, holder, BASE, -1, 0),
        (1, false, VECTOR, outer_name, -1, 0),
        (2, true, ARRAY, ARRAY, -1, ALIGN),
        (3, false, INT, SIZE, 4, 0),
        (3, false, inner_ty, DATA, -1, 0),
        (4, false, VECTOR, inner_name, -1, 0),
        (5, true, ARRAY, ARRAY, -1, ALIGN),
        (6, false, INT, SIZE, 4, 0),
        (6, false, empty, DATA, 0, ALIGN),
    ];
    let total = 4 + 4 * outer.max(0) as usize;
    let mut object = outer.to_le_bytes().to_vec();
    for k in 0..outer.max(0) as usize {
        // Bytes left once this element's count is read.
        let left = total - (4 + 4 * (k + 1));
        object.extend(inner(k, left).to_le_bytes());
    }
    build(&nodes, &local, &object)
}

#[test]
fn nested_arrays_of_empty_elements_are_bounded_by_the_files_size() {
    let small = UnityFile::parse_binary(&nested_empty_elements(3, |_, _| 2)).unwrap();
    assert_eq!(small.objects[0].type_name, "Holder");
    // Each inner array claims as many empty elements as there are bytes
    // left, which a check on one array alone would allow: about a million
    // values per 1000 outer elements, from 4 KB.
    let err = quickly(|| UnityFile::parse_binary(&nested_empty_elements(1000, |_, left| left as i32))).unwrap_err();
    assert!(err.to_string().contains("more than its size allows"), "{err}");
}

/// The small test object twice over the same bytes, and more: many objects
/// on one byte range would decode it again and again.
#[test]
fn overlapping_objects_are_errors() {
    let whole = file();
    let parsed = UnityFile::parse_binary(&whole).unwrap();
    assert_eq!(parsed.objects.len(), 1);
    let mut local = Strings(Vec::new());
    let holder = local.add("Holder");
    let text = local.add("m_Text");
    let nodes: Vec<Node> = vec![
        (0, false, holder, BASE, -1, 0),
        (1, false, STRING, text, -1, 0),
        (2, true, ARRAY, ARRAY, -1, 0x4000),
        (3, false, INT, SIZE, 4, 0),
        (3, false, CHAR, DATA, 1, 0),
    ];
    let mut data = 60_000i32.to_le_bytes().to_vec();
    data.extend(vec![b'x'; 60_000]);
    let size = data.len() as u32;
    // Apart: fine.
    let apart = build_objects(
        &nodes,
        &local,
        &[data.clone(), data.clone()].concat(),
        &[(1, 0, size), (2, i64::from(size), size)],
    );
    assert_eq!(UnityFile::parse_binary(&apart).unwrap().objects.len(), 2);
    // The same bytes, many times.
    let entries: Vec<(i64, i64, u32)> = (0..2_000).map(|n| (n, 0, size)).collect();
    let err = quickly(|| UnityFile::parse_binary(&build_objects(&nodes, &local, &data, &entries))).unwrap_err();
    assert!(err.to_string().contains("overlap"), "{err}");
    // Partly overlapping, and outside the data.
    let partly = build_objects(&nodes, &local, &data, &[(1, 0, size), (2, 4, 8)]);
    assert!(
        UnityFile::parse_binary(&partly)
            .unwrap_err()
            .to_string()
            .contains("overlap")
    );
    let outside = build_objects(&nodes, &local, &data, &[(1, 8, size)]);
    assert!(
        UnityFile::parse_binary(&outside)
            .unwrap_err()
            .to_string()
            .contains("outside the data")
    );
}

/// Every node naming offset 0 of a strings table without a NUL would copy
/// the whole table twice per node.
#[test]
fn type_tree_names_without_an_end_are_errors() {
    let mut local = Strings(vec![b'a'; 200_000]);
    let nodes: Vec<Node> = (0..5_000).map(|n| (u8::from(n > 0), false, 0, 0, 4, 0)).collect();
    let err = quickly(|| UnityFile::parse_binary(&build(&nodes, &local, &[0; 4]))).unwrap_err();
    assert!(err.to_string().contains("name longer than"), "{err}");
    // Short names, shared by many nodes, count against the budget.
    local.0 = b"abcdefghijklmnopqrstuvwxyz".repeat(30);
    local.0.push(0);
    let err = UnityFile::parse_binary(&build(&nodes, &local, &[0; 4])).unwrap_err();
    assert!(err.to_string().contains("more than its size allows"), "{err}");
}

/// Array nodes whose element type isn't in their subtree (here all on
/// level 0) would recurse once per two nodes, past any stack.
#[test]
fn arrays_must_have_their_element_type_inside_them() {
    let mut local = Strings(Vec::new());
    let ty = local.add("Loop");
    let nodes: Vec<Node> = (0..60_000).map(|_| (0, true, ty, ARRAY, -1, 0)).collect();
    let object: Vec<u8> = 1i32.to_le_bytes().repeat(30_000);
    let err = UnityFile::parse_binary(&build(&nodes, &local, &object)).unwrap_err();
    assert!(err.to_string().contains("no element type in its subtree"), "{err}");
}

/// Deep but well-formed nesting stops at a fixed depth too.
#[test]
fn values_nest_only_so_deep() {
    let mut local = Strings(Vec::new());
    let ty = local.add("Box");
    let inner = local.add("m_Inner");
    let mut nodes: Vec<Node> = (0..200u8).map(|level| (level, false, ty, inner, -1, 0)).collect();
    nodes.push((200, false, INT, SIZE, 4, 0));
    let err = UnityFile::parse_binary(&build(&nodes, &local, &7i32.to_le_bytes())).unwrap_err();
    assert!(err.to_string().contains("nested deeper than"), "{err}");
    let shallow: Vec<Node> = (0..20u8)
        .map(|level| (level, false, ty, inner, -1, 0))
        .chain([(20, false, INT, SIZE, 4, 0)])
        .collect();
    assert!(UnityFile::parse_binary(&build(&shallow, &local, &7i32.to_le_bytes())).is_ok());
}

/// A field name of 1 KiB, copied for each of many small elements, would
/// make a thousand times the file's size in names.
#[test]
fn long_field_names_copied_per_value_count_against_the_budget() {
    let mut local = Strings(Vec::new());
    let holder = local.add("Holder");
    let items = local.add("m_Items");
    let elem = local.add("Elem");
    let long = local.add(&"n".repeat(1000));
    let nodes: Vec<Node> = vec![
        (0, false, holder, BASE, -1, 0),
        (1, false, VECTOR, items, -1, 0),
        (2, true, ARRAY, ARRAY, -1, 0),
        (3, false, INT, SIZE, 4, 0),
        (3, false, elem, DATA, 4, 0),
        (4, false, INT, long, 4, 0),
    ];
    let few = [3i32, 1, 2, 3].map(i32::to_le_bytes).concat();
    let parsed = UnityFile::parse_binary(&build(&nodes, &local, &few)).unwrap();
    assert_eq!(parsed.objects[0].body["m_Items"].as_seq().unwrap().len(), 3);
    let n = 100_000;
    let mut object = (n as i32).to_le_bytes().to_vec();
    object.extend(0i32.to_le_bytes().repeat(n));
    let err = UnityFile::parse_binary(&build(&nodes, &local, &object)).unwrap_err();
    assert!(err.to_string().contains("more than its size allows"), "{err}");
}

/// A strings table that ends without a NUL: the name runs off its end.
#[test]
fn a_type_tree_name_must_end_inside_its_table() {
    let local = Strings(b"Holder".to_vec());
    let nodes: Vec<Node> = vec![(0, false, 0, BASE, -1, 0)];
    let err = UnityFile::parse_binary(&build(&nodes, &local, &[])).unwrap_err();
    assert!(err.to_string().contains("name without an end"), "{err}");
}

/// Each element's type is skipped past its subtree: a big subtree under an
/// element read many times must not be walked again for every element.
#[test]
fn big_subtrees_are_skipped_without_walking_them_per_value() {
    let mut local = Strings(Vec::new());
    let holder = local.add("Holder");
    let items = local.add("m_Items");
    let mut nodes: Vec<Node> = vec![
        (0, false, holder, BASE, -1, 0),
        (1, false, VECTOR, items, -1, 0),
        (2, true, ARRAY, ARRAY, -1, 0),
        (3, false, INT, SIZE, 4, 0),
        (3, false, STRING, DATA, -1, 0),
    ];
    nodes.extend((0..50_000).map(|_| (4, false, CHAR, DATA, 1, 0)));
    let n = 50_000;
    let mut object = (n as i32).to_le_bytes().to_vec();
    object.extend(0i32.to_le_bytes().repeat(n));
    let parsed = quickly(|| UnityFile::parse_binary(&build(&nodes, &local, &object))).unwrap();
    assert_eq!(parsed.objects[0].body["m_Items"].as_seq().unwrap().len(), n);
}

/// Arrays nested many levels deep, each claiming a huge count, over a
/// megabyte of data: nothing is reserved from the counts, so this fails
/// as soon as the data runs out.
#[test]
fn nested_arrays_with_huge_counts_reserve_nothing_up_front() {
    let mut local = Strings(Vec::new());
    let holder = local.add("Holder");
    let items = local.add("m_Items");
    let mut nodes: Vec<Node> = vec![(0, false, holder, BASE, -1, 0)];
    let depth = 40u8;
    for k in 0..depth {
        let level = 1 + 3 * k;
        nodes.push((level, false, VECTOR, items, -1, 0));
        nodes.push((level + 1, true, ARRAY, ARRAY, -1, 0));
        nodes.push((level + 2, false, INT, SIZE, 4, 0));
    }
    // The innermost element: a string, so the counts can't be numbers.
    let last = 1 + 3 * depth;
    nodes.push((last - 1, false, STRING, DATA, -1, 0));
    let mut object = i32::MAX.to_le_bytes().repeat(usize::from(depth));
    object.extend(i32::MAX.to_le_bytes());
    object.extend(vec![b'x'; 1 << 20]);
    let err = quickly(|| UnityFile::parse_binary(&build(&nodes, &local, &object))).unwrap_err();
    assert!(err.to_string().contains("binary asset"), "{err}");
}
