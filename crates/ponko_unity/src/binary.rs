//! Unity's binary serialized files (assets saved by the editor in binary,
//! such as terrain data): decoded with the type trees they carry.

use crate::{Error, UnityFile, UnityObject, Value};

/// Unity's built-in strings, which type trees refer to by offset (high
/// bit set) instead of storing them: the editor's table, in its order.
const COMMON: &str = "AABB\0AnimationClip\0AnimationCurve\0AnimationState\0Array\0Base\0BitField\0bitset\0bool\0char\0ColorRGBA\0Component\0data\0deque\0double\0dynamic_array\0FastPropertyName\0first\0float\0Font\0GameObject\0Generic Mono\0GradientNEW\0GUID\0GUIStyle\0int\0list\0long long\0map\0Matrix4x4f\0MdFour\0MonoBehaviour\0MonoScript\0m_ByteSize\0m_Curve\0m_EditorClassIdentifier\0m_EditorHideFlags\0m_Enabled\0m_ExtensionPtr\0m_GameObject\0m_Index\0m_IsArray\0m_IsStatic\0m_MetaFlag\0m_Name\0m_ObjectHideFlags\0m_PrefabInternal\0m_PrefabParentObject\0m_Script\0m_StaticEditorFlags\0m_Type\0m_Version\0Object\0pair\0PPtr<Component>\0PPtr<GameObject>\0PPtr<Material>\0PPtr<MonoBehaviour>\0PPtr<MonoScript>\0PPtr<Object>\0PPtr<Prefab>\0PPtr<Sprite>\0PPtr<TextAsset>\0PPtr<Texture>\0PPtr<Texture2D>\0PPtr<Transform>\0Prefab\0Quaternionf\0Rectf\0RectInt\0RectOffset\0second\0set\0short\0size\0SInt16\0SInt32\0SInt64\0SInt8\0staticvector\0string\0TextAsset\0TextMesh\0Texture\0Texture2D\0Transform\0TypelessData\0UInt16\0UInt32\0UInt64\0UInt8\0unsigned int\0unsigned long long\0unsigned short\0vector\0Vector2f\0Vector3f\0Vector4f\0m_ScriptingClassIdentifier\0Gradient\0Type*\0int2_storage\0int3_storage\0BoundsInt\0m_CorrespondingSourceObject\0m_PrefabInstance\0m_PrefabAsset\0FileSize\0Hash128\0RenderingLayerMask\0fixed_array\0EntityId\0LoadableObjectId\0LoadableSceneId\0";

/// Unity 6's wrapper around each type tree: `mhtt`.
const TREE_MAGIC: u32 = u32::from_le_bytes(*b"mhtt");

/// The node flag saying the data after it is padded to 4 bytes.
const ALIGN: i32 = 0x4000;

struct Node {
    level: u8,
    array: bool,
    ty: String,
    name: String,
    flags: i32,
    /// The index just past this node's subtree (its children follow it).
    end: usize,
}

struct Type {
    class_id: i32,
    nodes: Vec<Node>,
}

/// A little reader over the file's bytes.
struct Reader<'a> {
    data: &'a [u8],
    at: usize,
    big: bool,
    /// The memory the rest of the parse may still take, in bytes (see
    /// [`Reader::spend`]): each value's own size, each map entry's key and
    /// each byte of text, data or numbers it holds. Values can take no
    /// bytes of the file (empty structs) and names are copied for every
    /// value that uses them, so running out of data alone doesn't bound
    /// what a file decodes to: this does.
    budget: usize,
    /// How deep [`read_node`] is.
    depth: usize,
}

/// A file's budget: [`BUDGET_PER_BYTE`] bytes of memory per byte of the
/// file, plus this. Real assets take well under it (a struct of one-byte
/// fields comes closest); a crafted file can't take more.
const EXTRA_BUDGET: usize = 1024 * 1024;
const BUDGET_PER_BYTE: usize = 64;
/// What one value costs, besides what it holds.
const VALUE_COST: usize = std::mem::size_of::<Value>();
/// What a map entry costs besides its value: its key and the key's text.
const fn entry_cost(key: &str) -> usize {
    std::mem::size_of::<String>() + key.len()
}
/// The longest type or field name a type tree may have.
const MAX_NAME: usize = 1024;
/// The deepest nesting of values; type trees nest a handful of levels.
const MAX_DEPTH: usize = 128;

impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self.at.checked_add(n).filter(|&e| e <= self.data.len());
        let end = end.ok_or_else(|| Error::decode(format!("binary asset: data ends early (at byte {})", self.at)))?;
        let b = &self.data[self.at..end];
        self.at = end;
        Ok(b)
    }

    /// How many bytes are left to read.
    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.at)
    }

    /// Takes `n` bytes from the budget, or fails: the file decodes to more
    /// than its size allows. Charged before the memory is taken.
    fn spend(&mut self, n: usize) -> Result<(), Error> {
        self.budget = self.budget.checked_sub(n).ok_or_else(|| {
            Error::decode("binary asset: it decodes to more than its size allows (corrupt counts or offsets?)")
        })?;
        Ok(())
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let mut a: [u8; N] = self.bytes(N)?.try_into().expect("N bytes");
        if self.big {
            a.reverse();
        }
        Ok(a)
    }

    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.bytes(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.array()?))
    }
    fn i16(&mut self) -> Result<i16, Error> {
        Ok(i16::from_le_bytes(self.array()?))
    }
    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.array()?))
    }
    fn i32(&mut self) -> Result<i32, Error> {
        Ok(i32::from_le_bytes(self.array()?))
    }
    /// A length or element count: an `i32` that must not be negative.
    fn count(&mut self) -> Result<usize, Error> {
        let at = self.at;
        let n = self.i32()?;
        usize::try_from(n).map_err(|_| Error::decode(format!("binary asset: a count of {n} (at byte {at})")))
    }
    fn u64(&mut self) -> Result<u64, Error> {
        Ok(u64::from_le_bytes(self.array()?))
    }
    fn i64(&mut self) -> Result<i64, Error> {
        Ok(i64::from_le_bytes(self.array()?))
    }
    fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_le_bytes(self.array()?))
    }
    fn f64(&mut self) -> Result<f64, Error> {
        Ok(f64::from_le_bytes(self.array()?))
    }

    fn cstring(&mut self) -> Result<String, Error> {
        let rest = &self.data[self.at.min(self.data.len())..];
        let end = rest
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| Error::decode("binary asset: unterminated string"))?;
        let s = String::from_utf8_lossy(&rest[..end]).into_owned();
        self.at += end + 1;
        Ok(s)
    }

    fn align(&mut self) {
        self.at = self.at.next_multiple_of(4);
    }
}

/// Whether `data` looks like a binary serialized file.
pub(crate) fn is_binary(data: &[u8]) -> bool {
    data.len() >= 48
        && u32::from_be_bytes([data[8], data[9], data[10], data[11]]) >= 9
        && data[..4] != *b"%YAM"
        && data[..3] != *b"---"
}

/// Parses a binary serialized file (format 22 or later, with type trees).
pub(crate) fn parse(data: &[u8]) -> Result<UnityFile, Error> {
    let mut r = Reader {
        data,
        at: 0,
        big: true,
        budget: data.len().saturating_mul(BUDGET_PER_BYTE).saturating_add(EXTRA_BUDGET),
        depth: 0,
    };
    let _legacy_meta = r.u32()?;
    let _legacy_size = r.u32()?;
    let version = r.u32()?;
    let _legacy_offset = r.u32()?;
    if version < 22 {
        return Err(Error::unsupported(format!(
            "binary asset format {version} is older than Unity 2022 (22); resave it"
        )));
    }
    let big_endian = r.u8()? != 0;
    r.bytes(3)?;
    let _meta_size = r.u32()?;
    let _file_size = r.u64()?;
    let data_offset = r.u64()?;
    let data_offset =
        usize::try_from(data_offset).map_err(|_| Error::decode(format!("binary asset: data at byte {data_offset}")))?;
    let _unknown = r.u64()?;
    r.big = big_endian;

    let _unity_version = r.cstring()?;
    let _platform = r.i32()?;
    if r.u8()? == 0 {
        return Err(Error::unsupported(
            "binary asset has no type trees (only editor-saved assets are supported)",
        ));
    }
    let type_count = r.i32()?;
    let mut types = Vec::new();
    for _ in 0..type_count {
        types.push(read_type(&mut r)?);
    }
    let object_count = r.i32()?;
    let mut entries = Vec::new();
    for _ in 0..object_count {
        r.align();
        let path_id = r.i64()?;
        let start = r.i64()?;
        let start = usize::try_from(start)
            .map_err(|_| Error::decode(format!("binary asset: object {path_id} starts at {start}")))?;
        let size = r.u32()? as usize;
        let type_index = r.i32()?;
        let type_index = usize::try_from(type_index).unwrap_or(usize::MAX);
        entries.push((path_id, start, size, type_index));
    }
    // Scripts, then the files that references point into.
    let scripts = r.i32()?;
    for _ in 0..scripts {
        let _file = r.i32()?;
        r.align();
        let _id = r.i64()?;
    }
    let externals = r.i32()?;
    let mut guids = Vec::new();
    for _ in 0..externals {
        let _empty = r.cstring()?;
        let guid = r.bytes(16)?;
        let _kind = r.i32()?;
        let _path = r.cstring()?;
        guids.push(guid_text(guid));
    }

    // Each object's bytes lie in the data section, and no two overlap: so
    // the objects together read the file at most once.
    let mut ranges = Vec::with_capacity(entries.len());
    for &(path_id, start, size, _) in &entries {
        let range = data_offset
            .checked_add(start)
            .and_then(|begin| Some(begin..begin.checked_add(size)?))
            .filter(|range| range.end <= data.len())
            .ok_or_else(|| Error::decode(format!("binary asset: object {path_id} lies outside the data")))?;
        ranges.push((range, path_id));
    }
    ranges.sort_by_key(|(range, _)| (range.start, range.end));
    for pair in ranges.windows(2) {
        if pair[1].0.start < pair[0].0.end {
            return Err(Error::decode(format!(
                "binary asset: objects {} and {} overlap",
                pair[0].1, pair[1].1
            )));
        }
    }

    let mut objects = Vec::new();
    for (path_id, start, size, type_index) in entries {
        let ty = types
            .get(type_index)
            .ok_or_else(|| Error::decode(format!("binary asset: object {path_id} has no type {type_index}")))?;
        let begin = data_offset + start; // checked above
        let bytes = &data[begin..begin + size];
        let mut o = Reader {
            data: bytes,
            at: 0,
            big: big_endian,
            budget: r.budget,
            depth: 0,
        };
        let type_name = &ty.nodes[0].ty;
        o.spend(std::mem::size_of::<UnityObject>() + type_name.len())?;
        let mut i = 0;
        let body = read_node(&mut o, &ty.nodes, &mut i, &guids)?;
        r.budget = o.budget;
        if o.at != bytes.len() {
            return Err(Error::decode(format!(
                "binary asset: object {path_id} ({}) read {} of its {} bytes",
                ty.nodes[0].ty,
                o.at,
                bytes.len()
            )));
        }
        objects.push(UnityObject {
            class_id: ty.class_id,
            file_id: path_id,
            stripped: false,
            type_name: type_name.clone(),
            body,
        });
    }
    Ok(UnityFile { objects })
}

/// Unity writes a GUID's 16 bytes as text with each byte's two hex
/// digits swapped.
fn guid_text(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:x}{:x}", b & 0xf, b >> 4)).collect()
}

fn read_type(r: &mut Reader) -> Result<Type, Error> {
    let class_id = r.i32()?;
    let _stripped = r.u8()?;
    let _script_index = r.i16()?;
    // Hashes: the script's id (scripts only before Unity 6, every type
    // since) and the type's. Then, since Unity 6: the tree's size,
    // `mhtt` and a version; before: the tree itself.
    let magic_after = |skip: usize| {
        let at = r.at + skip + 4;
        r.data
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            == Some(TREE_MAGIC)
    };
    if magic_after(16) {
        r.bytes(16 + 12)?;
    } else if magic_after(32) {
        r.bytes(32 + 12)?;
    } else {
        r.bytes(if class_id == 114 { 32 } else { 16 })?;
    }
    // Checked before anything is allocated: each node takes 32 bytes, so a
    // count the rest of the file can't hold is corrupt.
    const NODE_BYTES: usize = 32;
    let count = r.i32()?;
    let count = usize::try_from(count)
        .ok()
        .filter(|&c| c > 0 && c <= r.remaining() / NODE_BYTES)
        .ok_or_else(|| Error::decode(format!("binary asset: a type tree with {count} nodes")))?;
    let strings_len = r.i32()?;
    let strings_len = usize::try_from(strings_len)
        .map_err(|_| Error::decode(format!("binary asset: type tree strings of {strings_len} bytes")))?;
    let mut raw = Vec::with_capacity(count);
    for _ in 0..count {
        let _version = r.u16()?;
        let level = r.u8()?;
        let type_flags = r.u8()?;
        let ty = r.u32()?;
        let name = r.u32()?;
        let size = r.i32()?;
        let _index = r.i32()?;
        let flags = r.i32()?;
        let _ref_hash = r.u64()?;
        raw.push((level, type_flags, ty, name, size, flags));
    }
    let strings = r.bytes(strings_len)?;
    // A name is copied for every node that uses it: each copy counts
    // against the budget, and none may be longer than `MAX_NAME` or run
    // off the end of its table without its NUL.
    let text = |r: &mut Reader, offset: u32| -> Result<String, Error> {
        let (table, at) = if offset & 0x8000_0000 != 0 {
            (COMMON.as_bytes(), (offset & 0x7fff_ffff) as usize)
        } else {
            (strings, offset as usize)
        };
        let rest = table.get(at..).unwrap_or_default();
        let window = &rest[..rest.len().min(MAX_NAME + 1)];
        let end = window.iter().position(|&b| b == 0).ok_or_else(|| {
            Error::decode(if window.len() > MAX_NAME {
                format!("binary asset: a type tree name longer than {MAX_NAME} bytes")
            } else {
                "binary asset: a type tree name without an end".to_owned()
            })
        })?;
        r.spend(std::mem::size_of::<String>() + end)?;
        let name = String::from_utf8_lossy(&window[..end]).into_owned();
        // Bytes that aren't UTF-8 grow into replacement characters.
        r.spend(name.len() - end)?;
        Ok(name)
    };
    let mut nodes = Vec::with_capacity(raw.len());
    for (level, type_flags, ty, name, _size, flags) in raw {
        nodes.push(Node {
            level,
            array: type_flags & 1 != 0,
            ty: text(r, ty)?,
            name: text(r, name)?,
            flags,
            end: 0,
        });
    }
    // Each node's subtree end, in one pass from the back: the first later
    // node that isn't deeper.
    let mut later: Vec<usize> = Vec::new();
    for i in (0..nodes.len()).rev() {
        while later.last().is_some_and(|&j| nodes[j].level > nodes[i].level) {
            later.pop();
        }
        nodes[i].end = later.last().copied().unwrap_or(nodes.len());
        later.push(i);
    }
    let dependencies = r.i32()?;
    for _ in 0..dependencies {
        r.i32()?;
    }
    Ok(Type { class_id, nodes })
}

/// Reads node `*i` (and its children), leaving `*i` after its subtree.
fn read_node(r: &mut Reader, nodes: &[Node], i: &mut usize, guids: &[String]) -> Result<Value, Error> {
    r.spend(VALUE_COST)?;
    r.depth += 1;
    if r.depth > MAX_DEPTH {
        return Err(Error::decode(format!(
            "binary asset: values nested deeper than {MAX_DEPTH}"
        )));
    }
    let value = read_node_value(r, nodes, i, guids)?;
    r.depth -= 1;
    Ok(value)
}

fn read_node_value(r: &mut Reader, nodes: &[Node], i: &mut usize, guids: &[String]) -> Result<Value, Error> {
    let node = &nodes[*i];
    let end = node.end;
    let value = if let Some(v) = primitive(r, &node.ty)? {
        *i += 1;
        v
    } else if node.ty == "string" {
        let n = r.count()?;
        let bytes = r.bytes(n)?;
        r.spend(n)?;
        let s = String::from_utf8_lossy(bytes).into_owned();
        // Bytes that aren't UTF-8 grow into replacement characters.
        r.spend(s.len() - n)?;
        // A string's array child carries the padding flag.
        if nodes.get(*i + 1).is_some_and(|c| c.flags & ALIGN != 0) {
            r.align();
        }
        *i = end;
        Value::Scalar(s)
    } else if node.ty == "TypelessData" {
        let n = r.count()?;
        let bytes = r.bytes(n)?;
        r.spend(n)?;
        let b = bytes.to_vec();
        *i = end;
        Value::Bytes(b)
    } else if node.array {
        // `Array`: its size, then that many of its data child, which must
        // be in its subtree (deeper), so nesting is bounded by the levels.
        let element = *i + 2;
        if element >= end {
            return Err(Error::decode(format!(
                "binary asset: array {} has no element type in its subtree",
                node.name
            )));
        }
        let n = r.count()?;
        let value = read_array(r, nodes, element, n, guids)?;
        *i = end;
        value
    } else if node.ty.starts_with("PPtr<") {
        *i += 1;
        let mut file = 0;
        let mut path = 0;
        while *i < end {
            let child = &nodes[*i];
            let v = read_node(r, nodes, i, guids)?;
            match child.name.as_str() {
                "m_FileID" => file = v.as_i64().unwrap_or(0),
                "m_PathID" => path = v.as_i64().unwrap_or(0),
                _ => {}
            }
        }
        let path = path.to_string();
        r.spend(entry_cost("fileID") + VALUE_COST + path.len())?;
        let mut entries = vec![("fileID".to_owned(), Value::Scalar(path))];
        let external = (file > 0)
            .then(|| usize::try_from(file - 1))
            .transpose()
            .map_err(|_| Error::decode(format!("binary asset: a reference into external file {file}")))?;
        if let Some(g) = external.and_then(|f| guids.get(f)) {
            r.spend(entry_cost("guid") + VALUE_COST + g.len())?;
            entries.push(("guid".to_owned(), Value::Scalar(g.clone())));
        }
        Value::Map(entries)
    } else {
        // A struct: its children by name. A child that's an array makes
        // the struct a container (vector, map, ...): it stands for it.
        *i += 1;
        let mut entries = Vec::new();
        while *i < end {
            let name = &nodes[*i].name;
            r.spend(entry_cost(name))?;
            let name = name.clone();
            entries.push((name, read_node(r, nodes, i, guids)?));
        }
        if let [(name, _)] = entries.as_slice()
            && name == "Array"
        {
            entries.pop().expect("one entry").1
        } else {
            Value::Map(entries)
        }
    };
    if node.flags & ALIGN != 0 {
        r.align();
    }
    Ok(value)
}

/// `n` elements of node `element`'s type.
fn read_array(r: &mut Reader, nodes: &[Node], element: usize, n: usize, guids: &[String]) -> Result<Value, Error> {
    let el = nodes
        .get(element)
        .ok_or_else(|| Error::decode("binary asset: an array without an element type"))?;
    // Numbers come out as one compact list (64-bit integers stay text, as
    // an f64 can't hold them all).
    let wide = matches!(
        el.ty.as_str(),
        "SInt64" | "UInt64" | "long long" | "unsigned long long" | "FileSize"
    );
    if el.end == element + 1 && el.flags & ALIGN == 0 && !wide {
        let Some(size) = number_size(&el.ty) else {
            return Err(Error::unsupported(format!(
                "binary asset: an array of {} isn't supported",
                el.ty
            )));
        };
        // Checked against the data first: each number takes its bytes.
        if n.checked_mul(size).is_none_or(|bytes| bytes > r.remaining()) {
            return Err(Error::decode(format!(
                "binary asset: data ends early (at byte {})",
                r.at
            )));
        }
        r.spend(n * std::mem::size_of::<f64>())?;
        let mut numbers = Vec::with_capacity(n);
        for _ in 0..n {
            numbers.push(number(r, &el.ty)?);
        }
        return Ok(Value::Numbers(numbers));
    }
    // Grown as elements are read (each charged to the budget), never
    // reserved up front from the count.
    let mut items = Vec::with_capacity(n.min(1024));
    for _ in 0..n {
        let mut i = element;
        items.push(read_node(r, nodes, &mut i, guids)?);
    }
    Ok(Value::Seq(items))
}

/// The bytes one number of a primitive type takes, or `None` for other types.
fn number_size(ty: &str) -> Option<usize> {
    Some(match ty {
        "bool" | "SInt8" | "UInt8" | "char" => 1,
        "SInt16" | "short" | "UInt16" | "unsigned short" => 2,
        "SInt32" | "int" | "Type*" | "UInt32" | "unsigned int" | "float" => 4,
        "double" => 8,
        _ => return None,
    })
}

/// One number of a type [`number_size`] knows, read straight into an f64.
fn number(r: &mut Reader, ty: &str) -> Result<f64, Error> {
    Ok(match ty {
        "bool" => f64::from(u8::from(r.u8()? != 0)),
        "SInt8" => f64::from(r.u8()? as i8),
        "UInt8" | "char" => f64::from(r.u8()?),
        "SInt16" | "short" => f64::from(r.i16()?),
        "UInt16" | "unsigned short" => f64::from(r.u16()?),
        "SInt32" | "int" | "Type*" => f64::from(r.i32()?),
        "UInt32" | "unsigned int" => f64::from(r.u32()?),
        "float" => f64::from(r.f32()?),
        "double" => r.f64()?,
        _ => unreachable!("number_size knows {ty}"),
    })
}

/// A primitive type's value, or `None` for other types.
fn primitive(r: &mut Reader, ty: &str) -> Result<Option<Value>, Error> {
    let text = match ty {
        "bool" => if r.u8()? != 0 { "1" } else { "0" }.to_owned(),
        "SInt8" => (r.u8()? as i8).to_string(),
        "UInt8" | "char" => r.u8()?.to_string(),
        "SInt16" | "short" => r.i16()?.to_string(),
        "UInt16" | "unsigned short" => r.u16()?.to_string(),
        "SInt32" | "int" | "Type*" => r.i32()?.to_string(),
        "UInt32" | "unsigned int" => r.u32()?.to_string(),
        "SInt64" | "long long" | "FileSize" => r.i64()?.to_string(),
        "UInt64" | "unsigned long long" => r.u64()?.to_string(),
        "float" => r.f32()?.to_string(),
        "double" => r.f64()?.to_string(),
        _ => return Ok(None),
    };
    // At most a few hundred bytes (an f64's digits), but charged too.
    r.spend(text.len())?;
    Ok(Some(Value::Scalar(text)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guids_swap_each_bytes_digits() {
        assert_eq!(guid_text(&[0x12, 0xab]), "21ba");
    }

    #[test]
    fn common_strings_start_where_type_trees_expect() {
        // Offsets type trees use for these.
        let at = |s: &str| COMMON.find(&format!("{s}\0")).unwrap();
        assert_eq!(at("AABB"), 0);
        assert_eq!(at("Array"), 49);
        assert_eq!(at("Base"), 55);
        assert_eq!(COMMON.matches('\0').count(), 114);
    }
}
