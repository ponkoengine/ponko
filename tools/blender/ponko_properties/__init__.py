"""Ponko properties: a Blender add-on that reads a game's `ponko-schema.json`
(written by `ponko schema`) and offers the `ponko:` custom properties the game
reads, checked as you type them. See the guide's "From Blender to the game".

Install: Edit > Preferences > Add-ons > Install... and pick a zip of this
folder (or copy the folder into Blender's `scripts/addons`), then enable
"Ponko properties". It adds a "Ponko" panel to Object Properties.

The schema logic is in `schema.py`, which needs no Blender.
"""

bl_info = {
    "name": "Ponko properties",
    "description": "Offers and checks the ponko: custom properties a game reads (from its ponko-schema.json)",
    "author": "Ponko contributors",
    "version": (0, 1, 0),
    "blender": (3, 6, 0),
    "location": "Properties > Object > Ponko",
    "category": "Import-Export",
}

try:
    import bpy
    from bpy.props import EnumProperty, StringProperty
except ImportError:  # Imported outside Blender (the tests): only `schema` is used.
    bpy = None

from . import schema as ponko_schema

# The schema loaded for each file path, with the error that stopped it.
_loaded = {}


def _schema(context):
    """The schema the scene names, loaded once (Reload reads it again)."""
    path = bpy.path.abspath(context.scene.ponko_schema_path)
    if path not in _loaded:
        try:
            _loaded[path] = (ponko_schema.Schema.load(path), None)
        except (OSError, ValueError) as e:
            _loaded[path] = (None, str(e))
    return _loaded[path]


def _plain(value):
    """A custom property's value as plain Python (Blender arrays and
    groups to lists and dicts)."""
    if hasattr(value, "to_dict"):
        return value.to_dict()
    if hasattr(value, "to_list"):
        return value.to_list()
    return value


def object_problems(sch, obj):
    """What is wrong with `obj`'s `ponko:` properties."""
    props = {k: _plain(obj[k]) for k in obj.keys() if k.startswith("ponko")}
    return sch.check_properties(props)


if bpy is not None:

    def _key_items(self, context):
        sch, _ = _schema(context)
        if sch is None:
            return [("", "(no schema)", "")]
        return [(k, k, sch.properties[k].description[:200]) for k in sch.keys()]

    class PONKO_OT_add_property(bpy.types.Operator):
        """Add a ponko: property the game reads to the active object"""

        bl_idname = "ponko.add_property"
        bl_label = "Add Ponko property"
        bl_property = "key"
        bl_options = {"REGISTER", "UNDO"}

        key: EnumProperty(name="Property", items=_key_items)

        def invoke(self, context, event):
            context.window_manager.invoke_search_popup(self)
            return {"FINISHED"}

        def execute(self, context):
            sch, error = _schema(context)
            obj = context.object
            if sch is None or obj is None or not self.key:
                self.report({"ERROR"}, error or "select an object")
                return {"CANCELLED"}
            prop = sch.properties[self.key]
            if self.key not in obj.keys():
                obj[self.key] = prop.default()
                ui = obj.id_properties_ui(self.key)
                ui.update(description=prop.description[:200])
            return {"FINISHED"}

    class PONKO_OT_reload_schema(bpy.types.Operator):
        """Read the schema file again (after `ponko schema` wrote it)"""

        bl_idname = "ponko.reload_schema"
        bl_label = "Reload schema"

        def execute(self, context):
            _loaded.clear()
            sch, error = _schema(context)
            if sch is None:
                self.report({"ERROR"}, error)
                return {"CANCELLED"}
            self.report({"INFO"}, f"{len(sch.properties)} ponko: properties")
            return {"FINISHED"}

    class PONKO_OT_check_scene(bpy.types.Operator):
        """Check every object's ponko: properties, before exporting"""

        bl_idname = "ponko.check_scene"
        bl_label = "Check all objects"

        def execute(self, context):
            sch, error = _schema(context)
            if sch is None:
                self.report({"ERROR"}, error)
                return {"CANCELLED"}
            problems = [f"{o.name}: {p}" for o in bpy.data.objects for p in object_problems(sch, o)]
            for p in problems:
                print("ponko:", p)
            if problems:
                self.report({"WARNING"}, f"{len(problems)} problems (in the system console); first: {problems[0]}")
            else:
                self.report({"INFO"}, "every ponko: property fits the schema")
            return {"FINISHED"}

    class PONKO_PT_object(bpy.types.Panel):
        bl_label = "Ponko"
        bl_space_type = "PROPERTIES"
        bl_region_type = "WINDOW"
        bl_context = "object"

        def draw(self, context):
            layout = self.layout
            layout.prop(context.scene, "ponko_schema_path", text="Schema")
            row = layout.row()
            row.operator(PONKO_OT_reload_schema.bl_idname, icon="FILE_REFRESH")
            row.operator(PONKO_OT_check_scene.bl_idname, icon="CHECKMARK")
            sch, error = _schema(context)
            if sch is None:
                layout.label(text=error, icon="ERROR")
                return
            obj = context.object
            if obj is None:
                return
            layout.operator(PONKO_OT_add_property.bl_idname, icon="ADD")
            for key in sorted(k for k in obj.keys() if k.startswith("ponko")):
                row = layout.row()
                row.prop(obj, f'["{key}"]', text=key)
                problem = sch.check_property(key, _plain(obj[key]))
                if problem:
                    layout.label(text=problem, icon="ERROR")

    class glTF2ExportUserExtension:
        """Picked up by Blender's glTF exporter: prints the ponko: property
        problems of every exported object (the export still runs)."""

        def __init__(self):
            self.schema = None

        def gather_node_hook(self, gltf2_object, blender_object, export_settings):
            if self.schema is None:
                self.schema, _ = _schema(bpy.context)
            if self.schema is not None and blender_object is not None:
                for p in object_problems(self.schema, blender_object):
                    print(f"ponko: {blender_object.name}: {p}")

    _classes = (PONKO_OT_add_property, PONKO_OT_reload_schema, PONKO_OT_check_scene, PONKO_PT_object)

    def register():
        bpy.types.Scene.ponko_schema_path = StringProperty(
            name="Ponko schema",
            description="The game's ponko-schema.json (`ponko schema --out ponko-schema.json`)",
            default="//ponko-schema.json",
            subtype="FILE_PATH",
        )
        for c in _classes:
            bpy.utils.register_class(c)

    def unregister():
        for c in reversed(_classes):
            bpy.utils.unregister_class(c)
        del bpy.types.Scene.ponko_schema_path

