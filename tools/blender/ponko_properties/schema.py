"""Reads `ponko-schema.json` (written by `ponko schema`) and checks custom
properties against it, without Blender: the add-on's logic, tested by
`tools/blender/test_ponko_properties.py` with plain Python.

The schema is a JSON Schema (draft 2020-12) of a glTF node's `extras`.
This module checks the part of JSON Schema the engine writes: `type`,
`enum`, `const`, `anyOf`, `not`, `pattern`, `properties`, `required`,
`additionalProperties`, `items`, `prefixItems`, `minItems`, `maxItems`,
`minimum`, `maximum` and `minLength`. Other keywords are ignored.
"""

import json
import re

PREFIX = "ponko:"

# A schema that takes a number, or a list of numbers etc., described for the
# "Add property" menu: what Blender property type a new one gets.
KINDS = ("choice", "flag", "layers", "spawn", "file", "lod", "component")


class Property:
    """One `ponko:` property the game reads."""

    def __init__(self, key, schema):
        self.key = key  # with the prefix: "ponko:collider"
        self.name = key[len(PREFIX):]
        self.schema = schema
        self.description = schema.get("description", "")
        self.choices = list(schema.get("enum", []))
        self.kind = _kind(self.name, schema)

    def default(self):
        """A value of the Blender property type a new property gets: a
        string, an int, a float list or a bool."""
        if self.kind == "choice":
            return self.choices[0] if self.choices else ""
        if self.kind == "flag":
            return True
        if self.kind == "layers":
            return 0
        if self.kind == "spawn":
            return True
        if self.kind == "file":
            return "prefabs/thing.json"
        if self.kind == "lod":
            return [0.3, 0.08]
        return json.dumps(example(_without_text(self.schema)))

    def __repr__(self):
        return f"Property({self.key!r}, {self.kind})"


def _kind(name, schema):
    if "enum" in schema:
        return "choice"
    if name in ("sensor", "hidden", "navmesh", "shadows"):
        return "flag"
    if name in ("collision_layers", "collides_with", "layers"):
        return "layers"
    if name == "spawn":
        return "spawn"
    if name == "prefab":
        return "file"
    if name == "lod":
        return "lod"
    return "component"


def _without_text(schema):
    """A component's schema without the "or JSON text" alternative the
    engine adds for Blender's String properties."""
    options = schema.get("anyOf")
    if isinstance(options, list):
        rest = [s for s in options if s != {"type": "string"}]
        if len(rest) == 1:
            return rest[0]
    return schema


def example(schema):
    """A small value that fits `schema`: what a new component property
    starts as."""
    if not isinstance(schema, dict):
        return None
    if "const" in schema:
        return schema["const"]
    if schema.get("enum"):
        return schema["enum"][0]
    if "anyOf" in schema and schema["anyOf"]:
        return example(schema["anyOf"][0])
    kind = schema.get("type")
    if kind == "object":
        props = schema.get("properties", {})
        return {k: example(props[k]) for k in schema.get("required", []) if k in props}
    if kind == "array":
        if "prefixItems" in schema:
            return [example(s) for s in schema["prefixItems"]]
        n = schema.get("minItems", 0)
        return [example(schema.get("items", {})) for _ in range(n)]
    if kind in ("number", "integer"):
        return max(0, schema.get("minimum", 0))
    if kind == "boolean":
        return False
    if kind == "string":
        return ""
    if kind == "null":
        return None
    return {}


class Schema:
    """The properties a game reads, from its `ponko-schema.json`."""

    def __init__(self, data):
        if not isinstance(data, dict) or not isinstance(data.get("properties"), dict):
            raise ValueError("not a ponko properties schema: it has no \"properties\" object")
        self.data = data
        self.properties = {
            key: Property(key, value)
            for key, value in data["properties"].items()
            if key.startswith(PREFIX) and isinstance(value, dict)
        }

    @classmethod
    def load(cls, path):
        with open(path, encoding="utf-8") as f:
            try:
                data = json.load(f)
            except json.JSONDecodeError as e:
                raise ValueError(f"{path}: not JSON: {e}") from e
        return cls(data)

    def keys(self):
        """The `ponko:` keys, sorted: the convention's first, then the
        components (capitalized)."""
        return sorted(self.properties, key=lambda k: (self.properties[k].kind == "component", k))

    def check_property(self, key, value):
        """What is wrong with custom property `key` = `value`: a message,
        or None. Keys without the prefix are the game's own and always
        pass, except `ponko` (the same properties as one object)."""
        if key == "ponko":
            return self._check_group(_parsed(value))
        if not key.startswith(PREFIX):
            return None
        prop = self.properties.get(key)
        if prop is None:
            return unknown_key(key, list(self.properties))
        value = _parsed(value)
        error = check(prop.schema, value)
        if error is None:
            return None
        if prop.choices:
            hint = suggestions(str(value), [str(c) for c in prop.choices])
            close = f" (did you mean {json.dumps(hint[0])}?)" if hint else ""
            return f"\"{key}\" is {json.dumps(value)}; it takes one of {', '.join(map(str, prop.choices))}{close}"
        return f"\"{key}\": {error}"

    def _check_group(self, value):
        if not isinstance(value, dict):
            return "\"ponko\" holds an object of properties without the prefix"
        for name, inner in value.items():
            error = self.check_property(PREFIX + name, inner)
            if error:
                return f"in \"ponko\": {error}"
        return None

    def check_properties(self, props):
        """Every problem of one object's custom properties (a dict of name
        to value), as messages, in key order."""
        out = []
        for key in sorted(props):
            error = self.check_property(key, props[key])
            if error:
                out.append(error)
        return out


def _parsed(value):
    """Text that holds a JSON object or list is read as JSON, as the engine
    reads it (Blender's String properties hold whole components)."""
    if isinstance(value, str):
        text = value.strip()
        if text[:1] in ("{", "["):
            try:
                return json.loads(text)
            except json.JSONDecodeError:
                return value
    if isinstance(value, tuple):
        return list(value)
    return value


def unknown_key(key, known):
    name = key[len(PREFIX):]
    close = suggestions(key, known)
    if close:
        return f"\"{key}\": no property is called \"{name}\"; did you mean \"{close[0]}\"?"
    if name[:1].isupper():
        return f"\"{key}\": no reflected component is called \"{name}\" (reflect it in the game's setup)"
    return f"\"{key}\": no property is called \"{name}\""


def suggestions(name, known):
    """The names in `known` closest to `name` (ignoring case), at most
    three, when they are close enough to be what was meant. The engine's
    own rule, so the add-on suggests what the engine would."""
    lower = name.lower()
    scored = []
    for k in known:
        d = edit_distance(lower, k.lower())
        if d <= max(max(len(k), len(name)) // 3, 1) + 1:
            scored.append((d, k))
    scored = sorted(set(scored))
    out = []
    for _, k in scored:
        if k not in out:
            out.append(k)
    return out[:3]


def edit_distance(a, b):
    """Edits (insert, delete, change, swap two neighbours) between a and b."""
    rows = [[0] * (len(b) + 1) for _ in range(len(a) + 1)]
    for i in range(len(a) + 1):
        rows[i][0] = i
    for j in range(len(b) + 1):
        rows[0][j] = j
    for i in range(1, len(a) + 1):
        for j in range(1, len(b) + 1):
            cost = 0 if a[i - 1] == b[j - 1] else 1
            d = min(rows[i - 1][j] + 1, rows[i][j - 1] + 1, rows[i - 1][j - 1] + cost)
            if i > 1 and j > 1 and a[i - 1] == b[j - 2] and a[i - 2] == b[j - 1]:
                d = min(d, rows[i - 2][j - 2] + 1)
            rows[i][j] = d
    return rows[len(a)][len(b)]


def _type_ok(kind, value):
    if kind == "object":
        return isinstance(value, dict)
    if kind == "array":
        return isinstance(value, list)
    if kind == "string":
        return isinstance(value, str)
    if kind == "boolean":
        return isinstance(value, bool)
    if kind == "integer":
        return isinstance(value, int) and not isinstance(value, bool) or (
            isinstance(value, float) and value.is_integer()
        )
    if kind == "number":
        return isinstance(value, (int, float)) and not isinstance(value, bool)
    if kind == "null":
        return value is None
    return True


def check(schema, value):
    """What makes `value` not fit `schema` (the JSON Schema subset above):
    a message, or None if it fits."""
    if not isinstance(schema, dict):
        return None
    kind = schema.get("type")
    if kind is not None:
        kinds = kind if isinstance(kind, list) else [kind]
        if not any(_type_ok(k, value) for k in kinds):
            return f"{json.dumps(value)} is not {' or '.join(_a(k) for k in kinds)}"
    if "const" in schema and value != schema["const"]:
        return f"{json.dumps(value)} is not {json.dumps(schema['const'])}"
    if "enum" in schema and not any(_same(value, e) for e in schema["enum"]):
        return f"{json.dumps(value)} is not one of {', '.join(json.dumps(e) for e in schema['enum'])}"
    if "anyOf" in schema:
        errors = [check(s, value) for s in schema["anyOf"]]
        if all(errors):
            return errors[0] if len(errors) == 1 else "; or ".join(e for e in errors if e)
    if "not" in schema and check(schema["not"], value) is None:
        return f"{json.dumps(value)} is not allowed here"
    if isinstance(value, str):
        if "pattern" in schema and not re.search(schema["pattern"], value):
            return f"{json.dumps(value)} does not match {schema['pattern']}"
        if len(value) < schema.get("minLength", 0):
            return "it is empty" if not value else f"{json.dumps(value)} is too short"
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        if "minimum" in schema and value < schema["minimum"]:
            return f"{value} is below {schema['minimum']}"
        if "maximum" in schema and value > schema["maximum"]:
            return f"{value} is above {schema['maximum']}"
    if isinstance(value, list):
        if len(value) < schema.get("minItems", 0):
            return f"it has {len(value)} items, at least {schema['minItems']} wanted"
        if "maxItems" in schema and len(value) > schema["maxItems"]:
            return f"it has {len(value)} items, at most {schema['maxItems']} wanted"
        prefix = schema.get("prefixItems", [])
        for i, item in enumerate(value):
            sub = prefix[i] if i < len(prefix) else schema.get("items")
            error = check(sub, item)
            if error:
                return f"item {i}: {error}"
    if isinstance(value, dict):
        props = schema.get("properties", {})
        for key in schema.get("required", []):
            if key not in value:
                return f"\"{key}\" is missing"
        for key, item in value.items():
            if key in props:
                error = check(props[key], item)
                if error:
                    return f"\"{key}\": {error}"
            elif schema.get("additionalProperties") is False:
                close = suggestions(key, list(props))
                hint = f"; did you mean \"{close[0]}\"?" if close else ""
                return f"\"{key}\" is not a field here{hint}"
    return None


def _same(a, b):
    # JSON equality: 1 == 1.0, but true is not 1.
    if isinstance(a, bool) or isinstance(b, bool):
        return isinstance(a, bool) and isinstance(b, bool) and a == b
    return a == b


def _a(kind):
    return {"object": "an object", "array": "a list", "integer": "an integer", "null": "null"}.get(kind, f"a {kind}")
