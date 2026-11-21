/* Ponko C ABI, version 2. C++: include ponko.hpp instead.
 *
 * Games: ponko_game_new -> settings and systems -> ponko_game_run (window)
 * or ponko_game_run_headless. Systems get a PonkoCtx*, valid until they
 * return: the world (ponko_ctx_world), clock, input, drawing, shapes, text
 * and fonts, UI widgets, sound, textures, camera, commands (*_later), events
 * (ponko_ctx_send_event / ponko_ctx_read_events), the built-in spatial
 * grid, physics and lockstep multiplayer (ponko_game_lockstep_host/join,
 * ponko_ctx_set_input / ponko_ctx_input).
 *
 * Handles are confined to the thread that runs the game, except inside
 * ponko_query_par callbacks, which run on many threads at once and may only
 * touch their chunk, ponko_ctx_dt/sim_tick/random_bits and spatial queries.
 * Non-null pointers must refer to live, correctly aligned memory of the stated
 * size; dangling/forged pointers cannot be detected. Only free owned handles,
 * exactly once. Components made from C are memcpy-safe values without
 * destructors. Built-ins: Transform2D (PonkoTransform2D, a plain value) and
 * Sprite (PonkoSprite in spawn/get/set, never a query column); with physics,
 * RigidBody and Collider (PonkoRigidBody, PonkoCollider: like sprites) and
 * Velocity (PonkoVelocity, a plain value). 3D: Transform3D and Model
 * (PonkoTransform3D, PonkoModel: plain values); with physics3d,
 * RigidBody3D, Collider3D and Character3D (like sprites) and Velocity3D
 * (PonkoVelocity3D, a plain value).
 *
 * Queries take up to 32 distinct component ids; zero ids match every entity.
 * Columns follow query order with registered-size stride; chunk memory expires
 * when the callback returns. While a query runs, every world and ctx call
 * returns PONKO_BUSY, except commands (ponko_query only) and spatial queries.
 * Callbacks must return normally: C++ exceptions and Rust panics must never
 * cross the C boundary (ponko.hpp catches them for you).
 *
 * Retained UI tree operations take a target: PONKO_UI_SCREEN for the
 * screen's own UI, or an entity with a world-space panel (ponko_ctx_create_ui_panel)
 * for that panel's UI.
 *
 * Enums (PonkoKey, PonkoMouseButton, PonkoGamepadButton, PonkoGamepadAxis,
 * PonkoControllerFamily, PonkoDeviceKind, PonkoGlyphKind,
 * PonkoTextAlign3D, PonkoBodyKind, PonkoShapeKind, PonkoShape3DKind, PonkoNetState, PonkoPhase, PonkoService, PonkoHandleKind, PonkoAssetClass, PonkoAssetOwner, PonkoResidencyKind, PonkoAssetState, PonkoAssetGroupState, PonkoAssetGroupKind, PonkoGpuTier, PonkoGpuFeature, PonkoScenarioStatus, PonkoWaterQuality, PonkoWaterReflection, PonkoHookKind, PonkoCloudQuality, PonkoJointChannel, PonkoEventTarget, and the
 * PonkoUi* layout and paint enums) are passed as uint32_t.
 * Last-error text is thread-local, borrowed until the next failed call;
 * successful calls leave it unchanged.
 *
 * ABI version: PONKO_ABI_VERSION is the version this header describes;
 * ponko_abi_version() is the version of the loaded library. A program that
 * loads the library at run time compares them at startup (ponko.hpp does this
 * when the first game or world is made).
 *
 * Constructors, for new functions: return a PonkoStatus and write the new
 * object through an out-pointer (ponko_clip_new, ponko_snapshot_stream_new,
 * ponko_interest_new, the ponko_ctx_create_* and ponko_ctx_load_* families).
 * The older constructors of standalone owned objects return the pointer, null
 * on failure with ponko_last_error() set (ponko_world_new, ponko_game_new,
 * ponko_nav_new, ponko_grid_new, ...).
 *
 * Destructors of owned objects (ponko_*_free) return void, except
 * ponko_world_free: a world can be busy (a query is running) or borrowed (it
 * belongs to a game or context), and then it is NOT freed. The returned
 * status (PONKO_BUSY) tells the caller that the handle is still live and
 * still theirs. ponko_ctx_handle_free is not a destructor of an owned object:
 * it frees an engine handle by id inside a context and returns a status like
 * the other ponko_ctx_* calls.
 *
 * Pointers and who owns them. A parameter's type says, unless the function's
 * documentation (its doc comment in crates/ponko_ffi, the ponko_ffi API docs)
 * has an "Ownership:" paragraph; scripts/check-ffi-ownership.py holds every
 * function to this:
 *   - a Ponko* object (PonkoCtx*, PonkoWorld*, a builder): borrowed for the
 *     call, the caller keeps it; a ponko_*_free takes its first one and frees it;
 *   - any const pointer (strings, arrays, descriptors, bytes, an array of
 *     strings or handles): read during the call only; what is kept is copied;
 *   - any other pointer (numbers, char buffers, value structs, void *out_*):
 *     the caller's memory, written (and maybe read) during the call only.
 * Returned pointers, pointers to pointers (T **out), callback user data
 * (void *user) and objects a function takes over always have an "Ownership:"
 * paragraph: who owns them, what frees them, how long they stay valid.
 */


#ifndef PONKO_H
#define PONKO_H

/* Generated by cbindgen. Regenerate: cargo run -p ponko_ffi --example generate_header > include/ponko.h */

#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>

#define PONKO_ABI_VERSION 2

#define PONKO_ACTION_NONE UINT32_MAX

#define PONKO_AI_EVERY_BRAIN UINT64_MAX

#define PONKO_SINCE_LAST_RUN UINT64_MAX

#define PONKO_NAV_WALL 0

#define PONKO_SYSTEM_PHYSICS 1

#define PONKO_SYSTEM_PHYSICS3D 2

#define PONKO_SYSTEM_INTERPOLATE_PHYSICS3D 3

#define PONKO_SYSTEM_REBUILD_SPATIAL_GRID 4

#define PONKO_SYSTEM_ANIMATE_SPRITES 5

#define PONKO_SYSTEM_DRAW_TILEMAPS 6

#define PONKO_SYSTEM_DRAW_SPRITES 7

#define PONKO_SYSTEM_FOLLOW_ATTACHMENTS 8

#define PONKO_SYSTEM_STEP_NAVMESH3D 9

#define PONKO_SYSTEM_UPDATE_ANIMATORS 10

#define PONKO_SYSTEM_UPDATE_PARTICLES 11

#define PONKO_SYSTEM_UPDATE_AUDIO3D 12

#define PONKO_SYSTEM_DRAW_PARTICLES 13

#define PONKO_SYSTEM_UPDATE_SKELETONS2D 14

#define PONKO_HANDLE_NONE UINT32_MAX

#define PONKO_MAX_GAMEPADS 8

#define PONKO_LEVEL_SHAPE_RECT 0

#define PONKO_LEVEL_SHAPE_ELLIPSE 1

#define PONKO_LEVEL_SHAPE_POINT 2

#define PONKO_LEVEL_SHAPE_POLYGON 3

#define PONKO_LEVEL_SHAPE_POLYLINE 4

#define PONKO_TILE_FLIP_X 1

#define PONKO_TILE_FLIP_Y 2

#define PONKO_TILE_FLIP_DIAGONAL 4

#define PONKO_PRESS_THRESHOLD 0.5

#define PONKO_QUERY_CACHE_AUTO 0

#define PONKO_QUERY_CACHE_ALWAYS 1

#define PONKO_QUERY_CACHE_NEVER 2

#define PONKO_SHADER_MATERIAL 0

#define PONKO_SHADER_EFFECT 1

#define PONKO_SHADER_COMPUTE 2

#define PONKO_SHADER_SPRITE 3

#define PONKO_SHADER_COMPILED 0

#define PONKO_SHADER_RELOADED 1

#define PONKO_SHADER_FAILED 2

#define PONKO_SHADER_RELOAD_FAILED 3

#define PONKO_SOUND_STARTED_VERSION 1

#define PONKO_BLEND_ALPHA 0

#define PONKO_BLEND_ADDITIVE 1

#define PONKO_BLEND_MULTIPLY 2

#define PONKO_BLEND_PREMULTIPLIED 3

#define PONKO_MESH_NONE UINT32_MAX

#define PONKO_TERRAIN_LODS 4

#define PONKO_TERRAIN_LAYERS 8

#define PONKO_TEXTURE_COLOR 0

#define PONKO_TEXTURE_DATA 1

#define PONKO_DECAL_NO_IMAGE UINT32_MAX

#define PONKO_TILE_EMPTY 65535

#define PONKO_UI_ACCESS_STATE_EX_VERSION 1

#define PONKO_UI_NONE UINT64_MAX

#define PONKO_UI_SCREEN UINT64_MAX

#define PONKO_UI_NO_INDEX UINT64_MAX

#define PONKO_UI_EVENT_EX_VERSION 1

#define PONKO_UI_THEME_EX_VERSION 2

#define PONKO_UI_INPUT_EX_VERSION 1

#define PONKO_DEBUG_STYLE_EX_VERSION 1

#define PONKO_COLLIDER_EX_VERSION 1

#define PONKO_QUERY_FILTER_EX_VERSION 1

#define PONKO_MATERIAL_DESC_EX_VERSION 1

#define PONKO_NAV_BAKE_SETTINGS_EX_VERSION 1

#define PONKO_LIGHTING_EX_VERSION 1

#define PONKO_POST_EX_VERSION 1

#define PONKO_PARTICLE_SETTINGS3D_EX_VERSION 1

#define PONKO_CAMERA3D_EX_VERSION 2

#define PONKO_TARGET_CAMERA_EX_VERSION 2

#define PONKO_PREDICTION_DEFAULT_CAPACITY 256

#define PONKO_CAMERA_SHAKE_VERSION 1

#define PONKO_FOLLOW_CAMERA2D_VERSION 1

#define PONKO_PIXEL_PERFECT_VERSION 1

#define PONKO_ORBIT_CAMERA_VERSION 1

#define PONKO_FIRST_PERSON_CAMERA_VERSION 1

#define PONKO_THIRD_PERSON_CAMERA_VERSION 1

#define PONKO_RENDER_LAYERS_DEFAULT 1

#define PONKO_RENDER_LAYERS_ALL UINT32_MAX

#define PONKO_VIEW2D_VERSION 1

#define PONKO_VIEW3D_VERSION 1

#define PONKO_WATER_DESC_VERSION 1

#define PONKO_WATER_MAX_WAVES 8

#define PONKO_BUOYANCY_DRAG 4.0

#define PONKO_LIVE_COUNTS_EX_VERSION 1

#define PONKO_LIGHT_BAKE_SETTINGS_VERSION 1

#define PONKO_BAKE_MODEL_VERSION 1

#define PONKO_BAKED_LIGHTING_INFO_VERSION 1

#define PONKO_SKY_KIT_VERSION 1

#define PONKO_DAY_NIGHT_VERSION 1

#define PONKO_VIDEO_STATE_VERSION 1

#define PONKO_WINDOW_NO_MONITOR UINT32_MAX

#define PONKO_WINDOW_VIDEO_MODE_EX_VERSION 1

#define PONKO_WINDOW_MODE_EX_VERSION 1

#define PONKO_WINDOW_PROPERTIES_EX_VERSION 1

#define PONKO_WINDOW_IMAGE_EX_VERSION 1

#define PONKO_WINDOW_MONITOR_EX_VERSION 1

#define PONKO_WINDOW_STATE_EX_VERSION 1

#define PONKO_WINDOW_EVENT_EX_VERSION 1

typedef enum PonkoStatus {
  PONKO_OK = 0,
  PONKO_INVALID_ARGUMENT = 1,
  PONKO_NOT_FOUND = 2,
  PONKO_BUSY = 3,
  PONKO_PANIC = 4,
  PONKO_UNSUPPORTED = 5,
} PonkoStatus;

typedef enum PonkoErrorKind {
  PONKO_ERROR_NONE = 0,
  PONKO_ERROR_NOT_FOUND = 1,
  PONKO_ERROR_IO = 2,
  PONKO_ERROR_DECODE = 3,
  PONKO_ERROR_SHADER = 4,
  PONKO_ERROR_UNSUPPORTED = 5,
  PONKO_ERROR_BAD_HANDLE = 6,
  PONKO_ERROR_JSON = 7,
  PONKO_ERROR_LIMIT = 8,
  PONKO_ERROR_INVALID = 9,
  PONKO_ERROR_OTHER = 10,
} PonkoErrorKind;

typedef enum PonkoAiStatus {
  PONKO_AI_RUNNING = 0,
  PONKO_AI_SUCCESS = 1,
  PONKO_AI_FAILURE = 2,
} PonkoAiStatus;

typedef enum PonkoAiCurve {
  PONKO_AI_CURVE_LINEAR = 0,
  PONKO_AI_CURVE_QUADRATIC = 1,
  PONKO_AI_CURVE_CUBIC = 2,
  PONKO_AI_CURVE_SQRT = 3,
  PONKO_AI_CURVE_SMOOTH = 4,
  PONKO_AI_CURVE_POWER = 5,
  PONKO_AI_CURVE_LOGISTIC = 6,
  PONKO_AI_CURVE_STEP = 7,
} PonkoAiCurve;

typedef enum PonkoAnimationCondition {
  PONKO_CONDITION_GREATER = 0,
  PONKO_CONDITION_LESS = 1,
  PONKO_CONDITION_BOOL = 2,
  PONKO_CONDITION_TRIGGER = 3,
} PonkoAnimationCondition;

typedef enum PonkoAudioRolloff {
  PONKO_ROLLOFF_LOGARITHMIC = 0,
  PONKO_ROLLOFF_LINEAR = 1,
} PonkoAudioRolloff;

typedef enum PonkoBusEffectKind {
  PONKO_BUS_EFFECT_FILTER = 0,
  PONKO_BUS_EFFECT_REVERB = 1,
  PONKO_BUS_EFFECT_COMPRESSOR = 2,
  PONKO_BUS_EFFECT_DELAY = 3,
  PONKO_BUS_EFFECT_PITCH_SHIFT = 4,
} PonkoBusEffectKind;

typedef enum PonkoFilterMode {
  PONKO_FILTER_LOW_PASS = 0,
  PONKO_FILTER_BAND_PASS = 1,
  PONKO_FILTER_HIGH_PASS = 2,
  PONKO_FILTER_NOTCH = 3,
} PonkoFilterMode;

typedef enum PonkoQuantize {
  PONKO_QUANTIZE_NOW = 0,
  PONKO_QUANTIZE_BEAT = 1,
  PONKO_QUANTIZE_BAR = 2,
} PonkoQuantize;

typedef enum PonkoLobbyEventKind {
  PONKO_LOBBY_CONNECTED = 0,
  PONKO_LOBBY_JOINED = 1,
  PONKO_LOBBY_MEMBER_JOINED = 2,
  PONKO_LOBBY_MEMBER_LEFT = 3,
  PONKO_LOBBY_HOST_CHANGED = 4,
  PONKO_LOBBY_META_CHANGED = 5,
  PONKO_LOBBY_LEFT = 6,
  PONKO_LOBBY_QUEUED = 7,
  PONKO_LOBBY_UNQUEUED = 8,
  PONKO_LOBBY_REFUSED = 9,
  PONKO_LOBBY_RELAYED = 10,
  PONKO_LOBBY_DISCONNECTED = 11,
} PonkoLobbyEventKind;

typedef enum PonkoLane {
  PONKO_LANE_RELIABLE = 0,
  PONKO_LANE_UNRELIABLE = 1,
  PONKO_LANE_RELIABLE_UNORDERED = 2,
} PonkoLane;

typedef enum PonkoLobbyError {
  PONKO_LOBBY_NO_ERROR = 0,
  PONKO_LOBBY_ROOM_NOT_FOUND = 1,
  PONKO_LOBBY_ROOM_FULL = 2,
  PONKO_LOBBY_ALREADY_IN_ROOM = 3,
  PONKO_LOBBY_ALREADY_QUEUED = 4,
  PONKO_LOBBY_NOT_IN_ROOM = 5,
  PONKO_LOBBY_NOT_HOST = 6,
  PONKO_LOBBY_RATE_LIMITED = 7,
  PONKO_LOBBY_TOO_LARGE = 8,
  PONKO_LOBBY_BAD_SIZE = 9,
  PONKO_LOBBY_SERVER_FULL = 10,
  PONKO_LOBBY_TOO_MANY_CONNECTIONS = 11,
  PONKO_LOBBY_BAD_REQUEST = 12,
  PONKO_LOBBY_OTHER_ERROR = 13,
} PonkoLobbyError;

typedef enum PonkoLobbyLeaveReason {
  PONKO_LOBBY_LEFT_NONE = 0,
  PONKO_LOBBY_LEFT_REQUESTED = 1,
  PONKO_LOBBY_LEFT_ROOM_CLOSED = 2,
  PONKO_LOBBY_LEFT_IDLE = 3,
  PONKO_LOBBY_LEFT_SERVER_STOPPING = 4,
  PONKO_LOBBY_LEFT_OTHER = 5,
} PonkoLobbyLeaveReason;

typedef enum PonkoMessageEventKind {
  PONKO_MESSAGE_CONNECTED = 0,
  PONKO_MESSAGE_DISCONNECTED = 1,
  PONKO_MESSAGE = 2,
} PonkoMessageEventKind;

typedef enum PonkoJointMotionKind {
  PONKO_JOINT_FREE = 0,
  PONKO_JOINT_LOCKED = 1,
  PONKO_JOINT_LIMITED = 2,
} PonkoJointMotionKind;

typedef enum PonkoAuthorityCheck {
  PONKO_AUTHORITY_ACCEPT = 0,
  PONKO_AUTHORITY_STALE = 1,
  PONKO_AUTHORITY_WRONG_OWNER = 2,
  PONKO_AUTHORITY_AHEAD = 3,
} PonkoAuthorityCheck;

typedef enum PonkoSnapshotPush {
  PONKO_SNAPSHOT_ADDED = 0,
  PONKO_SNAPSHOT_DUPLICATE = 1,
  PONKO_SNAPSHOT_TOO_OLD = 2,
  PONKO_SNAPSHOT_INVALID = 3,
} PonkoSnapshotPush;

typedef enum PonkoSnapshotState {
  PONKO_SNAPSHOT_INTERPOLATED = 0,
  PONKO_SNAPSHOT_EXTRAPOLATED = 1,
  PONKO_SNAPSHOT_HELD = 2,
} PonkoSnapshotState;

typedef enum PonkoInterestChangeKind {
  PONKO_INTEREST_ENTERED = 0,
  PONKO_INTEREST_LEFT = 1,
} PonkoInterestChangeKind;

typedef enum PonkoReplicatedType {
  PONKO_REPLICATED_F32,
  PONKO_REPLICATED_I32,
  PONKO_REPLICATED_U32,
  PONKO_REPLICATED_I16,
  PONKO_REPLICATED_U16,
  PONKO_REPLICATED_I8,
  PONKO_REPLICATED_U8,
  PONKO_REPLICATED_BOOL,
} PonkoReplicatedType;

typedef enum PonkoFieldQuantize {
  PONKO_FIELD_EXACT,
  PONKO_FIELD_RANGE,
  PONKO_FIELD_ANGLE,
  PONKO_FIELD_BITS,
} PonkoFieldQuantize;

typedef enum PonkoReplicationRole {
  PONKO_REPLICATION_OFF,
  PONKO_REPLICATION_SERVER,
  PONKO_REPLICATION_CLIENT,
} PonkoReplicationRole;

typedef enum PonkoDrawSpace {
  PONKO_DRAW_SPACE_WORLD = 0,
  PONKO_DRAW_SPACE_SCREEN = 1,
} PonkoDrawSpace;

typedef enum PonkoUiLive {
  PONKO_UI_LIVE_OFF,
  PONKO_UI_LIVE_POLITE,
  PONKO_UI_LIVE_ASSERTIVE,
} PonkoUiLive;

typedef enum PonkoUiRole {
  PONKO_UI_ROLE_GROUP,
  PONKO_UI_ROLE_DIALOG,
  PONKO_UI_ROLE_LABEL,
  PONKO_UI_ROLE_BUTTON,
  PONKO_UI_ROLE_IMAGE,
  PONKO_UI_ROLE_SCROLL_VIEW,
  PONKO_UI_ROLE_TEXT_INPUT,
  PONKO_UI_ROLE_CHECKBOX,
  PONKO_UI_ROLE_SLIDER,
  PONKO_UI_ROLE_COMBO_BOX,
  PONKO_UI_ROLE_TAB_LIST,
  PONKO_UI_ROLE_TAB,
  PONKO_UI_ROLE_TAB_PANEL,
  PONKO_UI_ROLE_RADIO_GROUP,
  PONKO_UI_ROLE_RADIO_BUTTON,
  PONKO_UI_ROLE_PROGRESS_BAR,
  PONKO_UI_ROLE_SPINNER,
  PONKO_UI_ROLE_SPIN_BUTTON,
  PONKO_UI_ROLE_LIST,
  PONKO_UI_ROLE_LIST_ITEM,
  PONKO_UI_ROLE_GRID,
  PONKO_UI_ROLE_GRID_CELL,
  PONKO_UI_ROLE_MENU,
  PONKO_UI_ROLE_MENU_ITEM,
  PONKO_UI_ROLE_SPLITTER,
} PonkoUiRole;

typedef enum PonkoUiAccessValueKind {
  PONKO_UI_ACCESS_VALUE_NONE,
  PONKO_UI_ACCESS_VALUE_TEXT,
  PONKO_UI_ACCESS_VALUE_CHECKED,
  PONKO_UI_ACCESS_VALUE_RANGE,
  PONKO_UI_ACCESS_VALUE_NUMBER,
} PonkoUiAccessValueKind;

typedef enum PonkoUiExpanded {
  PONKO_UI_EXPANDED_NONE,
  PONKO_UI_EXPANDED_CLOSED,
  PONKO_UI_EXPANDED_OPEN,
} PonkoUiExpanded;

typedef enum PonkoUiEventKind {
  PONKO_UI_EVENT_CLICKED,
  PONKO_UI_EVENT_CHANGED,
  PONKO_UI_EVENT_SUBMITTED,
  PONKO_UI_EVENT_SCREEN_POPPED,
  PONKO_UI_EVENT_CHOSEN,
  PONKO_UI_EVENT_DROPPED,
  PONKO_UI_EVENT_ROWS_SHOWN,
  PONKO_UI_EVENT_ANIMATION_DONE,
} PonkoUiEventKind;

typedef enum PonkoUiAxis {
  PONKO_UI_AXIS_HORIZONTAL,
  PONKO_UI_AXIS_VERTICAL,
} PonkoUiAxis;

enum PonkoStringArgKind
#ifdef __cplusplus
  : uint32_t
#endif // __cplusplus
 {
  PONKO_STRING_ARG_TEXT = 0,
  PONKO_STRING_ARG_INT = 1,
  PONKO_STRING_ARG_NUMBER = 2,
};
#ifndef __cplusplus
typedef uint32_t PonkoStringArgKind;
#endif // __cplusplus

enum PonkoPluralCategory
#ifdef __cplusplus
  : uint32_t
#endif // __cplusplus
 {
  PONKO_PLURAL_ZERO = 0,
  PONKO_PLURAL_ONE = 1,
  PONKO_PLURAL_TWO = 2,
  PONKO_PLURAL_FEW = 3,
  PONKO_PLURAL_MANY = 4,
  PONKO_PLURAL_OTHER = 5,
};
#ifndef __cplusplus
typedef uint32_t PonkoPluralCategory;
#endif // __cplusplus

enum PonkoTextAlign3D
#ifdef __cplusplus
  : uint32_t
#endif // __cplusplus
 {
  PONKO_TEXT_ALIGN3D_LEFT = 0,
  PONKO_TEXT_ALIGN3D_CENTER = 1,
  PONKO_TEXT_ALIGN3D_RIGHT = 2,
};
#ifndef __cplusplus
typedef uint32_t PonkoTextAlign3D;
#endif // __cplusplus

enum PonkoActionSource
#ifdef __cplusplus
  : uint32_t
#endif // __cplusplus
 {
  PONKO_ACTION_KEY = 0,
  PONKO_ACTION_MOUSE = 1,
  PONKO_ACTION_GAMEPAD_BUTTON = 2,
  PONKO_ACTION_KEYS = 3,
  PONKO_ACTION_GAMEPAD_AXIS = 4,
  PONKO_ACTION_KEYS2 = 5,
  PONKO_ACTION_GAMEPAD_STICK = 6,
  PONKO_ACTION_GESTURE = 7,
  PONKO_ACTION_GESTURE_AXIS = 8,
  PONKO_ACTION_DRAG = 9,
};
#ifndef __cplusplus
typedef uint32_t PonkoActionSource;
#endif // __cplusplus

typedef enum PonkoHandleKind {
  PONKO_HANDLE_TEXTURE = 0,
  PONKO_HANDLE_SOUND = 1,
  PONKO_HANDLE_FONT = 2,
  PONKO_HANDLE_MESH = 3,
  PONKO_HANDLE_MATERIAL = 4,
  PONKO_HANDLE_SHADER = 5,
  PONKO_HANDLE_COMPUTE = 6,
  PONKO_HANDLE_EFFECT = 7,
  PONKO_HANDLE_GPU_BUFFER = 8,
  PONKO_HANDLE_INSTANCE_BATCH = 9,
  PONKO_HANDLE_TEXTURE_ARRAY = 10,
  PONKO_HANDLE_SPRITE_SHADER = 11,
  PONKO_HANDLE_SPRITE_MATERIAL = 12,
  PONKO_HANDLE_RENDER_TARGET = 13,
} PonkoHandleKind;

typedef enum PonkoAssetClass {
  PONKO_ASSET_CLASS_TEXTURE = 0,
  PONKO_ASSET_CLASS_RENDER_TARGET = 1,
  PONKO_ASSET_CLASS_TEXTURE_ARRAY = 2,
  PONKO_ASSET_CLASS_MATERIAL = 3,
  PONKO_ASSET_CLASS_SHADER = 4,
  PONKO_ASSET_CLASS_MESH = 5,
  PONKO_ASSET_CLASS_GPU_BUFFER = 6,
  PONKO_ASSET_CLASS_EFFECT = 7,
  PONKO_ASSET_CLASS_COMPUTE = 8,
  PONKO_ASSET_CLASS_SOUND = 9,
} PonkoAssetClass;

typedef enum PonkoAssetOwner {
  PONKO_ASSET_OWNER_CODE = 0,
  PONKO_ASSET_OWNER_FILE = 1,
  PONKO_ASSET_OWNER_ENGINE = 2,
} PonkoAssetOwner;

typedef enum PonkoResidencyKind {
  PONKO_RESIDENCY_MESH = 0,
  PONKO_RESIDENCY_TEXTURE = 1,
} PonkoResidencyKind;

typedef enum PonkoAssetState {
  PONKO_ASSET_UNKNOWN = 0,
  PONKO_ASSET_LOADING = 1,
  PONKO_ASSET_UPLOADING = 2,
  PONKO_ASSET_STREAMING = 3,
  PONKO_ASSET_RESIDENT = 4,
  PONKO_ASSET_EVICTED = 5,
  PONKO_ASSET_FAILED = 6,
} PonkoAssetState;

typedef enum PonkoAssetGroupState {
  PONKO_GROUP_LOADING = 0,
  PONKO_GROUP_READY = 1,
  PONKO_GROUP_FAILED = 2,
  PONKO_GROUP_CANCELLED = 3,
} PonkoAssetGroupState;

typedef enum PonkoAssetGroupKind {
  PONKO_GROUP_TEXTURE = 0,
  PONKO_GROUP_STREAMED_TEXTURE = 1,
  PONKO_GROUP_MESH = 2,
  PONKO_GROUP_SOUND = 3,
  PONKO_GROUP_BYTES = 4,
} PonkoAssetGroupKind;

typedef enum PonkoKey {
  PONKO_KEY_A,
  PONKO_KEY_B,
  PONKO_KEY_C,
  PONKO_KEY_D,
  PONKO_KEY_E,
  PONKO_KEY_F,
  PONKO_KEY_G,
  PONKO_KEY_H,
  PONKO_KEY_I,
  PONKO_KEY_J,
  PONKO_KEY_K,
  PONKO_KEY_L,
  PONKO_KEY_M,
  PONKO_KEY_N,
  PONKO_KEY_O,
  PONKO_KEY_P,
  PONKO_KEY_Q,
  PONKO_KEY_R,
  PONKO_KEY_S,
  PONKO_KEY_T,
  PONKO_KEY_U,
  PONKO_KEY_V,
  PONKO_KEY_W,
  PONKO_KEY_X,
  PONKO_KEY_Y,
  PONKO_KEY_Z,
  PONKO_KEY_NUM_0,
  PONKO_KEY_NUM_1,
  PONKO_KEY_NUM_2,
  PONKO_KEY_NUM_3,
  PONKO_KEY_NUM_4,
  PONKO_KEY_NUM_5,
  PONKO_KEY_NUM_6,
  PONKO_KEY_NUM_7,
  PONKO_KEY_NUM_8,
  PONKO_KEY_NUM_9,
  PONKO_KEY_F1,
  PONKO_KEY_F2,
  PONKO_KEY_F3,
  PONKO_KEY_F4,
  PONKO_KEY_F5,
  PONKO_KEY_F6,
  PONKO_KEY_F7,
  PONKO_KEY_F8,
  PONKO_KEY_F9,
  PONKO_KEY_F10,
  PONKO_KEY_F11,
  PONKO_KEY_F12,
  PONKO_KEY_UP,
  PONKO_KEY_DOWN,
  PONKO_KEY_LEFT,
  PONKO_KEY_RIGHT,
  PONKO_KEY_SPACE,
  PONKO_KEY_ENTER,
  PONKO_KEY_ESCAPE,
  PONKO_KEY_TAB,
  PONKO_KEY_BACKSPACE,
  PONKO_KEY_CAPS_LOCK,
  PONKO_KEY_INSERT,
  PONKO_KEY_DELETE,
  PONKO_KEY_HOME,
  PONKO_KEY_END,
  PONKO_KEY_PAGE_UP,
  PONKO_KEY_PAGE_DOWN,
  PONKO_KEY_LEFT_SHIFT,
  PONKO_KEY_RIGHT_SHIFT,
  PONKO_KEY_LEFT_CTRL,
  PONKO_KEY_RIGHT_CTRL,
  PONKO_KEY_LEFT_ALT,
  PONKO_KEY_RIGHT_ALT,
  PONKO_KEY_LEFT_SUPER,
  PONKO_KEY_RIGHT_SUPER,
  PONKO_KEY_MINUS,
  PONKO_KEY_EQUAL,
  PONKO_KEY_LEFT_BRACKET,
  PONKO_KEY_RIGHT_BRACKET,
  PONKO_KEY_BACKSLASH,
  PONKO_KEY_SEMICOLON,
  PONKO_KEY_APOSTROPHE,
  PONKO_KEY_BACKQUOTE,
  PONKO_KEY_COMMA,
  PONKO_KEY_PERIOD,
  PONKO_KEY_SLASH,
  PONKO_KEY_NUMPAD_0,
  PONKO_KEY_NUMPAD_1,
  PONKO_KEY_NUMPAD_2,
  PONKO_KEY_NUMPAD_3,
  PONKO_KEY_NUMPAD_4,
  PONKO_KEY_NUMPAD_5,
  PONKO_KEY_NUMPAD_6,
  PONKO_KEY_NUMPAD_7,
  PONKO_KEY_NUMPAD_8,
  PONKO_KEY_NUMPAD_9,
  PONKO_KEY_NUMPAD_ADD,
  PONKO_KEY_NUMPAD_SUBTRACT,
  PONKO_KEY_NUMPAD_MULTIPLY,
  PONKO_KEY_NUMPAD_DIVIDE,
  PONKO_KEY_NUMPAD_DECIMAL,
  PONKO_KEY_NUMPAD_ENTER,
} PonkoKey;

typedef enum PonkoMouseButton {
  PONKO_MOUSE_LEFT,
  PONKO_MOUSE_RIGHT,
  PONKO_MOUSE_MIDDLE,
  PONKO_MOUSE_BACK,
  PONKO_MOUSE_FORWARD,
} PonkoMouseButton;

typedef enum PonkoGamepadButton {
  PONKO_GAMEPAD_SOUTH,
  PONKO_GAMEPAD_EAST,
  PONKO_GAMEPAD_NORTH,
  PONKO_GAMEPAD_WEST,
  PONKO_GAMEPAD_LEFT_BUMPER,
  PONKO_GAMEPAD_RIGHT_BUMPER,
  PONKO_GAMEPAD_LEFT_TRIGGER,
  PONKO_GAMEPAD_RIGHT_TRIGGER,
  PONKO_GAMEPAD_SELECT,
  PONKO_GAMEPAD_START,
  PONKO_GAMEPAD_LEFT_STICK,
  PONKO_GAMEPAD_RIGHT_STICK,
  PONKO_GAMEPAD_DPAD_UP,
  PONKO_GAMEPAD_DPAD_DOWN,
  PONKO_GAMEPAD_DPAD_LEFT,
  PONKO_GAMEPAD_DPAD_RIGHT,
} PonkoGamepadButton;

typedef enum PonkoGamepadAxis {
  PONKO_GAMEPAD_LEFT_STICK_X,
  PONKO_GAMEPAD_LEFT_STICK_Y,
  PONKO_GAMEPAD_RIGHT_STICK_X,
  PONKO_GAMEPAD_RIGHT_STICK_Y,
  PONKO_GAMEPAD_LEFT_TRIGGER_AXIS,
  PONKO_GAMEPAD_RIGHT_TRIGGER_AXIS,
} PonkoGamepadAxis;

enum PonkoControllerFamily
#ifdef __cplusplus
  : uint32_t
#endif // __cplusplus
 {
  PONKO_FAMILY_GENERIC = 0,
  PONKO_FAMILY_XBOX = 1,
  PONKO_FAMILY_PLAYSTATION = 2,
  PONKO_FAMILY_NINTENDO = 3,
};
#ifndef __cplusplus
typedef uint32_t PonkoControllerFamily;
#endif // __cplusplus

enum PonkoDeviceKind
#ifdef __cplusplus
  : uint32_t
#endif // __cplusplus
 {
  PONKO_DEVICE_KEYBOARD_MOUSE = 0,
  PONKO_DEVICE_GAMEPAD = 1,
};
#ifndef __cplusplus
typedef uint32_t PonkoDeviceKind;
#endif // __cplusplus

enum PonkoGlyphKind
#ifdef __cplusplus
  : uint32_t
#endif // __cplusplus
 {
  PONKO_GLYPH_KEY = 0,
  PONKO_GLYPH_MOUSE = 1,
  PONKO_GLYPH_BUTTON = 2,
  PONKO_GLYPH_STICK = 3,
  PONKO_GLYPH_AXIS = 4,
};
#ifndef __cplusplus
typedef uint32_t PonkoGlyphKind;
#endif // __cplusplus

enum PonkoFieldType
#ifdef __cplusplus
  : uint32_t
#endif // __cplusplus
 {
  PONKO_FIELD_F32 = 0,
  PONKO_FIELD_F64 = 1,
  PONKO_FIELD_I8 = 2,
  PONKO_FIELD_I16 = 3,
  PONKO_FIELD_I32 = 4,
  PONKO_FIELD_I64 = 5,
  PONKO_FIELD_U8 = 6,
  PONKO_FIELD_U16 = 7,
  PONKO_FIELD_U32 = 8,
  PONKO_FIELD_U64 = 9,
  PONKO_FIELD_BOOL = 10,
  PONKO_FIELD_VEC2 = 11,
  PONKO_FIELD_COLOR = 12,
  PONKO_FIELD_ENTITY = 13,
};
#ifndef __cplusplus
typedef uint32_t PonkoFieldType;
#endif // __cplusplus

typedef enum PonkoBodyKind {
  PONKO_BODY_DYNAMIC,
  PONKO_BODY_FIXED,
  PONKO_BODY_KINEMATIC,
} PonkoBodyKind;

typedef enum PonkoPhase {
  PONKO_PHASE_INPUT,
  PONKO_PHASE_BEFORE_PHYSICS,
  PONKO_PHASE_PHYSICS,
  PONKO_PHASE_AFTER_PHYSICS,
  PONKO_PHASE_SIM_LAST,
  PONKO_PHASE_FRAME,
  PONKO_PHASE_ANIMATE,
  PONKO_PHASE_LATE,
  PONKO_PHASE_DRAW,
} PonkoPhase;

enum PonkoService
#ifdef __cplusplus
  : uint32_t
#endif // __cplusplus
 {
  PONKO_SERVICE_INPUT = 0,
  PONKO_SERVICE_TIME = 1,
  PONKO_SERVICE_RANDOM = 2,
  PONKO_SERVICE_GFX = 3,
  PONKO_SERVICE_AUDIO = 4,
  PONKO_SERVICE_ASSETS = 5,
  PONKO_SERVICE_UI = 6,
  PONKO_SERVICE_PERF = 7,
  PONKO_SERVICE_CAMERA = 8,
  PONKO_SERVICE_SPATIAL = 9,
  PONKO_SERVICE_PHYSICS = 10,
  PONKO_SERVICE_PHYSICS3D = 11,
  PONKO_SERVICE_NET = 12,
};
#ifndef __cplusplus
typedef uint32_t PonkoService;
#endif // __cplusplus

typedef enum PonkoShapeKind {
  PONKO_SHAPE_CIRCLE,
  PONKO_SHAPE_RECT,
  PONKO_SHAPE_CAPSULE,
  PONKO_SHAPE_POLYGON,
} PonkoShapeKind;

typedef enum PonkoNetState {
  PONKO_NET_OFFLINE,
  PONKO_NET_CONNECTING,
  PONKO_NET_RUNNING,
  PONKO_NET_LOST,
} PonkoNetState;

typedef enum PonkoShape3DKind {
  PONKO_SHAPE3D_SPHERE,
  PONKO_SHAPE3D_BOX,
  PONKO_SHAPE3D_CAPSULE,
  PONKO_SHAPE3D_MESH,
  PONKO_SHAPE3D_SCALED_MESH,
} PonkoShape3DKind;

typedef enum PonkoParticleShape3D {
  PONKO_PARTICLES_POINT,
  PONKO_PARTICLES_SPHERE,
  PONKO_PARTICLES_CONE,
  PONKO_PARTICLES_BOX,
} PonkoParticleShape3D;

typedef enum PonkoParticleBlend3D {
  PONKO_PARTICLES_ALPHA,
  PONKO_PARTICLES_ADDITIVE,
} PonkoParticleBlend3D;

typedef enum PonkoStencilCompare {
  PONKO_STENCIL_NEVER,
  PONKO_STENCIL_LESS,
  PONKO_STENCIL_EQUAL,
  PONKO_STENCIL_LESS_EQUAL,
  PONKO_STENCIL_GREATER,
  PONKO_STENCIL_NOT_EQUAL,
  PONKO_STENCIL_GREATER_EQUAL,
  PONKO_STENCIL_ALWAYS,
} PonkoStencilCompare;

typedef enum PonkoStencilOp {
  PONKO_STENCIL_KEEP,
  PONKO_STENCIL_ZERO,
  PONKO_STENCIL_REPLACE,
  PONKO_STENCIL_INVERT,
  PONKO_STENCIL_INCREMENT_CLAMP,
  PONKO_STENCIL_DECREMENT_CLAMP,
  PONKO_STENCIL_INCREMENT_WRAP,
  PONKO_STENCIL_DECREMENT_WRAP,
} PonkoStencilOp;

typedef enum PonkoParticleSpace3D {
  PONKO_PARTICLES_WORLD,
  PONKO_PARTICLES_LOCAL,
} PonkoParticleSpace3D;

typedef enum PonkoParticleShape2D {
  PONKO_PARTICLES2D_POINT,
  PONKO_PARTICLES2D_CIRCLE,
  PONKO_PARTICLES2D_CONE,
  PONKO_PARTICLES2D_RECT,
} PonkoParticleShape2D;

typedef enum PonkoLayerBlend {
  PONKO_LAYER_OVERRIDE,
  PONKO_LAYER_ADDITIVE,
} PonkoLayerBlend;

typedef enum PonkoRootMotion {
  PONKO_ROOT_MOTION_OFF,
  PONKO_ROOT_MOTION_EXTRACT,
  PONKO_ROOT_MOTION_APPLY,
} PonkoRootMotion;

typedef enum PonkoUiDisplay {
  PONKO_UI_DISPLAY_FLEX,
  PONKO_UI_DISPLAY_GRID,
  PONKO_UI_DISPLAY_NONE,
} PonkoUiDisplay;

typedef enum PonkoUiPosition {
  PONKO_UI_POSITION_STATIC,
  PONKO_UI_POSITION_RELATIVE,
  PONKO_UI_POSITION_ABSOLUTE,
  PONKO_UI_POSITION_FIXED,
} PonkoUiPosition;

typedef enum PonkoUiFlexDirection {
  PONKO_UI_FLEX_ROW,
  PONKO_UI_FLEX_COLUMN,
  PONKO_UI_FLEX_ROW_REVERSE,
  PONKO_UI_FLEX_COLUMN_REVERSE,
} PonkoUiFlexDirection;

typedef enum PonkoUiFlexWrap {
  PONKO_UI_FLEX_NO_WRAP,
  PONKO_UI_FLEX_WRAP,
  PONKO_UI_FLEX_WRAP_REVERSE,
} PonkoUiFlexWrap;

typedef enum PonkoUiAlign {
  PONKO_UI_ALIGN_NORMAL,
  PONKO_UI_ALIGN_START,
  PONKO_UI_ALIGN_END,
  PONKO_UI_ALIGN_FLEX_START,
  PONKO_UI_ALIGN_FLEX_END,
  PONKO_UI_ALIGN_CENTER,
  PONKO_UI_ALIGN_BASELINE,
  PONKO_UI_ALIGN_STRETCH,
} PonkoUiAlign;

typedef enum PonkoUiAlignContent {
  PONKO_UI_ALIGN_CONTENT_NORMAL,
  PONKO_UI_ALIGN_CONTENT_START,
  PONKO_UI_ALIGN_CONTENT_END,
  PONKO_UI_ALIGN_CONTENT_FLEX_START,
  PONKO_UI_ALIGN_CONTENT_FLEX_END,
  PONKO_UI_ALIGN_CONTENT_CENTER,
  PONKO_UI_ALIGN_CONTENT_STRETCH,
  PONKO_UI_ALIGN_CONTENT_SPACE_BETWEEN,
  PONKO_UI_ALIGN_CONTENT_SPACE_EVENLY,
  PONKO_UI_ALIGN_CONTENT_SPACE_AROUND,
} PonkoUiAlignContent;

typedef enum PonkoUiScreenMode {
  PONKO_UI_SCREEN_MODE_MODELESS,
  PONKO_UI_SCREEN_MODE_MODAL,
} PonkoUiScreenMode;

typedef enum PonkoUiLengthKind {
  PONKO_UI_LENGTH_AUTO,
  PONKO_UI_LENGTH_POINTS,
  PONKO_UI_LENGTH_PERCENT,
} PonkoUiLengthKind;

typedef enum PonkoUiTrackKind {
  PONKO_UI_TRACK_AUTO,
  PONKO_UI_TRACK_POINTS,
  PONKO_UI_TRACK_PERCENT,
  PONKO_UI_TRACK_FR,
  PONKO_UI_TRACK_MIN_CONTENT,
  PONKO_UI_TRACK_MAX_CONTENT,
} PonkoUiTrackKind;

typedef enum PonkoUiGridLineKind {
  PONKO_UI_GRID_AUTO,
  PONKO_UI_GRID_LINE,
  PONKO_UI_GRID_SPAN,
} PonkoUiGridLineKind;

typedef enum PonkoUiTextAlign {
  PONKO_UI_TEXT_ALIGN_START,
  PONKO_UI_TEXT_ALIGN_CENTER,
  PONKO_UI_TEXT_ALIGN_END,
} PonkoUiTextAlign;

typedef enum PonkoUiAccessActionKind {
  PONKO_UI_ACCESS_ACTION_FOCUS,
  PONKO_UI_ACCESS_ACTION_ACTIVATE,
  PONKO_UI_ACCESS_ACTION_SET_VALUE,
  PONKO_UI_ACCESS_ACTION_INCREMENT,
  PONKO_UI_ACCESS_ACTION_DECREMENT,
  PONKO_UI_ACCESS_ACTION_SET_TEXT,
} PonkoUiAccessActionKind;

typedef enum PonkoGpuTier {
  PONKO_GPU_TIER_WEBGL2 = 0,
  PONKO_GPU_TIER_BASELINE = 1,
  PONKO_GPU_TIER_DESKTOP = 2,
} PonkoGpuTier;

typedef enum PonkoGpuFeature {
  PONKO_GPU_FEATURE_TIMESTAMPS = 0,
  PONKO_GPU_FEATURE_COMPUTE = 1,
  PONKO_GPU_FEATURE_STORAGE_BUFFERS = 2,
  PONKO_GPU_FEATURE_COMPUTE_CULLING = 3,
  PONKO_GPU_FEATURE_GPU_SKINNING = 4,
  PONKO_GPU_FEATURE_OCCLUSION_CULLING = 5,
  PONKO_GPU_FEATURE_TEMPORAL_ANTIALIASING = 6,
  PONKO_GPU_FEATURE_AMBIENT_OCCLUSION = 7,
  PONKO_GPU_FEATURE_CONTACT_SHADOWS = 8,
  PONKO_GPU_FEATURE_SCREEN_SPACE_REFLECTIONS = 9,
  PONKO_GPU_FEATURE_GLOBAL_ILLUMINATION = 10,
  PONKO_GPU_FEATURE_DECALS = 11,
  PONKO_GPU_FEATURE_MULTISAMPLING = 12,
} PonkoGpuFeature;

enum PonkoScenarioStatus
#ifdef __cplusplus
  : uint32_t
#endif // __cplusplus
 {
  PONKO_SCENARIO_PENDING = 0,
  PONKO_SCENARIO_RUNNING = 1,
  PONKO_SCENARIO_PASSED = 2,
  PONKO_SCENARIO_FAILED = 3,
  PONKO_SCENARIO_CANCELLED = 4,
};
#ifndef __cplusplus
typedef uint32_t PonkoScenarioStatus;
#endif // __cplusplus

enum PonkoTextShaping
#ifdef __cplusplus
  : uint32_t
#endif // __cplusplus
 {
  PONKO_TEXT_SHAPING_AUTO = 0,
  PONKO_TEXT_SHAPING_ALWAYS = 1,
};
#ifndef __cplusplus
typedef uint32_t PonkoTextShaping;
#endif // __cplusplus

enum PonkoImeEventKind
#ifdef __cplusplus
  : uint32_t
#endif // __cplusplus
 {
  PONKO_IME_ENABLED = 0,
  PONKO_IME_PREEDIT = 1,
  PONKO_IME_COMMIT = 2,
  PONKO_IME_DISABLED = 3,
};
#ifndef __cplusplus
typedef uint32_t PonkoImeEventKind;
#endif // __cplusplus

typedef enum PonkoHumanBone {
  PONKO_HUMAN_BONE_ROOT = 0,
  PONKO_HUMAN_BONE_HIPS = 1,
  PONKO_HUMAN_BONE_SPINE = 2,
  PONKO_HUMAN_BONE_CHEST = 3,
  PONKO_HUMAN_BONE_UPPER_CHEST = 4,
  PONKO_HUMAN_BONE_NECK = 5,
  PONKO_HUMAN_BONE_HEAD = 6,
  PONKO_HUMAN_BONE_LEFT_SHOULDER = 7,
  PONKO_HUMAN_BONE_LEFT_UPPER_ARM = 8,
  PONKO_HUMAN_BONE_LEFT_LOWER_ARM = 9,
  PONKO_HUMAN_BONE_LEFT_HAND = 10,
  PONKO_HUMAN_BONE_RIGHT_SHOULDER = 11,
  PONKO_HUMAN_BONE_RIGHT_UPPER_ARM = 12,
  PONKO_HUMAN_BONE_RIGHT_LOWER_ARM = 13,
  PONKO_HUMAN_BONE_RIGHT_HAND = 14,
  PONKO_HUMAN_BONE_LEFT_UPPER_LEG = 15,
  PONKO_HUMAN_BONE_LEFT_LOWER_LEG = 16,
  PONKO_HUMAN_BONE_LEFT_FOOT = 17,
  PONKO_HUMAN_BONE_LEFT_TOES = 18,
  PONKO_HUMAN_BONE_RIGHT_UPPER_LEG = 19,
  PONKO_HUMAN_BONE_RIGHT_LOWER_LEG = 20,
  PONKO_HUMAN_BONE_RIGHT_FOOT = 21,
  PONKO_HUMAN_BONE_RIGHT_TOES = 22,
  PONKO_HUMAN_BONE_LEFT_THUMB_PROXIMAL = 23,
  PONKO_HUMAN_BONE_LEFT_THUMB_INTERMEDIATE = 24,
  PONKO_HUMAN_BONE_LEFT_THUMB_DISTAL = 25,
  PONKO_HUMAN_BONE_LEFT_INDEX_PROXIMAL = 26,
  PONKO_HUMAN_BONE_LEFT_INDEX_INTERMEDIATE = 27,
  PONKO_HUMAN_BONE_LEFT_INDEX_DISTAL = 28,
  PONKO_HUMAN_BONE_LEFT_MIDDLE_PROXIMAL = 29,
  PONKO_HUMAN_BONE_LEFT_MIDDLE_INTERMEDIATE = 30,
  PONKO_HUMAN_BONE_LEFT_MIDDLE_DISTAL = 31,
  PONKO_HUMAN_BONE_LEFT_RING_PROXIMAL = 32,
  PONKO_HUMAN_BONE_LEFT_RING_INTERMEDIATE = 33,
  PONKO_HUMAN_BONE_LEFT_RING_DISTAL = 34,
  PONKO_HUMAN_BONE_LEFT_LITTLE_PROXIMAL = 35,
  PONKO_HUMAN_BONE_LEFT_LITTLE_INTERMEDIATE = 36,
  PONKO_HUMAN_BONE_LEFT_LITTLE_DISTAL = 37,
  PONKO_HUMAN_BONE_RIGHT_THUMB_PROXIMAL = 38,
  PONKO_HUMAN_BONE_RIGHT_THUMB_INTERMEDIATE = 39,
  PONKO_HUMAN_BONE_RIGHT_THUMB_DISTAL = 40,
  PONKO_HUMAN_BONE_RIGHT_INDEX_PROXIMAL = 41,
  PONKO_HUMAN_BONE_RIGHT_INDEX_INTERMEDIATE = 42,
  PONKO_HUMAN_BONE_RIGHT_INDEX_DISTAL = 43,
  PONKO_HUMAN_BONE_RIGHT_MIDDLE_PROXIMAL = 44,
  PONKO_HUMAN_BONE_RIGHT_MIDDLE_INTERMEDIATE = 45,
  PONKO_HUMAN_BONE_RIGHT_MIDDLE_DISTAL = 46,
  PONKO_HUMAN_BONE_RIGHT_RING_PROXIMAL = 47,
  PONKO_HUMAN_BONE_RIGHT_RING_INTERMEDIATE = 48,
  PONKO_HUMAN_BONE_RIGHT_RING_DISTAL = 49,
  PONKO_HUMAN_BONE_RIGHT_LITTLE_PROXIMAL = 50,
  PONKO_HUMAN_BONE_RIGHT_LITTLE_INTERMEDIATE = 51,
  PONKO_HUMAN_BONE_RIGHT_LITTLE_DISTAL = 52,
} PonkoHumanBone;

typedef enum PonkoReconcileOutcome {
  PONKO_RECONCILE_IGNORED = 0,
  PONKO_RECONCILE_CONFIRMED = 1,
  PONKO_RECONCILE_CORRECTED = 2,
} PonkoReconcileOutcome;

typedef enum PonkoInputPush {
  PONKO_INPUT_ADDED = 0,
  PONKO_INPUT_DUPLICATE = 1,
  PONKO_INPUT_LATE = 2,
} PonkoInputPush;

enum PonkoDebugView
#ifdef __cplusplus
  : uint32_t
#endif // __cplusplus
 {
  PONKO_DEBUG_VIEW_COLLIDERS = 0,
  PONKO_DEBUG_VIEW_NAVMESH = 1,
  PONKO_DEBUG_VIEW_BOUNDS = 2,
  PONKO_DEBUG_VIEW_LIGHTS = 3,
  PONKO_DEBUG_VIEW_HIERARCHY = 4,
};
#ifndef __cplusplus
typedef uint32_t PonkoDebugView;
#endif // __cplusplus

typedef enum PonkoWindowModeKind {
  PONKO_WINDOW_MODE_WINDOWED = 0,
  PONKO_WINDOW_MODE_BORDERLESS = 1,
  PONKO_WINDOW_MODE_EXCLUSIVE = 2,
} PonkoWindowModeKind;

typedef enum PonkoCursorGrab {
  PONKO_CURSOR_GRAB_NONE = 0,
  PONKO_CURSOR_GRAB_CONFINED = 1,
  PONKO_CURSOR_GRAB_LOCKED = 2,
} PonkoCursorGrab;

typedef enum PonkoWindowEventKind {
  PONKO_WINDOW_EVENT_FOCUSED = 0,
  PONKO_WINDOW_EVENT_MINIMIZED = 1,
  PONKO_WINDOW_EVENT_CLOSE_REQUESTED = 2,
  PONKO_WINDOW_EVENT_FILE_DROPPED = 3,
  PONKO_WINDOW_EVENT_SCALE_FACTOR_CHANGED = 4,
} PonkoWindowEventKind;

typedef enum PonkoCursorIcon {
  PONKO_CURSOR_DEFAULT = 0,
  PONKO_CURSOR_CONTEXT_MENU = 1,
  PONKO_CURSOR_HELP = 2,
  PONKO_CURSOR_POINTER = 3,
  PONKO_CURSOR_PROGRESS = 4,
  PONKO_CURSOR_WAIT = 5,
  PONKO_CURSOR_CELL = 6,
  PONKO_CURSOR_CROSSHAIR = 7,
  PONKO_CURSOR_TEXT = 8,
  PONKO_CURSOR_VERTICAL_TEXT = 9,
  PONKO_CURSOR_ALIAS = 10,
  PONKO_CURSOR_COPY = 11,
  PONKO_CURSOR_MOVE = 12,
  PONKO_CURSOR_NO_DROP = 13,
  PONKO_CURSOR_NOT_ALLOWED = 14,
  PONKO_CURSOR_GRAB = 15,
  PONKO_CURSOR_GRABBING = 16,
  PONKO_CURSOR_E_RESIZE = 17,
  PONKO_CURSOR_N_RESIZE = 18,
  PONKO_CURSOR_NE_RESIZE = 19,
  PONKO_CURSOR_NW_RESIZE = 20,
  PONKO_CURSOR_S_RESIZE = 21,
  PONKO_CURSOR_SE_RESIZE = 22,
  PONKO_CURSOR_SW_RESIZE = 23,
  PONKO_CURSOR_W_RESIZE = 24,
  PONKO_CURSOR_EW_RESIZE = 25,
  PONKO_CURSOR_NS_RESIZE = 26,
  PONKO_CURSOR_NESW_RESIZE = 27,
  PONKO_CURSOR_NWSE_RESIZE = 28,
  PONKO_CURSOR_COL_RESIZE = 29,
  PONKO_CURSOR_ROW_RESIZE = 30,
  PONKO_CURSOR_ALL_SCROLL = 31,
  PONKO_CURSOR_ZOOM_IN = 32,
  PONKO_CURSOR_ZOOM_OUT = 33,
  PONKO_CURSOR_DND_ASK = 34,
  PONKO_CURSOR_ALL_RESIZE = 35,
} PonkoCursorIcon;

enum PonkoTouchPhase
#ifdef __cplusplus
  : uint32_t
#endif // __cplusplus
 {
  PONKO_TOUCH_STARTED = 0,
  PONKO_TOUCH_MOVED = 1,
  PONKO_TOUCH_ENDED = 2,
  PONKO_TOUCH_CANCELLED = 3,
};
#ifndef __cplusplus
typedef uint32_t PonkoTouchPhase;
#endif // __cplusplus

enum PonkoGesture
#ifdef __cplusplus
  : uint32_t
#endif // __cplusplus
 {
  PONKO_GESTURE_TAP = 0,
  PONKO_GESTURE_DOUBLE_TAP = 1,
  PONKO_GESTURE_LONG_PRESS = 2,
  PONKO_GESTURE_DRAG = 3,
  PONKO_GESTURE_PINCH = 4,
  PONKO_GESTURE_ROTATE = 5,
  PONKO_GESTURE_SWIPE_LEFT = 6,
  PONKO_GESTURE_SWIPE_RIGHT = 7,
  PONKO_GESTURE_SWIPE_UP = 8,
  PONKO_GESTURE_SWIPE_DOWN = 9,
};
#ifndef __cplusplus
typedef uint32_t PonkoGesture;
#endif // __cplusplus

enum PonkoGestureAxis
#ifdef __cplusplus
  : uint32_t
#endif // __cplusplus
 {
  PONKO_GESTURE_AXIS_PINCH = 0,
  PONKO_GESTURE_AXIS_ROTATE = 1,
  PONKO_GESTURE_AXIS_DRAG_X = 2,
  PONKO_GESTURE_AXIS_DRAG_Y = 3,
};
#ifndef __cplusplus
typedef uint32_t PonkoGestureAxis;
#endif // __cplusplus

enum PonkoTouchShow
#ifdef __cplusplus
  : uint32_t
#endif // __cplusplus
 {
  PONKO_TOUCH_SHOW_AUTO = 0,
  PONKO_TOUCH_SHOW_ALWAYS = 1,
  PONKO_TOUCH_SHOW_NEVER = 2,
};
#ifndef __cplusplus
typedef uint32_t PonkoTouchShow;
#endif // __cplusplus

enum PonkoTouchControlKind
#ifdef __cplusplus
  : uint32_t
#endif // __cplusplus
 {
  PONKO_TOUCH_STICK = 0,
  PONKO_TOUCH_RIGHT_STICK = 1,
  PONKO_TOUCH_BUTTON = 2,
  PONKO_TOUCH_CORNER_BUTTON = 3,
};
#ifndef __cplusplus
typedef uint32_t PonkoTouchControlKind;
#endif // __cplusplus

typedef enum PonkoWaterQuality {
  PONKO_WATER_QUALITY_LOW,
  PONKO_WATER_QUALITY_MEDIUM,
  PONKO_WATER_QUALITY_HIGH,
} PonkoWaterQuality;

typedef enum PonkoWaterReflection {
  PONKO_WATER_REFLECTION_SKY,
  PONKO_WATER_REFLECTION_SCREEN_SPACE,
  PONKO_WATER_REFLECTION_PLANAR,
} PonkoWaterReflection;

typedef enum PonkoUiEaseName {
  PONKO_UI_EASE_LINEAR,
  PONKO_UI_EASE_EASE,
  PONKO_UI_EASE_IN,
  PONKO_UI_EASE_OUT,
  PONKO_UI_EASE_IN_OUT,
  PONKO_UI_EASE_BACK_OUT,
} PonkoUiEaseName;

typedef enum PonkoUiProperty {
  PONKO_UI_PROPERTY_OPACITY,
  PONKO_UI_PROPERTY_SCALE,
  PONKO_UI_PROPERTY_OFFSET,
  PONKO_UI_PROPERTY_BACKGROUND,
  PONKO_UI_PROPERTY_TEXT_COLOR,
  PONKO_UI_PROPERTY_WIDTH,
  PONKO_UI_PROPERTY_HEIGHT,
} PonkoUiProperty;

typedef enum PonkoUiRepeat {
  PONKO_UI_REPEAT_ONCE,
  PONKO_UI_REPEAT_TIMES,
  PONKO_UI_REPEAT_LOOP,
  PONKO_UI_REPEAT_PING_PONG,
} PonkoUiRepeat;

typedef enum PonkoHookKind {
  PONKO_HOOK_ADD = 0,
  PONKO_HOOK_REPLACE = 1,
  PONKO_HOOK_REMOVE = 2,
} PonkoHookKind;

typedef enum PonkoCloudQuality {
  PONKO_CLOUDS_FLAT = 0,
  PONKO_CLOUDS_LOW = 1,
  PONKO_CLOUDS_MEDIUM = 2,
  PONKO_CLOUDS_HIGH = 3,
} PonkoCloudQuality;

typedef enum PonkoJointChannel {
  PONKO_JOINT_TRANSLATION = 0,
  PONKO_JOINT_ROTATION = 1,
  PONKO_JOINT_SCALE = 2,
} PonkoJointChannel;

typedef enum PonkoEventTarget {
  PONKO_EVENT_TO_SERVER = 0,
  PONKO_EVENT_TO_CLIENT = 1,
  PONKO_EVENT_TO_CLIENTS = 2,
} PonkoEventTarget;

typedef enum PonkoDebugDraw {
  PONKO_DEBUG_DRAW_DEFAULT,
  PONKO_DEBUG_DRAW_ON,
  PONKO_DEBUG_DRAW_OFF,
} PonkoDebugDraw;

typedef struct PonkoAccess PonkoAccess;

typedef struct PonkoActionMap PonkoActionMap;

typedef struct PonkoAnimationGraph PonkoAnimationGraph;

typedef struct PonkoAnimationGraphBuilder PonkoAnimationGraphBuilder;

typedef struct PonkoAssetGroup PonkoAssetGroup;

typedef struct PonkoBakedLighting PonkoBakedLighting;

typedef struct PonkoBehaviorTree PonkoBehaviorTree;

typedef struct PonkoBrain PonkoBrain;

typedef struct PonkoCellStreamer PonkoCellStreamer;

typedef struct PonkoClip PonkoClip;

typedef struct PonkoCollisionMeshData PonkoCollisionMeshData;

typedef struct PonkoContactPair3D PonkoContactPair3D;

typedef struct PonkoCrowdAnimation PonkoCrowdAnimation;

typedef struct PonkoCtx PonkoCtx;

typedef struct PonkoFarTerrain PonkoFarTerrain;

typedef struct PonkoFilter PonkoFilter;

typedef struct PonkoFlowField PonkoFlowField;

typedef struct PonkoGame PonkoGame;

typedef struct PonkoGlyphAtlas PonkoGlyphAtlas;

typedef struct PonkoGrid PonkoGrid;

typedef struct PonkoHeightfield PonkoHeightfield;

typedef struct PonkoHookWorld PonkoHookWorld;

typedef struct PonkoHumanoidMap PonkoHumanoidMap;

typedef struct PonkoInputQueue PonkoInputQueue;

typedef struct PonkoInputScenario PonkoInputScenario;

typedef struct PonkoInterest PonkoInterest;

typedef struct PonkoLatestWins PonkoLatestWins;

typedef struct PonkoLightBakeScene PonkoLightBakeScene;

typedef struct PonkoLobby PonkoLobby;

typedef struct PonkoMemorySource PonkoMemorySource;

typedef struct PonkoMeshData PonkoMeshData;

typedef struct PonkoMotion PonkoMotion;

typedef struct PonkoNavGrid PonkoNavGrid;

typedef struct PonkoNavMesh PonkoNavMesh;

typedef struct PonkoNavSources PonkoNavSources;

typedef struct PonkoNetMessages PonkoNetMessages;

typedef struct PonkoPrediction PonkoPrediction;

typedef struct PonkoProfileScope PonkoProfileScope;

typedef struct PonkoRetargeter PonkoRetargeter;

typedef struct PonkoSnapshotStream PonkoSnapshotStream;

typedef struct PonkoSteamLobby PonkoSteamLobby;

typedef struct PonkoTerrainTileData PonkoTerrainTileData;

typedef struct PonkoUiAccess PonkoUiAccess;

typedef struct PonkoVideo PonkoVideo;

typedef struct PonkoWorld PonkoWorld;

typedef struct PonkoValue {
  uint32_t component;
  const void *data;
  size_t size;
} PonkoValue;

typedef struct PonkoEntity {
  uint32_t index;
  uint32_t generation;
} PonkoEntity;

typedef struct PonkoChunk {
  size_t count;
  size_t column_count;
  void *const *columns;
  const struct PonkoEntity *entities;
  size_t first;
} PonkoChunk;

typedef void (*PonkoQueryFn)(const struct PonkoChunk*, void*);

typedef void (*PonkoSystemFn)(struct PonkoCtx *ctx, void *user);

typedef struct PonkoActionBinding {
  uint32_t kind;
  uint32_t pad;
  uint32_t first;
  uint32_t second;
  uint32_t third;
  uint32_t fourth;
  float deadzone;
} PonkoActionBinding;

typedef struct PonkoVec2 {
  float x;
  float y;
} PonkoVec2;

typedef struct PonkoVec3 {
  float x;
  float y;
  float z;
} PonkoVec3;

typedef struct PonkoQuat {
  float x;
  float y;
  float z;
  float w;
} PonkoQuat;

typedef struct PonkoAssetGroupProgress {
  uint32_t state;
  size_t loaded;
  size_t total;
  uint64_t bytes_read;
  uint64_t bytes_total;
  bool bytes_known;
  uint64_t bytes_loaded;
  float fraction;
  float upload_ms;
  uint64_t bytes_uploaded;
} PonkoAssetGroupProgress;

typedef uint32_t (*PonkoAssetDecodeFn)(void *user, const uint8_t *data, size_t len);

typedef struct PonkoAssetGroupStats {
  float frame_budget_ms;
  uint64_t steps;
  float step_ms;
  float max_step_ms;
  uint64_t over_budget;
  uint64_t frame_bytes;
  float upload_ms;
  float max_upload_ms;
  double total_upload_ms;
  uint64_t total_upload_bytes;
} PonkoAssetGroupStats;

typedef void (*PonkoAssetGroupBuildFn)(struct PonkoCtx *ctx,
                                       struct PonkoAssetGroup *group,
                                       void *user);

typedef uint32_t (*PonkoAssetReadFn)(void *user, const char *name, const uint8_t **data, size_t *len);

typedef bool (*PonkoAssetContainsFn)(void *user, const char *name);

typedef void (*PonkoUserFreeFn)(void *user);

typedef struct PonkoAudioEffects {
  float low_pass_hz;
  float reverb;
} PonkoAudioEffects;

typedef struct PonkoSoundParams {
  float volume;
  float speed;
  bool looping;
} PonkoSoundParams;

typedef struct PonkoAudioBus {
  size_t index;
} PonkoAudioBus;

typedef struct PonkoAudioSettings3D {
  struct PonkoSoundParams sound;
  struct PonkoAudioBus bus;
  struct PonkoAudioEffects effects;
  float min_distance;
  float max_distance;
  enum PonkoAudioRolloff rolloff;
  float doppler;
} PonkoAudioSettings3D;

typedef struct PonkoSound {
  uint32_t id;
  float seconds;
} PonkoSound;

typedef struct PonkoPlaying {
  uint64_t id;
} PonkoPlaying;

typedef struct PonkoAudioListener3D {
  struct PonkoVec3 position;
  struct PonkoQuat rotation;
} PonkoAudioListener3D;

typedef struct PonkoBusEffect {
  enum PonkoBusEffectKind kind;
  enum PonkoFilterMode mode;
  float cutoff_hz;
  float resonance;
  float feedback;
  float damping;
  float stereo_width;
  float threshold_db;
  float ratio;
  float attack;
  float release;
  float makeup_db;
  float delay;
  float feedback_db;
  float mix;
} PonkoBusEffect;

typedef struct PonkoDucking {
  struct PonkoAudioBus key;
  float gain;
  float threshold;
  float attack;
  float release;
} PonkoDucking;

typedef struct PonkoEffectOverride {
  size_t index;
  struct PonkoBusEffect effect;
} PonkoEffectOverride;

typedef struct PonkoBusSnapshot {
  struct PonkoAudioBus bus;
  bool has_volume;
  float volume;
  const struct PonkoEffectOverride *effects;
  size_t effect_count;
} PonkoBusSnapshot;

typedef struct PonkoQueryOptions {
  uint32_t writes;
  uint32_t changed;
  uint32_t added;
  bool any;
  bool parallel;
  uint64_t since;
} PonkoQueryOptions;

typedef struct PonkoContactModifier3D {
  bool has_one_way;
  struct PonkoVec3 one_way;
  float one_way_angle;
  struct PonkoVec3 surface_velocity;
  bool hook;
} PonkoContactModifier3D;

typedef void (*PonkoContactHook3D)(struct PonkoContactPair3D *pair, void *user);

typedef struct PonkoMotorLevels {
  float strong;
  float weak;
  float left_trigger;
  float right_trigger;
} PonkoMotorLevels;

typedef struct PonkoGamepadInfo {
  uint32_t family;
  uint16_t vendor_id;
  uint16_t product_id;
  bool has_ids;
  bool rumble;
  bool trigger_rumble;
  bool motion;
} PonkoGamepadInfo;

typedef struct PonkoGlyph {
  uint32_t kind;
  uint32_t family;
  uint32_t code;
} PonkoGlyph;

typedef struct PonkoRect {
  struct PonkoVec2 min;
  struct PonkoVec2 max;
} PonkoRect;

typedef struct PonkoTexture {
  uint32_t id;
  uint32_t width;
  uint32_t height;
} PonkoTexture;

typedef struct PonkoColor {
  float r;
  float g;
  float b;
  float a;
} PonkoColor;

typedef struct PonkoDrawParams {
  struct PonkoVec2 scale;
  float rotation;
  struct PonkoColor color;
  float z;
  struct PonkoVec2 pivot;
  bool screen;
  bool has_region;
  struct PonkoRect region;
} PonkoDrawParams;

typedef struct PonkoCamera {
  struct PonkoVec2 position;
  float zoom;
} PonkoCamera;

typedef struct PonkoGridHit {
  struct PonkoEntity entity;
  struct PonkoVec2 position;
} PonkoGridHit;

typedef struct PonkoGridView {
  const void *entries;
  size_t entry_size;
  const uint64_t *cells;
  const uint32_t *starts;
  size_t len;
  size_t bucket_count;
  float inv_cell;
  uint64_t bucket_mask;
  uint32_t x_bits;
  uint32_t y_bits;
} PonkoGridView;

typedef struct PonkoQueryFilter2D {
  bool exclude;
  uint64_t exclude_entity;
  bool sensors;
} PonkoQueryFilter2D;

typedef struct PonkoRayHit {
  struct PonkoEntity entity;
  struct PonkoVec2 point;
  struct PonkoVec2 normal;
  float distance;
} PonkoRayHit;

typedef struct PonkoTransform3D {
  struct PonkoQuat rotation;
  struct PonkoVec3 position;
  struct PonkoVec3 scale;
  float reserved[2];
} PonkoTransform3D;

typedef struct PonkoTransform2D {
  struct PonkoVec2 position;
  float rotation;
  struct PonkoVec2 scale;
} PonkoTransform2D;

typedef struct PonkoFlowView {
  const uint8_t *cells;
  const struct PonkoVec2 *directions;
  uint32_t width;
  uint32_t height;
  size_t stride;
  struct PonkoVec2 origin;
  float cell_size;
} PonkoFlowView;

typedef void (*PonkoPluginBuildFn)(struct PonkoGame *game, void *user);

typedef struct PonkoGridItem {
  struct PonkoEntity entity;
  struct PonkoVec2 position;
  uint32_t data;
} PonkoGridItem;

typedef struct PonkoLiveCounts {
  size_t textures;
  size_t render_targets;
  size_t meshes;
  size_t materials;
  size_t shaders;
  size_t computes;
  size_t effects;
  size_t gpu_buffers;
  size_t instance_batches;
  size_t texture_arrays;
  size_t sounds;
  size_t fonts;
} PonkoLiveCounts;

typedef struct PonkoAssetCount {
  size_t live;
  uint64_t bytes;
} PonkoAssetCount;

typedef struct PonkoAssetRecord {
  uint32_t id;
  uint32_t owner;
  uint64_t bytes;
  uint64_t made_frame;
  uint64_t age_frames;
} PonkoAssetRecord;

typedef struct PonkoMesh {
  uint32_t id;
} PonkoMesh;

typedef struct PonkoMeshCost {
  struct PonkoMesh mesh;
  size_t models;
  size_t triangles;
  size_t shadow_models;
  size_t shadow_triangles;
  size_t name_offset;
  size_t name_len;
} PonkoMeshCost;

typedef struct PonkoStreamedAsset {
  uint32_t kind;
  uint32_t state;
  bool has_handle;
  uint32_t id;
  uint32_t width;
  uint32_t height;
  size_t name_offset;
  size_t name_len;
} PonkoStreamedAsset;

typedef struct PonkoLevelOptions {
  const char *const *prefab_classes;
  const char *const *prefab_paths;
  size_t prefab_count;
  const char *const *solid;
  size_t solid_count;
  const char *level;
  bool no_colliders;
  float z;
} PonkoLevelOptions;

typedef struct PonkoLdtkOptions {
  const struct PonkoLevelOptions *level;
  bool at_world_position;
  bool without_saved_objects;
  const char *const *classes;
  const char *const *components;
  size_t mapping_count;
} PonkoLdtkOptions;

typedef struct PonkoLevelObject {
  uint64_t id;
  struct PonkoVec2 position;
  struct PonkoVec2 size;
  float rotation;
  uint32_t shape;
  bool has_tile;
  uint32_t tileset;
  uint32_t tile;
  uint32_t flip;
  bool visible;
} PonkoLevelObject;

typedef struct PonkoPointLight2D {
  struct PonkoColor color;
  float intensity;
  float radius;
  float falloff;
  float height;
  bool shadows;
} PonkoPointLight2D;

typedef struct PonkoSpotLight2D {
  struct PonkoColor color;
  float intensity;
  float radius;
  float falloff;
  float height;
  float angle;
  float softness;
  bool shadows;
} PonkoSpotLight2D;

typedef struct PonkoDirectionalLight2D {
  struct PonkoColor color;
  float intensity;
  struct PonkoVec2 direction;
  float elevation;
  bool shadows;
} PonkoDirectionalLight2D;

typedef struct PonkoFont {
  uint32_t id;
} PonkoFont;

typedef struct PonkoLobbyEvent {
  enum PonkoLobbyEventKind kind;
  uint32_t member;
  enum PonkoLane lane;
  const uint8_t *bytes;
  size_t len;
  const char *text;
  enum PonkoLobbyError error;
  enum PonkoLobbyLeaveReason reason;
} PonkoLobbyEvent;

typedef struct PonkoLobbyState {
  bool connected;
  uint32_t me;
  bool in_room;
  bool is_host;
  bool queued;
  uint32_t host;
  uint16_t size;
  uint32_t members;
  char code[8];
  uint32_t max_relay_bytes;
} PonkoLobbyState;

typedef struct PonkoTextParams {
  struct PonkoFont font;
  float size;
  struct PonkoColor color;
  float z;
} PonkoTextParams;

typedef struct PonkoUiStyle {
  struct PonkoColor panel;
  struct PonkoColor button;
  struct PonkoColor button_hovered;
  struct PonkoColor button_pressed;
  struct PonkoColor accent;
  struct PonkoColor text;
  float text_size;
  float padding;
} PonkoUiStyle;

typedef struct PonkoTurns {
  uint32_t every;
  uint32_t now;
  float dt;
} PonkoTurns;

typedef struct PonkoMessageEvent {
  enum PonkoMessageEventKind kind;
  uint64_t peer;
  enum PonkoLane lane;
  const uint8_t *bytes;
  size_t len;
  const char *reason;
} PonkoMessageEvent;

typedef struct PonkoMessageStats {
  uint64_t packets_sent;
  uint64_t bytes_sent;
  uint64_t packets_received;
  uint64_t bytes_received;
} PonkoMessageStats;

typedef struct PonkoNavBakeSettings {
  float agent_radius;
  float agent_height;
  float max_climb;
  float max_slope;
  float cell_size;
  float cell_height;
  uint32_t tile_size;
  bool carving;
} PonkoNavBakeSettings;

typedef struct PonkoParticleKey {
  float t;
  float value;
} PonkoParticleKey;

typedef struct PonkoParticleColorKey {
  float t;
  struct PonkoColor color;
} PonkoParticleColorKey;

typedef struct PonkoParticleSettings3D {
  float rate;
  uint32_t burst;
  float duration;
  bool looping;
  float lifetime_min;
  float lifetime_max;
  float speed_min;
  float speed_max;
  uint32_t shape;
  struct PonkoVec3 shape_size;
  struct PonkoVec3 gravity;
  float drag;
  float size_min;
  float size_max;
  float rotation_min;
  float rotation_max;
  const struct PonkoParticleKey *size_keys;
  size_t size_key_count;
  const struct PonkoParticleColorKey *color_keys;
  size_t color_key_count;
  bool has_texture;
  struct PonkoTexture texture;
  uint32_t sheet_columns;
  uint32_t sheet_rows;
  uint32_t blend;
  uint32_t max_particles;
  uint32_t space;
  uint64_t seed;
} PonkoParticleSettings3D;

typedef struct PonkoSpriteMaterial {
  uint32_t id;
} PonkoSpriteMaterial;

typedef struct PonkoParticleSettings2D {
  float rate;
  uint32_t burst;
  float duration;
  bool looping;
  float lifetime_min;
  float lifetime_max;
  float speed_min;
  float speed_max;
  uint32_t shape;
  struct PonkoVec2 shape_size;
  struct PonkoVec2 gravity;
  float drag;
  float size_min;
  float size_max;
  float rotation_min;
  float rotation_max;
  const struct PonkoParticleKey *size_keys;
  size_t size_key_count;
  const struct PonkoParticleColorKey *color_keys;
  size_t color_key_count;
  bool has_texture;
  struct PonkoTexture texture;
  uint32_t sheet_columns;
  uint32_t sheet_rows;
  uint32_t blend;
  struct PonkoSpriteMaterial material;
  float z;
  uint32_t max_particles;
  uint32_t space;
  uint64_t seed;
} PonkoParticleSettings2D;

typedef struct PonkoFrameStats {
  uint64_t frames;
  float fps;
  float avg_ms;
  float p50_ms;
  float p95_ms;
  float p99_ms;
  float max_ms;
  float systems_ms;
  float render_cpu_ms;
  float display_wait_ms;
  float gpu_ms;
} PonkoFrameStats;

typedef struct PonkoGpuUploads {
  uint64_t bytes;
  uint64_t instances;
  uint64_t sprites;
  uint64_t static_batches;
  uint64_t uniforms;
  uint64_t lights;
  uint64_t materials;
  uint64_t skins;
  uint64_t meshes;
  uint64_t textures;
  uint64_t other;
  uint64_t buffer_bytes_created;
  uint32_t buffers_created;
  uint32_t reallocations;
} PonkoGpuUploads;

typedef struct PonkoRigidBody {
  uint32_t kind;
  float gravity_scale;
  float linear_damping;
  float angular_damping;
  bool lock_rotation;
  bool ccd;
} PonkoRigidBody;

typedef struct PonkoCollider {
  uint32_t shape;
  float radius;
  float height;
  struct PonkoVec2 size;
  struct PonkoVec2 offset;
  float density;
  float friction;
  float restitution;
  bool sensor;
} PonkoCollider;

typedef struct PonkoCollision {
  struct PonkoEntity a;
  struct PonkoEntity b;
  bool sensor;
} PonkoCollision;

typedef struct PonkoJointMotion3D {
  enum PonkoJointMotionKind kind;
  float min;
  float max;
} PonkoJointMotion3D;

typedef struct PonkoJointDrive3D {
  bool enabled;
  float target_position;
  float target_velocity;
  float stiffness;
  float damping;
  float max_force;
} PonkoJointDrive3D;

typedef struct PonkoJoint3D {
  uint64_t body_a;
  uint64_t body_b;
  struct PonkoTransform3D frame_a;
  struct PonkoTransform3D frame_b;
  struct PonkoJointMotion3D axes[6];
  struct PonkoJointDrive3D drives[6];
  bool contacts_enabled;
  float swing_cone;
} PonkoJoint3D;

typedef struct PonkoCollisionGroups {
  uint32_t memberships;
  uint32_t filter;
} PonkoCollisionGroups;

typedef struct PonkoQueryFilter3D {
  bool exclude;
  uint64_t exclude_entity;
  struct PonkoCollisionGroups groups;
  bool sensors;
} PonkoQueryFilter3D;

typedef struct PonkoRigidBody3D {
  uint32_t kind;
  float gravity_scale;
  float linear_damping;
  float angular_damping;
  bool lock_rotation;
  bool ccd;
  float mass;
  bool has_center_of_mass;
  struct PonkoVec3 center_of_mass;
  int8_t dominance;
} PonkoRigidBody3D;

typedef struct PonkoCollisionMesh {
  uint64_t id;
} PonkoCollisionMesh;

typedef struct PonkoCollider3D {
  uint32_t shape;
  float radius;
  float height;
  struct PonkoVec3 size;
  struct PonkoCollisionMesh mesh;
  struct PonkoVec3 offset;
  float density;
  float friction;
  float restitution;
  bool sensor;
  struct PonkoCollisionGroups groups;
} PonkoCollider3D;

typedef struct PonkoCharacter3D {
  float height;
  float radius;
  float max_slope;
  float step_height;
  float snap_distance;
  float push_mass;
  float gravity_scale;
  struct PonkoCollisionGroups groups;
  struct PonkoVec3 velocity;
  bool grounded;
} PonkoCharacter3D;

typedef struct PonkoHit3D {
  struct PonkoEntity entity;
  struct PonkoVec3 point;
  struct PonkoVec3 normal;
  float distance;
} PonkoHit3D;

typedef struct PonkoTerrainCollider {
  struct PonkoCollisionMesh mesh;
  float friction;
  float restitution;
  struct PonkoCollisionGroups groups;
} PonkoTerrainCollider;

typedef struct PonkoColliderFocus {
  float radius;
  float keep_radius;
} PonkoColliderFocus;

typedef struct PonkoPointHit2D {
  struct PonkoEntity entity;
  struct PonkoVec2 point;
  float distance;
  bool inside;
} PonkoPointHit2D;

typedef struct PonkoPenetration2D {
  struct PonkoEntity entity;
  struct PonkoVec2 point;
  struct PonkoVec2 normal;
  float depth;
} PonkoPenetration2D;

typedef struct PonkoPointHit3D {
  struct PonkoEntity entity;
  struct PonkoVec3 point;
  float distance;
  bool inside;
} PonkoPointHit3D;

typedef struct PonkoPenetration3D {
  struct PonkoEntity entity;
  struct PonkoVec3 point;
  struct PonkoVec3 normal;
  float depth;
} PonkoPenetration3D;

typedef uint64_t (*PonkoUiBuildFn)(struct PonkoCtx *ctx, void *user);

typedef bool (*PonkoConditionFn)(struct PonkoCtx *ctx, void *user);

typedef struct PonkoQueryCachePolicy {
  size_t min_archetypes;
  size_t max_signatures;
  size_t max_index_bytes;
  uint32_t promote_after;
  uint8_t max_match_percent;
  uint8_t reserved[19];
} PonkoQueryCachePolicy;

typedef struct PonkoQueryCacheStats {
  size_t signatures;
  size_t indexed;
  size_t indices;
  size_t bytes;
  uint64_t hits;
  uint64_t evictions;
} PonkoQueryCacheStats;

typedef struct PonkoRagdollBone {
  const char *joint;
  float radius;
  float swing;
  float twist;
  float density;
} PonkoRagdollBone;

typedef struct PonkoField {
  const char *name;
  uint32_t kind;
  size_t offset;
} PonkoField;

typedef struct PonkoUpdateStamp {
  uint16_t epoch;
  uint16_t seq;
} PonkoUpdateStamp;

typedef struct PonkoAuthority {
  uint64_t owner;
  uint16_t epoch;
} PonkoAuthority;

typedef struct PonkoSnapshotConfig {
  double delay;
  double max_extrapolation;
  uint32_t capacity;
} PonkoSnapshotConfig;

typedef struct PonkoPose {
  struct PonkoVec3 position;
  struct PonkoQuat rotation;
  struct PonkoVec3 velocity;
  bool has_velocity;
} PonkoPose;

typedef struct PonkoInterestTier {
  float radius;
  uint32_t every;
} PonkoInterestTier;

typedef struct PonkoInterestConfig {
  struct PonkoInterestTier tiers[8];
  uint32_t tier_count;
  float hysteresis;
  bool stagger;
} PonkoInterestConfig;

typedef struct PonkoInterestViewer {
  uint64_t peer;
  struct PonkoVec3 position;
} PonkoInterestViewer;

typedef struct PonkoInterestEntity {
  uint64_t id;
  struct PonkoVec3 position;
} PonkoInterestEntity;

typedef struct PonkoInterestEntry {
  uint64_t entity;
  float distance;
  uint32_t tier;
  bool send;
} PonkoInterestEntry;

typedef struct PonkoInterestChange {
  uint64_t peer;
  uint64_t entity;
  enum PonkoInterestChangeKind kind;
} PonkoInterestChange;

typedef struct PonkoReplicated {
  bool has_owner;
  uint64_t owner;
  float priority;
  bool global;
} PonkoReplicated;

typedef struct PonkoReplicatedField {
  const char *name;
  size_t offset;
  enum PonkoReplicatedType ty;
  enum PonkoFieldQuantize quantize;
  float min;
  float max;
  float step;
  uint8_t bits;
} PonkoReplicatedField;

typedef struct PonkoReplicatedComponent {
  const char *name;
  uint32_t component;
  const struct PonkoReplicatedField *fields;
  size_t field_count;
} PonkoReplicatedComponent;

typedef struct PonkoReplicationStatus {
  enum PonkoReplicationRole role;
  bool joined;
  bool has_me;
  uint64_t me;
  uint64_t server_tick;
  bool refused;
} PonkoReplicationStatus;

typedef struct PonkoReplica {
  uint32_t id;
  bool has_owner;
  uint64_t owner;
  uint32_t epoch;
  bool mine;
} PonkoReplica;

typedef struct PonkoReplicationPeerStats {
  uint64_t packets_sent;
  uint64_t bytes_sent;
  uint64_t packets_acked;
  uint64_t packets_lost;
  uint64_t records_sent;
  uint64_t creates_sent;
  uint64_t fulls_sent;
  uint64_t deltas_sent;
  uint64_t destroys_sent;
  uint64_t deferred;
} PonkoReplicationPeerStats;

typedef struct PonkoReplicationResendStats {
  uint64_t early_resends;
  uint64_t timeout_resends;
  uint64_t late_acks;
  uint32_t timeout_ticks;
  uint32_t backoff;
  uint32_t reorder_ticks;
} PonkoReplicationResendStats;

typedef struct PonkoReplicationClientStats {
  uint64_t packets_received;
  uint64_t bytes_received;
  uint64_t packets_stale;
  uint64_t packets_bad;
  uint64_t records_applied;
  uint64_t spawned;
  uint64_t despawned;
  uint64_t packets_sent;
  uint64_t bytes_sent;
} PonkoReplicationClientStats;

typedef struct PonkoResidencyStats {
  bool has_budget;
  uint64_t budget;
  size_t resident;
  uint64_t resident_bytes;
  uint64_t over_budget_bytes;
  size_t pinned;
  uint64_t pinned_bytes;
  uint64_t loads;
  uint64_t reloads;
  uint64_t evictions;
  uint64_t evicted_bytes;
} PonkoResidencyStats;

typedef struct PonkoUploadStats {
  bool has_budget;
  uint64_t budget;
  size_t pending;
  uint64_t frame_bytes;
  uint32_t frame_uploads;
  uint64_t total_bytes;
  uint64_t total_uploads;
  uint64_t oversized;
  uint64_t decode_waits;
  uint64_t held_by_budget;
  uint64_t failures;
  float step_ms;
  float max_step_ms;
} PonkoUploadStats;

typedef void (*PonkoResourceScopeFn)(struct PonkoWorld *world, void *value, void *user);

typedef uint32_t (*PonkoJsonConvertFn)(void *user, const char *json, const char **out_json);

typedef struct PonkoScenarioReport {
  uint32_t status;
  bool failed;
  uint64_t duration;
  uint64_t ticks_run;
  uint64_t start_tick;
  uint64_t steps;
  uint64_t inputs_applied;
  uint64_t checks_passed;
  uint64_t failure_step;
  uint64_t failure_tick;
  uint64_t failure_sim_tick;
  uint64_t released;
  uint64_t still_held;
} PonkoScenarioReport;

typedef struct PonkoShaderStatus {
  uint32_t kind;
  uint32_t outcome;
  bool ok;
  uint32_t reloads;
  uint64_t frame;
  uint32_t diagnostics;
  uint32_t line;
  uint32_t column;
  bool in_engine_code;
} PonkoShaderStatus;

typedef struct PonkoJoint {
  const char *name;
  int32_t parent;
  struct PonkoTransform3D rest;
} PonkoJoint;

typedef struct PonkoMaterial {
  uint32_t id;
} PonkoMaterial;

typedef struct PonkoSoundStarted {
  uint32_t struct_size;
  uint32_t version;
  struct PonkoPlaying playing;
  struct PonkoSound sound;
  struct PonkoAudioBus bus;
  float volume;
  float speed;
  bool looping;
} PonkoSoundStarted;

typedef struct PonkoSpriteShader {
  uint32_t id;
} PonkoSpriteShader;

typedef struct PonkoSpriteMaterialDesc {
  bool has_shader;
  struct PonkoSpriteShader shader;
  float params[16];
  bool has_textures[2];
  struct PonkoTexture textures[2];
} PonkoSpriteMaterialDesc;

typedef struct PonkoStencilFace {
  uint32_t compare;
  uint32_t fail;
  uint32_t depth_fail;
  uint32_t pass;
} PonkoStencilFace;

typedef struct PonkoStencil {
  struct PonkoStencilFace front;
  struct PonkoStencilFace back;
  int32_t order;
  uint8_t reference;
  uint8_t read_mask;
  uint8_t write_mask;
} PonkoStencil;

typedef struct PonkoShader {
  uint32_t id;
} PonkoShader;

typedef struct PonkoTextureArray {
  uint32_t id;
  uint32_t width;
  uint32_t height;
  uint32_t layers;
} PonkoTextureArray;

typedef struct PonkoMaterialDesc {
  struct PonkoColor color;
  bool has_texture;
  struct PonkoTexture texture;
  float roughness;
  float metallic;
  struct PonkoColor emissive;
  bool has_normal_map;
  struct PonkoTexture normal_map;
  float normal_strength;
  bool has_orm_map;
  struct PonkoTexture orm_map;
  bool has_emissive_map;
  struct PonkoTexture emissive_map;
  uint32_t blend;
  bool double_sided;
  bool has_shader;
  struct PonkoShader shader;
  float custom[16];
  bool has_custom_textures[2];
  struct PonkoTexture custom_textures[2];
  bool has_custom_arrays[3];
  struct PonkoTextureArray custom_arrays[3];
} PonkoMaterialDesc;

typedef struct PonkoCell {
  int32_t x;
  int32_t z;
} PonkoCell;

typedef struct PonkoTerrainTile {
  struct PonkoMesh lods[PONKO_TERRAIN_LODS];
  float lod_distance;
} PonkoTerrainTile;

typedef struct PonkoTrail3D {
  float lifetime;
  float width;
  const struct PonkoParticleKey *width_keys;
  size_t width_key_count;
  const struct PonkoParticleColorKey *color_keys;
  size_t color_key_count;
  bool has_texture;
  struct PonkoTexture texture;
  uint32_t blend;
  float min_distance;
  uint32_t max_points;
  bool emitting;
} PonkoTrail3D;

typedef struct PonkoLine3D {
  const struct PonkoVec3 *points;
  size_t point_count;
  float width;
  struct PonkoColor color;
  bool has_texture;
  struct PonkoTexture texture;
  uint32_t blend;
  float texture_length;
} PonkoLine3D;

typedef struct PonkoText3D {
  struct PonkoFont font;
  float size;
  struct PonkoColor color;
  struct PonkoColor outline;
  float outline_width;
  float softness;
  uint32_t align;
  bool billboard;
} PonkoText3D;

typedef struct PonkoModel {
  struct PonkoMesh mesh;
  struct PonkoColor color;
} PonkoModel;

typedef struct PonkoTerrainMaterialDesc {
  struct PonkoTextureArray layers;
  bool has_normals;
  struct PonkoTextureArray normals;
  float normal_strength;
  struct PonkoTextureArray weights;
  struct PonkoVec2 weights_corner;
  struct PonkoVec2 weights_size;
  float layer_size[PONKO_TERRAIN_LAYERS];
  float roughness;
} PonkoTerrainMaterialDesc;

typedef struct PonkoMeshView {
  const struct PonkoVec3 *positions;
  const struct PonkoVec3 *normals;
  const struct PonkoVec2 *uvs;
  size_t vertex_count;
  const uint32_t *indices;
  size_t index_count;
} PonkoMeshView;

typedef struct PonkoCompute {
  uint32_t id;
} PonkoCompute;

typedef struct PonkoGpuBuffer {
  uint32_t id;
} PonkoGpuBuffer;

typedef struct PonkoComputePass {
  bool has_buffer;
  struct PonkoGpuBuffer buffer;
  struct PonkoCompute compute;
  struct PonkoTexture target;
  bool has_material;
  struct PonkoMaterial material;
} PonkoComputePass;

typedef struct PonkoEffect {
  uint32_t id;
} PonkoEffect;

typedef struct PonkoScreenEffect {
  struct PonkoEffect effect;
  bool has_material;
  struct PonkoMaterial material;
} PonkoScreenEffect;

typedef struct PonkoCamera3D {
  struct PonkoVec3 position;
  struct PonkoQuat rotation;
  float fov_y;
  float near;
  float far;
} PonkoCamera3D;

typedef struct PonkoTargetCamera {
  struct PonkoCamera3D camera;
  struct PonkoTexture target;
  bool has_clip_plane;
  float clip_plane[4];
} PonkoTargetCamera;

typedef struct PonkoLighting {
  struct PonkoVec3 sun_direction;
  struct PonkoColor sun_color;
  float sun_intensity;
  struct PonkoColor sky_color;
  struct PonkoColor ground_color;
  float ambient_intensity;
  bool shadows;
  float shadow_distance;
  uint32_t shadow_cascades;
  uint32_t shadow_resolution;
  uint32_t light_shadow_views;
  uint32_t light_shadow_resolution;
  uint32_t light_shadow_atlas;
  uint32_t shadow_refresh;
  uint32_t shadow_filter;
  bool has_equator;
  struct PonkoColor equator_color;
  bool gi_enabled;
  struct PonkoVec3 gi_extent;
  float gi_spacing;
  uint32_t gi_probes_per_frame;
  float gi_intensity;
} PonkoLighting;

typedef struct PonkoSky {
  bool atmosphere;
  float haze;
  float sun_size;
  float stars;
  struct PonkoColor night_light;
  float fog_density;
  float fog_height;
  float fog_falloff;
  struct PonkoColor fog_color;
  float fog_start;
  float fog_end;
  float fog_sun_glow;
  float fog_sun_focus;
  float light_shafts;
} PonkoSky;

typedef struct PonkoPost {
  float render_scale;
  float sharpen;
  uint32_t anti_aliasing;
  bool ao_enabled;
  float ao_radius;
  float ao_intensity;
  uint32_t ao_slices;
  uint32_t ao_steps;
  bool ao_half_resolution;
  bool ao_temporal;
  float exposure;
  uint32_t tonemap;
  float bloom;
  float bloom_threshold;
  float contrast;
  float saturation;
  float temperature;
  float tint;
  float vignette;
  float chromatic_aberration;
  float lens_distortion;
  float lens_scale;
  bool has_lut;
  struct PonkoTexture lut;
  float taa_sharpness;
  float contact_shadows;
  float ssr;
  float ssr_max_distance;
} PonkoPost;

typedef struct PonkoGraphicsQuality {
  float render_scale;
  float sharpen;
  bool shadows;
  float shadow_distance;
  uint32_t shadow_cascades;
  uint32_t shadow_resolution;
  uint32_t light_shadow_views;
  uint32_t light_shadow_resolution;
  uint32_t light_shadow_atlas;
  bool ambient_occlusion;
  uint32_t ao_slices;
  uint32_t ao_steps;
  bool ao_half_resolution;
  bool ao_temporal;
  uint32_t anti_aliasing;
  uint32_t shadow_refresh;
  uint32_t shadow_filter;
  float ssr;
  bool gi;
} PonkoGraphicsQuality;

typedef struct PonkoReflectionProbe3D {
  struct PonkoVec3 size;
  float blend_distance;
  float intensity;
  uint32_t content;
  bool box_projection;
} PonkoReflectionProbe3D;

typedef struct PonkoDecal3D {
  struct PonkoVec3 size;
  uint32_t image;
  uint32_t normal_image;
  struct PonkoVec2 uv_min;
  struct PonkoVec2 uv_max;
  struct PonkoColor color;
  float opacity;
  float normal_strength;
  float roughness;
  float metallic;
  float angle_fade_start;
  float angle_fade_end;
  float depth_fade;
  float max_distance;
  int32_t order;
  uint32_t layers;
} PonkoDecal3D;

typedef struct PonkoLodRenderer {
  struct PonkoMesh mesh;
  struct PonkoTransform3D offset;
  struct PonkoColor color;
} PonkoLodRenderer;

typedef struct PonkoLodLevel {
  float screen_height;
  float fade_width;
  const struct PonkoLodRenderer *renderers;
  size_t renderer_count;
  bool custom_shadow;
  const struct PonkoLodRenderer *shadow_casters;
  size_t shadow_caster_count;
} PonkoLodLevel;

typedef struct PonkoLodPick {
  uint32_t kind;
  uint32_t level;
  int32_t to;
  float blend;
} PonkoLodPick;

typedef struct PonkoLodSettings {
  float bias;
  bool crossfade;
} PonkoLodSettings;

typedef struct PonkoInstanceBatch {
  uint32_t id;
} PonkoInstanceBatch;

typedef struct PonkoInstanceBatchOptions {
  float cell_size;
  float max_distance;
  bool cast_shadows;
  struct PonkoColor tint;
} PonkoInstanceBatchOptions;

typedef struct PonkoInstanceLevel {
  struct PonkoMesh mesh;
  float min_distance;
  float max_distance;
  bool camera;
  bool shadows;
} PonkoInstanceLevel;

typedef struct PonkoInstanceCell {
  uint32_t id;
} PonkoInstanceCell;

typedef struct PonkoCrowdInstance {
  float transform[16];
  struct PonkoColor color;
  uint16_t clip;
  uint16_t blend_clip;
  float time;
  float blend_time;
  float blend;
  float speed;
} PonkoCrowdInstance;

typedef struct PonkoCrowdClip {
  const struct PonkoClip *clip;
  bool looping;
} PonkoCrowdClip;

typedef struct PonkoCrowdLevel {
  struct PonkoMesh mesh;
  float min_distance;
  float max_distance;
  bool camera;
  bool shadows;
  uint8_t influences;
} PonkoCrowdLevel;

typedef struct PonkoCrowdOptions {
  bool cast_shadows;
} PonkoCrowdOptions;

typedef struct PonkoCrowd {
  uint32_t id;
} PonkoCrowd;

typedef struct PonkoSpriteSheet {
  struct PonkoTexture texture;
  struct PonkoVec2 frame_size;
  uint32_t columns;
  uint32_t len;
} PonkoSpriteSheet;

typedef struct PonkoTileMap {
  struct PonkoSpriteSheet sheet;
  uint32_t width;
  uint32_t height;
  struct PonkoVec2 tile_size;
  float z;
  struct PonkoColor color;
  uint16_t *tiles;
} PonkoTileMap;

typedef struct PonkoUiAccessChanges {
  uint32_t changed_count;
  uint32_t removed_count;
  bool any;
  bool screens_changed;
  bool focus_changed;
} PonkoUiAccessChanges;

typedef struct PonkoUiAccessNode {
  uint64_t id;
  uint64_t parent;
  uint64_t selection_anchor;
  uint64_t selection_caret;
  struct PonkoRect rect;
  enum PonkoUiRole role;
  enum PonkoUiAccessValueKind value_kind;
  float value;
  enum PonkoUiLive live;
  uint32_t depth;
  uint32_t child_count;
  bool has_selection;
  bool focusable;
  bool focused;
  bool disabled;
  bool inert;
  bool modal;
} PonkoUiAccessNode;

typedef struct PonkoUiAccessStateEx {
  uint32_t struct_size;
  uint32_t version;
  bool selected;
  enum PonkoUiExpanded expanded;
  uint32_t position;
  uint32_t set_size;
  double min;
  double max;
  double step;
} PonkoUiAccessStateEx;

typedef struct PonkoUiEase {
  float x1;
  float y1;
  float x2;
  float y2;
} PonkoUiEase;

typedef struct PonkoUiTransition {
  float seconds;
  struct PonkoUiEase ease;
} PonkoUiTransition;

typedef struct PonkoUiScreenTransition {
  struct PonkoUiTransition transition;
  float opacity;
  struct PonkoVec2 offset;
  float scale;
} PonkoUiScreenTransition;

typedef struct PonkoUiValue {
  float number;
  struct PonkoVec2 point;
  struct PonkoColor color;
} PonkoUiValue;

typedef struct PonkoUiTween {
  uint32_t property;
  float start;
  float seconds;
  struct PonkoUiEase ease;
  bool has_from;
  struct PonkoUiValue from;
  struct PonkoUiValue to;
} PonkoUiTween;

typedef struct PonkoUiDrawn {
  struct PonkoRect rect;
  float opacity;
  float scale;
  struct PonkoColor fill;
} PonkoUiDrawn;

typedef struct PonkoUiLength {
  uint32_t kind;
  float value;
} PonkoUiLength;

typedef struct PonkoUiSides {
  struct PonkoUiLength top;
  struct PonkoUiLength right;
  struct PonkoUiLength bottom;
  struct PonkoUiLength left;
} PonkoUiSides;

typedef struct PonkoUiGap {
  struct PonkoUiLength column;
  struct PonkoUiLength row;
} PonkoUiGap;

typedef struct PonkoUiTrack {
  uint32_t kind;
  float value;
} PonkoUiTrack;

typedef struct PonkoUiGridLine {
  uint32_t kind;
  int32_t value;
} PonkoUiGridLine;

typedef struct PonkoUiTemplateTrack {
  struct PonkoUiTrack track;
  uint32_t repetition;
  uint32_t count;
  const struct PonkoUiTrack *tracks;
  size_t track_count;
  const char *const *line_names;
  size_t line_name_count;
} PonkoUiTemplateTrack;

typedef struct PonkoUiLayout {
  uint32_t display;
  uint32_t position;
  uint32_t flex_direction;
  uint32_t flex_wrap;
  float flex_grow;
  float flex_shrink;
  struct PonkoUiLength flex_basis;
  uint32_t align_items;
  uint32_t align_self;
  uint32_t justify_items;
  uint32_t justify_self;
  uint32_t align_content;
  uint32_t justify_content;
  struct PonkoUiLength width;
  struct PonkoUiLength height;
  struct PonkoUiLength min_width;
  struct PonkoUiLength min_height;
  struct PonkoUiLength max_width;
  struct PonkoUiLength max_height;
  struct PonkoUiSides margin;
  struct PonkoUiSides padding;
  struct PonkoUiSides inset;
  struct PonkoUiGap gap;
  float aspect_ratio;
  const struct PonkoUiTrack *grid_columns;
  size_t grid_column_count;
  const struct PonkoUiTrack *grid_rows;
  size_t grid_row_count;
  const struct PonkoUiTrack *grid_auto_columns;
  size_t grid_auto_column_count;
  const struct PonkoUiTrack *grid_auto_rows;
  size_t grid_auto_row_count;
  struct PonkoUiGridLine grid_column_start;
  struct PonkoUiGridLine grid_column_end;
  struct PonkoUiGridLine grid_row_start;
  struct PonkoUiGridLine grid_row_end;
  const struct PonkoUiTemplateTrack *grid_template_columns;
  size_t grid_template_column_count;
  const struct PonkoUiTemplateTrack *grid_template_rows;
  size_t grid_template_row_count;
  const char *const *grid_template_areas;
  size_t grid_template_area_count;
  const char *const *grid_column_names;
  size_t grid_column_name_count;
  const char *const *grid_row_names;
  size_t grid_row_name_count;
  const char *grid_area;
} PonkoUiLayout;

typedef struct PonkoUiPaint {
  struct PonkoColor background;
  struct PonkoColor hover;
  struct PonkoColor pressed;
  struct PonkoColor disabled;
  struct PonkoColor border;
  float border_width;
  struct PonkoColor color;
  struct PonkoFont font;
  float font_size;
  bool wrap;
  uint32_t align;
  bool has_texture;
  struct PonkoTexture texture;
  float slice;
} PonkoUiPaint;

typedef struct PonkoUiInput {
  struct PonkoVec2 pointer;
  bool pressed;
  bool held;
  bool released;
  struct PonkoVec2 wheel;
  bool next;
  bool previous;
  struct PonkoVec2 direction;
  bool confirm;
  bool cancel;
  bool shift;
  bool select_all;
  bool backspace;
  bool delete_;
  bool home;
  bool end;
  const char *text;
} PonkoUiInput;

typedef struct PonkoUiEvent {
  enum PonkoUiEventKind kind;
  uint64_t id;
} PonkoUiEvent;

typedef struct PonkoUiTheme {
  struct PonkoColor panel;
  struct PonkoColor control;
  struct PonkoColor hover;
  struct PonkoColor pressed;
  struct PonkoColor disabled;
  struct PonkoColor accent;
  struct PonkoColor text;
  struct PonkoColor title;
  struct PonkoFont font;
  float text_size;
  float title_size;
  float border_width;
} PonkoUiTheme;

typedef struct PonkoUiEventEx {
  uint32_t struct_size;
  uint32_t version;
  enum PonkoUiEventKind kind;
  uint64_t id;
  uint64_t other;
  uint64_t index;
} PonkoUiEventEx;

typedef struct PonkoUiThemeEx {
  uint32_t struct_size;
  uint32_t version;
  struct PonkoUiTheme theme;
  float spacing;
  float radius;
  float tooltip_delay;
  struct PonkoUiTransition transition;
  float press_scale;
} PonkoUiThemeEx;

typedef struct PonkoUiInputEx {
  uint32_t struct_size;
  uint32_t version;
  struct PonkoUiInput input;
  float dt;
  bool secondary;
  bool menu;
  bool has_captured;
  struct PonkoActionBinding captured;
} PonkoUiInputEx;

typedef struct PonkoFrictionCurve {
  float extremum_slip;
  float extremum_value;
  float asymptote_slip;
  float asymptote_value;
  float stiffness;
} PonkoFrictionCurve;

typedef struct PonkoWheel3D {
  struct PonkoVec3 position;
  float radius;
  float mass;
  float suspension_distance;
  float spring;
  float damper;
  float target_position;
  float force_app_point_distance;
  float damping_rate;
  struct PonkoFrictionCurve forward_friction;
  struct PonkoFrictionCurve sideways_friction;
  float motor_torque;
  float brake_torque;
  float steer_angle;
  struct PonkoQuat local_rotation;
  bool grounded;
  struct PonkoVec3 contact_point;
  struct PonkoVec3 contact_normal;
  float compression;
  float suspension_force;
  float sprung_mass;
  float angular_velocity;
  float forward_slip;
  float sideways_slip;
  float spin;
  struct PonkoVec3 local_position;
  float reserved[3];
} PonkoWheel3D;

typedef struct PonkoVehicle3D {
  struct PonkoWheel3D wheels[8];
  uint32_t wheel_count;
  struct PonkoCollisionGroups groups;
  uint32_t reserved;
} PonkoVehicle3D;

typedef struct PonkoAudioKey {
  float distance;
  float gain;
} PonkoAudioKey;

typedef struct PonkoTargetEffect {
  struct PonkoEffect effect;
  struct PonkoTexture target;
  bool has_material;
  struct PonkoMaterial material;
} PonkoTargetEffect;

typedef struct PonkoImeEvent {
  uint32_t kind;
  const char *text;
  bool has_cursor;
  size_t cursor_start;
  size_t cursor_end;
} PonkoImeEvent;

typedef struct PonkoUiComposition {
  const char *preedit;
  bool has_cursor;
  size_t cursor_start;
  size_t cursor_end;
  bool composing;
} PonkoUiComposition;

typedef struct PonkoStringArg {
  const char *name;
  PonkoStringArgKind kind;
  const char *text;
  int64_t integer;
  double number;
} PonkoStringArg;

typedef struct PonkoDebugStyle {
  float seconds;
  float width;
  float size;
  bool on_top;
  bool screen;
} PonkoDebugStyle;

typedef struct PonkoDebugStyleEx {
  uint32_t struct_size;
  uint32_t version;
  float seconds;
  float width;
  float size;
  bool on_top;
  uint32_t space;
} PonkoDebugStyleEx;

typedef struct PonkoDebugStats {
  size_t lines_2d;
  size_t lines_3d;
  size_t labels;
} PonkoDebugStats;

typedef struct PonkoColliderEx {
  uint32_t struct_size;
  uint32_t version;
  struct PonkoCollider collider;
  struct PonkoCollisionGroups groups;
  uint32_t point_count;
  struct PonkoVec2 points[8];
} PonkoColliderEx;

typedef struct PonkoQueryFilterEx {
  uint32_t struct_size;
  uint32_t version;
  bool exclude;
  uint64_t exclude_entity;
  struct PonkoCollisionGroups groups;
  bool sensors;
} PonkoQueryFilterEx;

typedef struct PonkoMaterialDescEx {
  uint32_t struct_size;
  uint32_t version;
  struct PonkoMaterialDesc desc;
  bool has_stencil;
  struct PonkoStencil stencil;
} PonkoMaterialDescEx;

typedef struct PonkoNavBakeSettingsEx {
  uint32_t struct_size;
  uint32_t version;
  struct PonkoNavBakeSettings settings;
} PonkoNavBakeSettingsEx;

typedef struct PonkoLightingEx {
  uint32_t struct_size;
  uint32_t version;
  struct PonkoLighting lighting;
  float shadow_sun_size;
  float shadow_sun_step;
} PonkoLightingEx;

typedef struct PonkoPostEx {
  uint32_t struct_size;
  uint32_t version;
  struct PonkoPost post;
  struct PonkoColor split_toning_shadows;
  struct PonkoColor split_toning_highlights;
  float split_toning_balance;
  bool dof_enabled;
  float dof_focus_distance;
  float dof_focus_range;
  float dof_blur;
  uint32_t msaa;
} PonkoPostEx;

typedef struct PonkoPostVolume {
  struct PonkoVec3 min;
  struct PonkoVec3 max;
  float blend_distance;
  const struct PonkoPostEx *post;
} PonkoPostVolume;

typedef struct PonkoParticleSettings3DEx {
  uint32_t struct_size;
  uint32_t version;
  struct PonkoParticleSettings3D settings;
  float softness;
} PonkoParticleSettings3DEx;

typedef struct PonkoCamera3DEx {
  uint32_t struct_size;
  uint32_t version;
  struct PonkoCamera3D camera;
  bool occlusion_culling;
  bool particles;
  uint32_t debug_draw;
} PonkoCamera3DEx;

typedef struct PonkoTargetCameraEx {
  uint32_t struct_size;
  uint32_t version;
  struct PonkoTargetCamera target_camera;
  bool particles;
  uint32_t debug_draw;
} PonkoTargetCameraEx;

typedef bool (*PonkoPredictStepFn)(void *user,
                                   void *state,
                                   size_t state_size,
                                   const void *input,
                                   size_t input_size);

typedef struct PonkoReconciled {
  uint32_t outcome;
  uint32_t replayed;
} PonkoReconciled;

typedef struct PonkoPredictionStats {
  uint64_t predicted;
  uint64_t confirmed;
  uint64_t corrections;
  uint64_t replayed;
  uint64_t dropped;
} PonkoPredictionStats;

typedef struct PonkoSprite {
  struct PonkoTexture texture;
  struct PonkoColor color;
  float z;
  struct PonkoVec2 pivot;
  bool has_region;
  struct PonkoRect region;
} PonkoSprite;

typedef struct PonkoCameraShake {
  uint32_t struct_size;
  uint32_t version;
  float trauma;
  float decay;
  float exponent;
  float frequency;
  struct PonkoVec2 max_offset;
  struct PonkoVec3 max_angles;
  uint32_t seed;
  float time;
} PonkoCameraShake;

typedef struct PonkoFollowCamera2D {
  uint32_t struct_size;
  uint32_t version;
  struct PonkoVec2 dead_zone;
  struct PonkoVec2 look_ahead;
  float look_ahead_speed;
  struct PonkoVec2 offset;
  float smooth_time;
  bool has_bounds;
  struct PonkoRect bounds;
  struct PonkoVec2 center;
  struct PonkoVec2 velocity;
  struct PonkoVec2 focus;
  struct PonkoVec2 facing;
  struct PonkoVec2 goal;
  struct PonkoVec2 last_target;
  bool started;
} PonkoFollowCamera2D;

typedef struct PonkoPixelPerfect {
  uint32_t struct_size;
  uint32_t version;
  struct PonkoVec2 resolution;
  float pixels_per_unit;
  bool snap;
} PonkoPixelPerfect;

typedef struct PonkoOrbitCamera {
  uint32_t struct_size;
  uint32_t version;
  struct PonkoVec3 target;
  float yaw;
  float pitch;
  float distance;
  float min_pitch;
  float max_pitch;
  float min_distance;
  float max_distance;
  float sensitivity;
  float smoothing;
  struct PonkoVec3 current_target;
  struct PonkoVec3 current;
  struct PonkoVec3 last_target;
  struct PonkoVec3 last_goal;
  bool started;
} PonkoOrbitCamera;

typedef struct PonkoFirstPersonCamera {
  uint32_t struct_size;
  uint32_t version;
  float yaw;
  float pitch;
  float min_pitch;
  float max_pitch;
  float sensitivity;
  float smoothing;
  struct PonkoVec2 current;
  struct PonkoVec2 last_goal;
  bool started;
} PonkoFirstPersonCamera;

typedef struct PonkoThirdPersonCamera {
  uint32_t struct_size;
  uint32_t version;
  float yaw;
  float pitch;
  float distance;
  float height;
  float shoulder;
  float min_pitch;
  float max_pitch;
  float sensitivity;
  float follow_smoothing;
  float collision_radius;
  float recover_smoothing;
  struct PonkoVec3 current_pivot;
  float current_distance;
  struct PonkoVec3 last_pivot;
  bool started;
} PonkoThirdPersonCamera;

typedef struct PonkoViewport {
  float x;
  float y;
  float width;
  float height;
} PonkoViewport;

typedef struct PonkoView2D {
  uint32_t struct_size;
  uint32_t version;
  struct PonkoCamera camera;
  struct PonkoViewport rect;
  uint32_t layers;
  bool has_clear;
  struct PonkoColor clear;
} PonkoView2D;

typedef struct PonkoView3D {
  uint32_t struct_size;
  uint32_t version;
  struct PonkoCamera3D camera;
  struct PonkoViewport rect;
  uint32_t layers;
  bool has_clear;
  struct PonkoColor clear;
  bool has_post;
  struct PonkoPost post;
} PonkoView3D;

typedef struct PonkoTouch {
  uint64_t id;
  struct PonkoVec2 position;
  struct PonkoVec2 start;
  struct PonkoVec2 delta;
  bool claimed;
} PonkoTouch;

typedef struct PonkoGestureEvent {
  uint32_t gesture;
  struct PonkoVec2 position;
} PonkoGestureEvent;

typedef struct PonkoGestureSettings {
  float tap_distance;
  float tap_time;
  float double_tap_time;
  float double_tap_distance;
  float long_press_time;
  float swipe_distance;
  float swipe_time;
  float pinch_distance;
  float rotate_angle;
} PonkoGestureSettings;

typedef struct PonkoTouchControl {
  uint32_t kind;
  const char *action;
  const char *label;
} PonkoTouchControl;

typedef struct PonkoWave {
  struct PonkoVec2 direction;
  float wavelength;
  float amplitude;
  float steepness;
  float speed;
  float phase;
} PonkoWave;

typedef struct PonkoWaterDesc {
  uint32_t struct_size;
  uint32_t version;
  float level;
  struct PonkoVec2 center;
  struct PonkoVec2 size;
  bool follow_camera;
  struct PonkoWave waves[PONKO_WATER_MAX_WAVES];
  uint32_t wave_count;
  struct PonkoVec2 flow;
  const uint8_t *flow_map;
  uint32_t flow_map_width;
  uint32_t flow_map_height;
  float flow_map_max_speed;
  struct PonkoVec3 absorption;
  struct PonkoColor scatter;
  float foam;
  float shore_foam;
  float refraction;
  uint32_t reflection;
  uint32_t quality;
  bool ripples;
  float ripple_area;
  bool underwater;
} PonkoWaterDesc;

typedef struct PonkoWaterSample {
  float height;
  struct PonkoVec3 normal;
  struct PonkoVec3 velocity;
  float squeeze;
} PonkoWaterSample;

typedef struct PonkoHookEvent {
  uint64_t entity;
  uint32_t component;
  uint32_t kind;
  const void *value;
  const void *new_value;
  size_t size;
} PonkoHookEvent;

typedef void (*PonkoHookFn)(struct PonkoHookWorld*, const struct PonkoHookEvent*, void*);

typedef void (*PonkoFreeUserFn)(void*);

typedef struct PonkoJointTrack {
  uint32_t joint;
  uint32_t channel;
  uint32_t interpolation;
  const float *times;
  const float *values;
  size_t count;
} PonkoJointTrack;

typedef struct PonkoLiveCountsEx {
  uint32_t struct_size;
  uint32_t version;
  struct PonkoLiveCounts counts;
  size_t crowds;
  size_t instance_cells;
  size_t collision_meshes;
  size_t sprite_shaders;
  size_t sprite_materials;
  size_t sky_shaders;
} PonkoLiveCountsEx;

typedef struct PonkoLightBakeSettings {
  uint32_t struct_size;
  uint32_t version;
  float texels_per_unit;
  uint32_t samples;
  uint32_t bounces;
  uint32_t page_size;
  bool directional;
  bool denoise;
  float probe_spacing;
  uint64_t seed;
} PonkoLightBakeSettings;

typedef struct PonkoBakeModel {
  uint32_t struct_size;
  uint32_t version;
  const struct PonkoMeshData *mesh;
  struct PonkoTransform3D transform;
  struct PonkoColor albedo;
  struct PonkoColor emissive;
  bool double_sided;
  bool lightmapped;
} PonkoBakeModel;

typedef struct PonkoPointLight3D {
  struct PonkoColor color;
  float intensity;
  float range;
  bool shadows;
  uint32_t shadow_resolution;
} PonkoPointLight3D;

typedef struct PonkoSpotLight3D {
  struct PonkoColor color;
  float intensity;
  float range;
  float inner_angle;
  float outer_angle;
  bool shadows;
  uint32_t shadow_resolution;
} PonkoSpotLight3D;

typedef struct PonkoBakedLightingInfo {
  uint32_t struct_size;
  uint32_t version;
  uint32_t page_size;
  uint32_t pages;
  bool directional;
  uint64_t gpu_bytes;
  double seconds;
  uint64_t texels;
  uint64_t inside_texels;
  uint64_t probes;
  uint64_t rays;
} PonkoBakedLightingInfo;

typedef struct PonkoLightmapRect {
  uint32_t page;
  uint32_t x;
  uint32_t y;
  uint32_t width;
  uint32_t height;
} PonkoLightmapRect;

typedef struct PonkoLightmapped {
  uint32_t index;
} PonkoLightmapped;

typedef struct PonkoNavView {
  const struct PonkoNavMesh *navmesh;
} PonkoNavView;

typedef void (*PonkoPredictFn)(void*, size_t, const void*, size_t, void*);

typedef struct PonkoNetEvent {
  uint64_t from;
  uint64_t tick;
  uint32_t kind;
  const uint8_t *data;
  size_t size;
} PonkoNetEvent;

typedef struct PonkoSkyKit {
  uint32_t struct_size;
  uint32_t version;
  bool enabled;
  uint32_t quality;
  float coverage;
  float kind;
  struct PonkoVec2 wind;
  struct PonkoVec2 offset;
  float altitude;
  float thickness;
  float density;
  float self_shadow;
  float silver_lining;
  float shadows;
  bool moon_has_direction;
  struct PonkoVec3 moon_direction;
  float moon_phase;
  float moon_size;
  float moon_brightness;
} PonkoSkyKit;

typedef struct PonkoDayNight {
  uint32_t struct_size;
  uint32_t version;
  float hour;
  uint32_t day;
  float hours_per_second;
  float days_per_moon;
  float first_phase;
  struct PonkoColor sun_color;
  float sun_intensity;
  struct PonkoColor night_light;
} PonkoDayNight;

typedef struct PonkoVideoState {
  uint32_t struct_size;
  uint32_t version;
  struct PonkoTexture texture;
  uint32_t width;
  uint32_t height;
  float seconds;
  float time;
  float frame_time;
  float volume;
  bool playing;
  bool finished;
  bool looping;
  bool has_sound;
  struct PonkoSound sound;
  struct PonkoAudioBus bus;
} PonkoVideoState;

typedef struct PonkoWindowVideoModeEx {
  uint32_t struct_size;
  uint32_t version;
  uint32_t width;
  uint32_t height;
  uint32_t refresh_millihertz;
  uint32_t bit_depth;
} PonkoWindowVideoModeEx;

typedef struct PonkoWindowModeEx {
  uint32_t struct_size;
  uint32_t version;
  uint32_t kind;
  uint32_t monitor;
  uint32_t width;
  uint32_t height;
  uint32_t refresh_millihertz;
  uint32_t bit_depth;
} PonkoWindowModeEx;

typedef struct PonkoWindowPropertiesEx {
  uint32_t struct_size;
  uint32_t version;
  struct PonkoVec2 size;
  int32_t position_x;
  int32_t position_y;
  bool has_position;
  struct PonkoVec2 min_size;
  struct PonkoVec2 max_size;
  bool has_min_size;
  bool has_max_size;
  bool always_on_top;
  bool decorations;
} PonkoWindowPropertiesEx;

typedef struct PonkoWindowImageEx {
  uint32_t struct_size;
  uint32_t version;
  uint32_t width;
  uint32_t height;
  const uint8_t *rgba;
  size_t rgba_len;
  uint32_t hotspot_x;
  uint32_t hotspot_y;
} PonkoWindowImageEx;

typedef struct PonkoWindowMonitorEx {
  uint32_t struct_size;
  uint32_t version;
  int32_t position_x;
  int32_t position_y;
  uint32_t width;
  uint32_t height;
  float scale_factor;
  uint32_t refresh_millihertz;
} PonkoWindowMonitorEx;

typedef struct PonkoWindowStateEx {
  uint32_t struct_size;
  uint32_t version;
  uint32_t width;
  uint32_t height;
  float scale_factor;
  bool focused;
  bool minimized;
  uint32_t monitor;
} PonkoWindowStateEx;

typedef struct PonkoWindowEventEx {
  uint32_t struct_size;
  uint32_t version;
  uint32_t kind;
  bool value;
  float scale_factor;
} PonkoWindowEventEx;

typedef struct PonkoAnimation {
  struct PonkoSpriteSheet sheet;
  uint32_t first;
  uint32_t frames;
  float fps;
  bool looping;
  float time;
} PonkoAnimation;

typedef struct PonkoAttached {
  struct PonkoEntity to;
  struct PonkoVec2 offset;
  float rotation;
} PonkoAttached;

typedef struct PonkoAttached3D {
  struct PonkoEntity to;
  uint32_t reserved[2];
  struct PonkoTransform3D offset;
} PonkoAttached3D;

typedef struct PonkoVelocity {
  struct PonkoVec2 linear;
  float angular;
} PonkoVelocity;

typedef struct PonkoReceivesDecals {
  uint32_t layers;
} PonkoReceivesDecals;

typedef struct PonkoVelocity3D {
  struct PonkoVec3 linear;
  struct PonkoVec3 angular;
} PonkoVelocity3D;

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

struct PonkoWorld *ponko_world_new(void);

enum PonkoStatus ponko_world_free(struct PonkoWorld *world);

const char *ponko_last_error(void);

enum PonkoErrorKind ponko_last_error_kind(void);

const char *ponko_last_error_path(void);

const char *ponko_error_kind_name(uint32_t kind);

uint32_t ponko_abi_version(void);

enum PonkoStatus ponko_component_register(struct PonkoWorld *world,
                                          const char *name,
                                          size_t size,
                                          size_t alignment,
                                          uint32_t *out_id);

enum PonkoStatus ponko_transform_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_sprite_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_spawn(struct PonkoWorld *world,
                             const struct PonkoValue *values,
                             size_t count,
                             uint64_t *out_entity);

enum PonkoStatus ponko_despawn(struct PonkoWorld *world, uint64_t entity);

enum PonkoStatus ponko_is_alive(struct PonkoWorld *world, uint64_t entity, bool *out_alive);

enum PonkoStatus ponko_world_len(struct PonkoWorld *world, size_t *out_len);

enum PonkoStatus ponko_get(struct PonkoWorld *world,
                           uint64_t entity,
                           uint32_t id,
                           void *out_data,
                           size_t size);

enum PonkoStatus ponko_set(struct PonkoWorld *world,
                           uint64_t entity,
                           uint32_t id,
                           const void *data,
                           size_t size);

enum PonkoStatus ponko_remove(struct PonkoWorld *world, uint64_t entity, uint32_t id);

enum PonkoStatus ponko_world_sort_spatial(struct PonkoWorld *world, float cell_size);

enum PonkoStatus ponko_query(struct PonkoWorld *world,
                             const uint32_t *components,
                             size_t count,
                             PonkoQueryFn callback,
                             void *user);

enum PonkoStatus ponko_query_par(struct PonkoWorld *world,
                                 const uint32_t *components,
                                 size_t count,
                                 PonkoQueryFn callback,
                                 void *user);

struct PonkoAccess *ponko_access_new(void);

void ponko_access_free(struct PonkoAccess *access);

enum PonkoStatus ponko_access_read_component(struct PonkoAccess *access, const char *name);

enum PonkoStatus ponko_access_write_component(struct PonkoAccess *access, const char *name);

enum PonkoStatus ponko_access_read_resource(struct PonkoAccess *access, const char *name);

enum PonkoStatus ponko_access_write_resource(struct PonkoAccess *access, const char *name);

enum PonkoStatus ponko_access_read_events(struct PonkoAccess *access, const char *name);

enum PonkoStatus ponko_access_send_events(struct PonkoAccess *access, const char *name);

enum PonkoStatus ponko_access_commands(struct PonkoAccess *access);

enum PonkoStatus ponko_access_service(struct PonkoAccess *access, uint32_t service, bool write);

enum PonkoStatus ponko_access_conflicts(const struct PonkoAccess *earlier,
                                        const struct PonkoAccess *later,
                                        bool ordered,
                                        size_t *out_count);

enum PonkoStatus ponko_game_add_system_with_access(struct PonkoGame *game,
                                                   uint32_t phase_raw,
                                                   const char *name,
                                                   uint32_t every,
                                                   const struct PonkoAccess *access,
                                                   PonkoSystemFn system_fn,
                                                   void *user);

enum PonkoStatus ponko_game_access_report(struct PonkoGame *game,
                                          char *buffer,
                                          size_t capacity,
                                          size_t *out_len);

enum PonkoStatus ponko_game_parallel_systems(struct PonkoGame *game, bool on);

enum PonkoStatus ponko_game_schedule_plan(struct PonkoGame *game,
                                          char *buffer,
                                          size_t capacity,
                                          size_t *out_len);

struct PonkoActionMap *ponko_action_map_new(void);

void ponko_action_map_free(struct PonkoActionMap *map);

enum PonkoStatus ponko_action_map_set(struct PonkoActionMap *map,
                                      const char *name,
                                      uint32_t kind,
                                      const struct PonkoActionBinding *bindings,
                                      size_t count);

enum PonkoStatus ponko_action_map_remove(struct PonkoActionMap *map, const char *name);

enum PonkoStatus ponko_action_map_load(struct PonkoActionMap *map, const char *json);

enum PonkoStatus ponko_action_map_save(struct PonkoActionMap *map,
                                       char *buffer,
                                       size_t capacity,
                                       size_t *out_len);

enum PonkoStatus ponko_ctx_set_actions(struct PonkoCtx *ctx, struct PonkoActionMap *map);

enum PonkoStatus ponko_ctx_actions(struct PonkoCtx *ctx, struct PonkoActionMap *map);

enum PonkoStatus ponko_ctx_feed_input(struct PonkoCtx *ctx,
                                      uint32_t kind,
                                      uint32_t slot,
                                      uint32_t input,
                                      float value);

enum PonkoStatus ponko_ctx_feed_mouse_move(struct PonkoCtx *ctx, struct PonkoVec2 position);

enum PonkoStatus ponko_ctx_feed_mouse_wheel(struct PonkoCtx *ctx, struct PonkoVec2 lines);

enum PonkoStatus ponko_ctx_action_down(struct PonkoCtx *ctx, const char *name, bool *value);

enum PonkoStatus ponko_ctx_action_pressed(struct PonkoCtx *ctx, const char *name, bool *value);

enum PonkoStatus ponko_ctx_action_released(struct PonkoCtx *ctx, const char *name, bool *value);

enum PonkoStatus ponko_ctx_action_axis(struct PonkoCtx *ctx, const char *name, float *value);

enum PonkoStatus ponko_ctx_action_axis2(struct PonkoCtx *ctx,
                                        const char *name,
                                        struct PonkoVec2 *value);

enum PonkoStatus ponko_ctx_action_id(struct PonkoCtx *ctx, const char *name, uint32_t *out_id);

enum PonkoStatus ponko_ctx_action_ids(struct PonkoCtx *ctx,
                                      const char *const *names,
                                      size_t count,
                                      uint32_t *out_ids);

enum PonkoStatus ponko_ctx_action_down_id(struct PonkoCtx *ctx, uint32_t action, bool *value);

enum PonkoStatus ponko_ctx_action_pressed_id(struct PonkoCtx *ctx, uint32_t action, bool *value);

enum PonkoStatus ponko_ctx_action_released_id(struct PonkoCtx *ctx, uint32_t action, bool *value);

enum PonkoStatus ponko_ctx_action_axis_id(struct PonkoCtx *ctx, uint32_t action, float *value);

enum PonkoStatus ponko_ctx_action_axis2_id(struct PonkoCtx *ctx,
                                           uint32_t action,
                                           struct PonkoVec2 *value);

enum PonkoStatus ponko_ai_key(const char *name, uint64_t *out_key);

enum PonkoStatus ponko_ai_key_name(uint64_t key, char *buffer, size_t capacity, size_t *out_len);

struct PonkoBehaviorTree *ponko_behavior_tree_new(const char *json);

struct PonkoBehaviorTree *ponko_behavior_tree_new_linked(const char *json,
                                                         const char *const *names,
                                                         const struct PonkoBehaviorTree *const *subtrees,
                                                         size_t count);

void ponko_behavior_tree_free(struct PonkoBehaviorTree *tree);

enum PonkoStatus ponko_behavior_tree_len(const struct PonkoBehaviorTree *tree, size_t *out_len);

struct PonkoBrain *ponko_brain_new(uint32_t period);

void ponko_brain_free(struct PonkoBrain *brain);

enum PonkoStatus ponko_brain_think(struct PonkoBrain *brain,
                                   const struct PonkoBehaviorTree *tree,
                                   uint64_t tick,
                                   float dt);

enum PonkoStatus ponko_brain_due(const struct PonkoBrain *brain,
                                 uint32_t index,
                                 uint64_t tick,
                                 bool *out_due);

enum PonkoStatus ponko_brain_set_period(struct PonkoBrain *brain, uint32_t period);

enum PonkoStatus ponko_brain_action(const struct PonkoBrain *brain, uint64_t *out_key);

enum PonkoStatus ponko_brain_started(const struct PonkoBrain *brain, bool *out_started);

enum PonkoStatus ponko_brain_param_num(const struct PonkoBrain *brain,
                                       uint64_t key,
                                       float *out_value);

enum PonkoStatus ponko_brain_finish(struct PonkoBrain *brain, enum PonkoAiStatus status);

enum PonkoStatus ponko_brain_status(const struct PonkoBrain *brain, enum PonkoAiStatus *out_status);

enum PonkoStatus ponko_brain_wake(struct PonkoBrain *brain);

enum PonkoStatus ponko_brain_signal(struct PonkoBrain *brain, uint64_t key);

enum PonkoStatus ponko_brain_set_seed(struct PonkoBrain *brain, uint64_t seed);

enum PonkoStatus ponko_brain_save_json(const struct PonkoBrain *brain,
                                       char *buffer,
                                       size_t capacity,
                                       size_t *out_len);

struct PonkoBrain *ponko_brain_load_json(const char *json);

enum PonkoStatus ponko_brain_set_num(struct PonkoBrain *brain, uint64_t key, float value);

enum PonkoStatus ponko_brain_set_flag(struct PonkoBrain *brain, uint64_t key, bool value);

enum PonkoStatus ponko_brain_set_vec2(struct PonkoBrain *brain,
                                      uint64_t key,
                                      struct PonkoVec2 value);

enum PonkoStatus ponko_brain_set_vec3(struct PonkoBrain *brain,
                                      uint64_t key,
                                      struct PonkoVec3 value);

enum PonkoStatus ponko_brain_set_entity(struct PonkoBrain *brain, uint64_t key, uint64_t entity);

enum PonkoStatus ponko_brain_num(const struct PonkoBrain *brain, uint64_t key, float *out_value);

enum PonkoStatus ponko_brain_flag(const struct PonkoBrain *brain, uint64_t key, bool *out_value);

enum PonkoStatus ponko_brain_blackboard_json(const struct PonkoBrain *brain,
                                             char *buffer,
                                             size_t capacity,
                                             size_t *out_len);

enum PonkoStatus ponko_brain_trace(const struct PonkoBrain *brain,
                                   char *buffer,
                                   size_t capacity,
                                   size_t *out_len);

float ponko_ai_curve(enum PonkoAiCurve curve, float param, float x);

float ponko_ai_combine(const float *scores, size_t count);

enum PonkoStatus ponko_brain_action_at(const struct PonkoBrain *brain,
                                       size_t index,
                                       uint64_t *out_key);

enum PonkoStatus ponko_brain_action_started(const struct PonkoBrain *brain,
                                            uint64_t action,
                                            bool *out_started);

enum PonkoStatus ponko_brain_finish_action(struct PonkoBrain *brain,
                                           uint64_t action,
                                           enum PonkoAiStatus status);

enum PonkoStatus ponko_brain_action_param_num(const struct PonkoBrain *brain,
                                              uint64_t action,
                                              uint64_t key,
                                              float *out_value);

enum PonkoStatus ponko_ctx_ai_load_tree(struct PonkoCtx *ctx, const char *path);

enum PonkoStatus ponko_ctx_ai_add_tree(struct PonkoCtx *ctx,
                                       const char *name,
                                       const struct PonkoBehaviorTree *tree);

enum PonkoStatus ponko_ctx_ai_add_brain(struct PonkoCtx *ctx,
                                        uint64_t entity,
                                        const char *tree,
                                        uint32_t period);

enum PonkoStatus ponko_ctx_ai_think(struct PonkoCtx *ctx);

enum PonkoStatus ponko_ctx_ai_signal(struct PonkoCtx *ctx, uint64_t target, uint64_t key);

enum PonkoStatus ponko_ctx_ai_signal_num(struct PonkoCtx *ctx,
                                         uint64_t target,
                                         uint64_t key,
                                         float value);

enum PonkoStatus ponko_ctx_ai_signal_entity(struct PonkoCtx *ctx,
                                            uint64_t target,
                                            uint64_t key,
                                            uint64_t value);

enum PonkoStatus ponko_ctx_ai_signal_vec2(struct PonkoCtx *ctx,
                                          uint64_t target,
                                          uint64_t key,
                                          struct PonkoVec2 value);

enum PonkoStatus ponko_ctx_ai_signal_vec3(struct PonkoCtx *ctx,
                                          uint64_t target,
                                          uint64_t key,
                                          struct PonkoVec3 value);

enum PonkoStatus ponko_ctx_ai_action(struct PonkoCtx *ctx,
                                     uint64_t entity,
                                     size_t index,
                                     uint64_t *out_key);

enum PonkoStatus ponko_ctx_ai_started(struct PonkoCtx *ctx,
                                      uint64_t entity,
                                      uint64_t action,
                                      bool *out_started);

enum PonkoStatus ponko_ctx_ai_finish(struct PonkoCtx *ctx,
                                     uint64_t entity,
                                     uint64_t action,
                                     enum PonkoAiStatus status);

enum PonkoStatus ponko_ctx_ai_param_num(struct PonkoCtx *ctx,
                                        uint64_t entity,
                                        uint64_t action,
                                        uint64_t key,
                                        float *out_value);

enum PonkoStatus ponko_ctx_ai_set_num(struct PonkoCtx *ctx,
                                      uint64_t entity,
                                      uint64_t key,
                                      float value);

enum PonkoStatus ponko_ctx_ai_set_flag(struct PonkoCtx *ctx,
                                       uint64_t entity,
                                       uint64_t key,
                                       bool value);

enum PonkoStatus ponko_ctx_ai_set_vec2(struct PonkoCtx *ctx,
                                       uint64_t entity,
                                       uint64_t key,
                                       struct PonkoVec2 value);

enum PonkoStatus ponko_ctx_ai_set_vec3(struct PonkoCtx *ctx,
                                       uint64_t entity,
                                       uint64_t key,
                                       struct PonkoVec3 value);

enum PonkoStatus ponko_ctx_ai_set_entity(struct PonkoCtx *ctx,
                                         uint64_t entity,
                                         uint64_t key,
                                         uint64_t value);

enum PonkoStatus ponko_ctx_ai_num(struct PonkoCtx *ctx,
                                  uint64_t entity,
                                  uint64_t key,
                                  float *out_value);

enum PonkoStatus ponko_ctx_ai_flag(struct PonkoCtx *ctx,
                                   uint64_t entity,
                                   uint64_t key,
                                   bool *out_value);

enum PonkoStatus ponko_motion_clip(const struct PonkoClip *clip, struct PonkoMotion **out_motion);

enum PonkoStatus ponko_motion_blend1d(size_t parameter, struct PonkoMotion **out_motion);

enum PonkoStatus ponko_motion_blend2d(size_t x, size_t y, struct PonkoMotion **out_motion);

enum PonkoStatus ponko_motion_blend1d_add(struct PonkoMotion *tree,
                                          float threshold,
                                          struct PonkoMotion *child,
                                          float time_scale);

enum PonkoStatus ponko_motion_blend2d_add(struct PonkoMotion *tree,
                                          float x,
                                          float y,
                                          struct PonkoMotion *child,
                                          float time_scale);

void ponko_motion_free(struct PonkoMotion *motion);

struct PonkoAnimationGraphBuilder *ponko_animation_graph_builder_new(void);

void ponko_animation_graph_builder_free(struct PonkoAnimationGraphBuilder *builder);

enum PonkoStatus ponko_animation_graph_add_float(struct PonkoAnimationGraphBuilder *builder,
                                                 float value,
                                                 size_t *out_index);

enum PonkoStatus ponko_animation_graph_add_bool(struct PonkoAnimationGraphBuilder *builder,
                                                bool value,
                                                size_t *out_index);

enum PonkoStatus ponko_animation_graph_add_trigger(struct PonkoAnimationGraphBuilder *builder,
                                                   size_t *out_index);

enum PonkoStatus ponko_animation_graph_add_state(struct PonkoAnimationGraphBuilder *builder_ptr,
                                                 struct PonkoMotion *motion,
                                                 float speed,
                                                 int64_t speed_parameter,
                                                 bool looping,
                                                 size_t *out_index);

enum PonkoStatus ponko_animation_graph_add_event(struct PonkoAnimationGraphBuilder *builder_ptr,
                                                 size_t state,
                                                 float time,
                                                 const char *name);

enum PonkoStatus ponko_animation_graph_add_transition(struct PonkoAnimationGraphBuilder *builder_ptr,
                                                      int64_t from,
                                                      size_t to,
                                                      float exit_time,
                                                      float duration,
                                                      float offset,
                                                      size_t *out_index);

enum PonkoStatus ponko_animation_graph_add_condition(struct PonkoAnimationGraphBuilder *builder_ptr,
                                                     size_t transition,
                                                     enum PonkoAnimationCondition kind,
                                                     size_t parameter,
                                                     float value);

enum PonkoStatus ponko_animation_graph_build(struct PonkoAnimationGraphBuilder *builder_ptr,
                                             size_t initial,
                                             struct PonkoAnimationGraph **out_graph);

void ponko_animation_graph_free(struct PonkoAnimationGraph *graph);

enum PonkoStatus ponko_ctx_set_animator(struct PonkoCtx *ctx,
                                        uint64_t entity_bits,
                                        const struct PonkoAnimationGraph *graph);

enum PonkoStatus ponko_ctx_animator_set_float(struct PonkoCtx *ctx,
                                              uint64_t entity_bits,
                                              size_t parameter,
                                              float value);

enum PonkoStatus ponko_ctx_animator_set_bool(struct PonkoCtx *ctx,
                                             uint64_t entity_bits,
                                             size_t parameter,
                                             bool value);

enum PonkoStatus ponko_ctx_animator_set_trigger(struct PonkoCtx *ctx,
                                                uint64_t entity_bits,
                                                size_t parameter,
                                                bool value);

enum PonkoStatus ponko_ctx_animator_state(struct PonkoCtx *ctx,
                                          uint64_t entity_bits,
                                          size_t *out_state,
                                          float *out_normalized_time);

enum PonkoStatus ponko_ctx_animator_normalized_time_f64(struct PonkoCtx *ctx,
                                                        uint64_t entity_bits,
                                                        double *out_time);

enum PonkoStatus ponko_ctx_animator_crossfade(struct PonkoCtx *ctx,
                                              uint64_t entity_bits,
                                              size_t state,
                                              float duration,
                                              float offset);

enum PonkoStatus ponko_ctx_animator_set_manual(struct PonkoCtx *ctx,
                                               uint64_t entity_bits,
                                               bool manual);

enum PonkoStatus ponko_ctx_animator_update(struct PonkoCtx *ctx, uint64_t entity_bits, float dt);

enum PonkoStatus ponko_ctx_animator_next_event(struct PonkoCtx *ctx,
                                               uint64_t entity_bits,
                                               char *buffer,
                                               size_t capacity,
                                               bool *out_found);

enum PonkoStatus ponko_ctx_animator_add_layer(struct PonkoCtx *ctx,
                                              uint64_t entity_bits,
                                              const struct PonkoAnimationGraph *graph,
                                              uint32_t mode,
                                              const char *mask_joint,
                                              size_t *out_layer);

enum PonkoStatus ponko_ctx_animator_set_layer_weight(struct PonkoCtx *ctx,
                                                     uint64_t entity_bits,
                                                     size_t layer,
                                                     float weight);

enum PonkoStatus ponko_ctx_animator_layer_set_float(struct PonkoCtx *ctx,
                                                    uint64_t entity_bits,
                                                    size_t layer_index,
                                                    size_t parameter,
                                                    float value);

enum PonkoStatus ponko_ctx_animator_layer_set_bool(struct PonkoCtx *ctx,
                                                   uint64_t entity_bits,
                                                   size_t layer_index,
                                                   size_t parameter,
                                                   bool value);

enum PonkoStatus ponko_ctx_animator_layer_set_trigger(struct PonkoCtx *ctx,
                                                      uint64_t entity_bits,
                                                      size_t layer_index,
                                                      size_t parameter,
                                                      bool value);

enum PonkoStatus ponko_ctx_animator_layer_crossfade(struct PonkoCtx *ctx,
                                                    uint64_t entity_bits,
                                                    size_t layer_index,
                                                    size_t state,
                                                    float duration,
                                                    float offset);

enum PonkoStatus ponko_ctx_animator_set_root_motion(struct PonkoCtx *ctx,
                                                    uint64_t entity_bits,
                                                    uint32_t mode,
                                                    size_t root_joint);

enum PonkoStatus ponko_ctx_animator_root_motion(struct PonkoCtx *ctx,
                                                uint64_t entity_bits,
                                                struct PonkoVec3 *out_position,
                                                struct PonkoQuat *out_rotation);

struct PonkoAssetGroup *ponko_asset_group_new(const char *name);

void ponko_asset_group_free(struct PonkoAssetGroup *group);

enum PonkoStatus ponko_asset_group_add(struct PonkoAssetGroup *group,
                                       uint32_t kind,
                                       const char *path);

enum PonkoStatus ponko_ctx_start_asset_group(struct PonkoCtx *ctx,
                                             struct PonkoAssetGroup *group,
                                             uint32_t *out_id);

enum PonkoStatus ponko_ctx_find_asset_group(struct PonkoCtx *ctx,
                                            const char *name,
                                            uint32_t *out_id);

enum PonkoStatus ponko_ctx_asset_group_state(struct PonkoCtx *ctx,
                                             uint32_t id,
                                             uint32_t *out_state);

enum PonkoStatus ponko_ctx_asset_group_progress(struct PonkoCtx *ctx,
                                                uint32_t id,
                                                struct PonkoAssetGroupProgress *out_progress);

enum PonkoStatus ponko_ctx_asset_group_error(struct PonkoCtx *ctx, uint32_t id);

enum PonkoStatus ponko_ctx_cancel_asset_group(struct PonkoCtx *ctx, uint32_t id);

enum PonkoStatus ponko_ctx_restart_asset_group(struct PonkoCtx *ctx, uint32_t id);

enum PonkoStatus ponko_ctx_take_asset_group_bytes(struct PonkoCtx *ctx,
                                                  uint32_t id,
                                                  const char *path,
                                                  PonkoAssetDecodeFn decode,
                                                  void *user);

enum PonkoStatus ponko_ctx_set_asset_group_frame_budget(struct PonkoCtx *ctx, float milliseconds);

enum PonkoStatus ponko_ctx_asset_group_stats(struct PonkoCtx *ctx,
                                             struct PonkoAssetGroupStats *out_stats);

enum PonkoStatus ponko_ctx_add_background_memory_source(struct PonkoCtx *ctx,
                                                        const struct PonkoMemorySource *source);

enum PonkoStatus ponko_game_loading_state(struct PonkoGame *game,
                                          uint32_t loading,
                                          const char *name,
                                          PonkoAssetGroupBuildFn build_fn,
                                          void *user,
                                          uint32_t then);

enum PonkoStatus ponko_ctx_add_asset_source(struct PonkoCtx *ctx,
                                            const char *id,
                                            PonkoAssetReadFn read,
                                            PonkoAssetContainsFn contains,
                                            PonkoUserFreeFn free_user,
                                            void *user);

struct PonkoMemorySource *ponko_memory_source_new(const char *id);

void ponko_memory_source_free(struct PonkoMemorySource *source);

enum PonkoStatus ponko_memory_source_insert(struct PonkoMemorySource *source,
                                            const char *name,
                                            const uint8_t *data,
                                            size_t len);

enum PonkoStatus ponko_ctx_add_memory_source(struct PonkoCtx *ctx,
                                             const struct PonkoMemorySource *source);

enum PonkoStatus ponko_ctx_load_asset_with(struct PonkoCtx *ctx,
                                           const char *name,
                                           PonkoAssetDecodeFn decode,
                                           void *user);

enum PonkoStatus ponko_attached_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_game_follow_attachments(struct PonkoGame *game);

enum PonkoStatus ponko_attached3d_component(struct PonkoWorld *world, uint32_t *out_id);

struct PonkoAudioEffects ponko_audio_effects_default(void);

struct PonkoAudioSettings3D ponko_audio_settings3d_default(void);

enum PonkoStatus ponko_ctx_audio_bus(struct PonkoCtx *ctx,
                                     const char *name,
                                     struct PonkoAudioBus *out_bus);

enum PonkoStatus ponko_ctx_set_audio_bus_volume(struct PonkoCtx *ctx,
                                                struct PonkoAudioBus bus,
                                                float volume);

enum PonkoStatus ponko_ctx_mute_audio_bus(struct PonkoCtx *ctx,
                                          struct PonkoAudioBus bus,
                                          bool muted);

enum PonkoStatus ponko_ctx_play_sound_on(struct PonkoCtx *ctx,
                                         struct PonkoSound sound,
                                         const struct PonkoSoundParams *params,
                                         struct PonkoAudioBus bus,
                                         const struct PonkoAudioEffects *effects,
                                         struct PonkoPlaying *out_playing);

enum PonkoStatus ponko_ctx_set_sound_effects(struct PonkoCtx *ctx,
                                             struct PonkoPlaying playing,
                                             const struct PonkoAudioEffects *effects);

enum PonkoStatus ponko_ctx_load_stream(struct PonkoCtx *ctx,
                                       const char *path,
                                       struct PonkoSound *out_sound);

enum PonkoStatus ponko_ctx_play_sound_at(struct PonkoCtx *ctx,
                                         struct PonkoSound sound,
                                         struct PonkoVec3 position,
                                         const struct PonkoAudioSettings3D *settings,
                                         struct PonkoPlaying *out_playing);

enum PonkoStatus ponko_ctx_set_audio_source3d(struct PonkoCtx *ctx,
                                              uint64_t entity_bits,
                                              struct PonkoSound sound,
                                              const struct PonkoAudioSettings3D *settings);

enum PonkoStatus ponko_ctx_remove_audio_source3d(struct PonkoCtx *ctx, uint64_t entity_bits);

enum PonkoStatus ponko_ctx_set_audio_listener(struct PonkoCtx *ctx,
                                              const struct PonkoAudioListener3D *listener);

enum PonkoStatus ponko_ctx_audio_listener(struct PonkoCtx *ctx,
                                          struct PonkoAudioListener3D *out_listener);

struct PonkoBusEffect ponko_bus_effect_default(enum PonkoBusEffectKind kind);

struct PonkoDucking ponko_ducking_default(struct PonkoAudioBus key);

enum PonkoStatus ponko_ctx_set_audio_bus_parent(struct PonkoCtx *ctx,
                                                struct PonkoAudioBus bus,
                                                const struct PonkoAudioBus *parent);

enum PonkoStatus ponko_ctx_audio_bus_parent(struct PonkoCtx *ctx,
                                            struct PonkoAudioBus bus,
                                            struct PonkoAudioBus *out_parent,
                                            bool *out_has_parent);

enum PonkoStatus ponko_ctx_audio_bus_volume(struct PonkoCtx *ctx,
                                            struct PonkoAudioBus bus,
                                            float *out_volume);

enum PonkoStatus ponko_ctx_audio_bus_gain(struct PonkoCtx *ctx,
                                          struct PonkoAudioBus bus,
                                          float *out_gain);

enum PonkoStatus ponko_ctx_audio_bus_muted(struct PonkoCtx *ctx,
                                           struct PonkoAudioBus bus,
                                           bool *out_muted);

enum PonkoStatus ponko_ctx_set_audio_bus_effects(struct PonkoCtx *ctx,
                                                 struct PonkoAudioBus bus,
                                                 const struct PonkoBusEffect *effects,
                                                 size_t count);

enum PonkoStatus ponko_ctx_audio_bus_effects(struct PonkoCtx *ctx,
                                             struct PonkoAudioBus bus,
                                             struct PonkoBusEffect *buffer,
                                             size_t capacity,
                                             size_t *out_count);

enum PonkoStatus ponko_ctx_set_audio_bus_ducking(struct PonkoCtx *ctx,
                                                 struct PonkoAudioBus bus,
                                                 const struct PonkoDucking *ducking,
                                                 size_t count);

enum PonkoStatus ponko_ctx_audio_bus_ducking(struct PonkoCtx *ctx,
                                             struct PonkoAudioBus bus,
                                             struct PonkoDucking *buffer,
                                             size_t capacity,
                                             size_t *out_count);

enum PonkoStatus ponko_ctx_add_audio_snapshot(struct PonkoCtx *ctx,
                                              const char *name,
                                              const struct PonkoBusSnapshot *buses,
                                              size_t count);

enum PonkoStatus ponko_ctx_start_audio_snapshot(struct PonkoCtx *ctx, const char *name, float fade);

enum PonkoStatus ponko_ctx_stop_audio_snapshot(struct PonkoCtx *ctx, const char *name, float fade);

enum PonkoStatus ponko_ctx_audio_snapshot_weight(struct PonkoCtx *ctx,
                                                 const char *name,
                                                 float *out_weight);

enum PonkoStatus ponko_ctx_load_audio_mixer(struct PonkoCtx *ctx, const char *path);

enum PonkoStatus ponko_ctx_apply_audio_mixer_json(struct PonkoCtx *ctx, const char *json);

enum PonkoStatus ponko_ctx_audio_mixer_state(struct PonkoCtx *ctx,
                                             char *buffer,
                                             size_t capacity,
                                             size_t *out_len);

enum PonkoStatus ponko_ctx_restore_audio_mixer_state(struct PonkoCtx *ctx, const char *json);

enum PonkoStatus ponko_ctx_set_sound_volume(struct PonkoCtx *ctx,
                                            struct PonkoPlaying playing,
                                            float volume,
                                            float seconds);

enum PonkoStatus ponko_ctx_set_sound_speed(struct PonkoCtx *ctx,
                                           struct PonkoPlaying playing,
                                           float speed,
                                           float seconds);

enum PonkoStatus ponko_ctx_fade_out_sound(struct PonkoCtx *ctx,
                                          struct PonkoPlaying playing,
                                          float seconds);

enum PonkoStatus ponko_ctx_set_sound_priority(struct PonkoCtx *ctx,
                                              struct PonkoPlaying playing,
                                              uint8_t priority);

enum PonkoStatus ponko_ctx_sound_is_virtual(struct PonkoCtx *ctx,
                                            struct PonkoPlaying playing,
                                            bool *out_virtual);

enum PonkoStatus ponko_ctx_audio_voice_count(struct PonkoCtx *ctx,
                                             uint32_t *out_real,
                                             uint32_t *out_virtual);

enum PonkoStatus ponko_ctx_set_audio_voice_limit(struct PonkoCtx *ctx, uint32_t limit);

enum PonkoStatus ponko_ctx_audio_voice_limit(struct PonkoCtx *ctx, uint32_t *out_limit);

enum PonkoStatus ponko_ctx_set_audio_bus_voice_limit(struct PonkoCtx *ctx,
                                                     struct PonkoAudioBus bus,
                                                     uint32_t limit);

enum PonkoStatus ponko_ctx_audio_bus_voice_limit(struct PonkoCtx *ctx,
                                                 struct PonkoAudioBus bus,
                                                 uint32_t *out_limit);

enum PonkoStatus ponko_ctx_set_sound_loop(struct PonkoCtx *ctx,
                                          struct PonkoSound sound,
                                          float start,
                                          float end);

enum PonkoStatus ponko_ctx_sound_loop(struct PonkoCtx *ctx,
                                      struct PonkoSound sound,
                                      float *out_start,
                                      float *out_end);

enum PonkoStatus ponko_ctx_set_audio_source3d_priority(struct PonkoCtx *ctx,
                                                       uint64_t entity_bits,
                                                       uint8_t priority);

enum PonkoStatus ponko_ctx_pause_audio_bus(struct PonkoCtx *ctx,
                                           struct PonkoAudioBus bus,
                                           bool paused);

enum PonkoStatus ponko_ctx_audio_bus_paused(struct PonkoCtx *ctx,
                                            struct PonkoAudioBus bus,
                                            bool *out_paused);

enum PonkoStatus ponko_game_pause_audio_in_state(struct PonkoGame *game,
                                                 uint32_t state,
                                                 const char *const *buses,
                                                 size_t count);

enum PonkoStatus ponko_ctx_play_music(struct PonkoCtx *ctx,
                                      const struct PonkoSound *layers,
                                      const float *volumes,
                                      size_t count,
                                      float bpm,
                                      uint32_t beats_per_bar,
                                      float fade,
                                      enum PonkoQuantize at);

enum PonkoStatus ponko_ctx_stop_music(struct PonkoCtx *ctx, float fade, enum PonkoQuantize at);

enum PonkoStatus ponko_ctx_set_music_layer(struct PonkoCtx *ctx,
                                           size_t layer,
                                           float volume,
                                           float fade,
                                           enum PonkoQuantize at);

enum PonkoStatus ponko_ctx_play_stinger(struct PonkoCtx *ctx,
                                        struct PonkoSound sound,
                                        enum PonkoQuantize at,
                                        struct PonkoPlaying *out_playing);

enum PonkoStatus ponko_ctx_set_music_speed(struct PonkoCtx *ctx, float speed, float fade);

enum PonkoStatus ponko_ctx_set_music_bus(struct PonkoCtx *ctx, struct PonkoAudioBus bus);

enum PonkoStatus ponko_ctx_music_bus(struct PonkoCtx *ctx, struct PonkoAudioBus *out_bus);

enum PonkoStatus ponko_ctx_music_beat(struct PonkoCtx *ctx, double *out_beat, bool *out_playing);

enum PonkoStatus ponko_ctx_music_layer_volume(struct PonkoCtx *ctx,
                                              size_t layer,
                                              float *out_volume);

enum PonkoStatus ponko_ctx_audio_output_devices(struct PonkoCtx *ctx,
                                                char *buffer,
                                                size_t capacity,
                                                size_t *out_len);

enum PonkoStatus ponko_ctx_set_audio_output_device(struct PonkoCtx *ctx, const char *device);

enum PonkoStatus ponko_ctx_audio_output_device(struct PonkoCtx *ctx,
                                               char *buffer,
                                               size_t capacity,
                                               size_t *out_len);

enum PonkoStatus ponko_ctx_audio_device_events(struct PonkoCtx *ctx,
                                               char *buffer,
                                               size_t capacity,
                                               size_t *out_len);

enum PonkoStatus ponko_ctx_open_audio_offline(struct PonkoCtx *ctx, uint32_t sample_rate);

enum PonkoStatus ponko_ctx_render_audio_offline(struct PonkoCtx *ctx,
                                                float seconds,
                                                float *frames,
                                                size_t capacity,
                                                size_t *out_count);

struct PonkoQueryOptions ponko_query_options_default(void);

enum PonkoStatus ponko_component_track_changes(struct PonkoWorld *world, uint32_t id);

enum PonkoStatus ponko_world_change_tick(struct PonkoWorld *world, uint64_t *out_tick);

enum PonkoStatus ponko_world_advance_change_tick(struct PonkoWorld *world, uint64_t *out_tick);

enum PonkoStatus ponko_world_changes_since(struct PonkoWorld *world, uint64_t *out_tick);

enum PonkoStatus ponko_world_set_changes_since(struct PonkoWorld *world, uint64_t tick);

enum PonkoStatus ponko_mark_changed(struct PonkoWorld *world, uint64_t entity, uint32_t id);

enum PonkoStatus ponko_removed(struct PonkoWorld *world,
                               uint32_t id,
                               uint64_t since,
                               uint64_t *out_entities,
                               size_t capacity,
                               size_t *out_count);

enum PonkoStatus ponko_query_with(struct PonkoWorld *world,
                                  const uint32_t *components,
                                  size_t count,
                                  const struct PonkoQueryOptions *options,
                                  PonkoQueryFn callback,
                                  void *user);

struct PonkoContactModifier3D ponko_contact_modifier3d_default(void);

struct PonkoContactModifier3D ponko_contact_modifier3d_one_way(struct PonkoVec3 up);

struct PonkoContactModifier3D ponko_contact_modifier3d_conveyor(struct PonkoVec3 velocity);

struct PonkoContactModifier3D ponko_contact_modifier3d_hook(void);

enum PonkoStatus ponko_contact_modifier3d_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_ctx_set_contact_hook3d(struct PonkoCtx *ctx,
                                              PonkoContactHook3D hook,
                                              void *user);

enum PonkoStatus ponko_contact_pair3d_entities(struct PonkoContactPair3D *pair,
                                               uint64_t *out_a,
                                               uint64_t *out_b);

enum PonkoStatus ponko_contact_pair3d_normal(struct PonkoContactPair3D *pair,
                                             struct PonkoVec3 *out_normal);

enum PonkoStatus ponko_contact_pair3d_point_count(struct PonkoContactPair3D *pair,
                                                  size_t *out_count);

enum PonkoStatus ponko_contact_pair3d_point(struct PonkoContactPair3D *pair,
                                            size_t index,
                                            struct PonkoVec3 *out_point,
                                            float *out_distance);

enum PonkoStatus ponko_contact_pair3d_ignore(struct PonkoContactPair3D *pair);

enum PonkoStatus ponko_contact_pair3d_remove_point(struct PonkoContactPair3D *pair, size_t index);

enum PonkoStatus ponko_contact_pair3d_friction(struct PonkoContactPair3D *pair,
                                               float *out_friction);

enum PonkoStatus ponko_contact_pair3d_set_friction(struct PonkoContactPair3D *pair, float friction);

enum PonkoStatus ponko_contact_pair3d_restitution(struct PonkoContactPair3D *pair,
                                                  float *out_restitution);

enum PonkoStatus ponko_contact_pair3d_set_restitution(struct PonkoContactPair3D *pair,
                                                      float restitution);

enum PonkoStatus ponko_contact_pair3d_set_surface_velocity(struct PonkoContactPair3D *pair,
                                                           struct PonkoVec3 velocity);

enum PonkoStatus ponko_ctx_gamepad_rumble(struct PonkoCtx *ctx,
                                          uint32_t pad_slot,
                                          float strong,
                                          float weak,
                                          float left_trigger,
                                          float right_trigger,
                                          float seconds);

enum PonkoStatus ponko_ctx_gamepad_stop_rumble(struct PonkoCtx *ctx, uint32_t pad_slot);

enum PonkoStatus ponko_ctx_gamepad_rumble_levels(struct PonkoCtx *ctx,
                                                 uint32_t pad_slot,
                                                 struct PonkoMotorLevels *out_levels);

enum PonkoStatus ponko_ctx_gamepad_info(struct PonkoCtx *ctx,
                                        uint32_t pad_slot,
                                        struct PonkoGamepadInfo *out_info,
                                        bool *out_connected);

enum PonkoStatus ponko_ctx_gamepad_name(struct PonkoCtx *ctx,
                                        uint32_t pad_slot,
                                        char *buffer,
                                        size_t capacity,
                                        size_t *out_len);

enum PonkoStatus ponko_ctx_set_gamepad_family(struct PonkoCtx *ctx,
                                              uint32_t pad_slot,
                                              uint32_t family);

enum PonkoStatus ponko_ctx_gamepad_motion(struct PonkoCtx *ctx,
                                          uint32_t pad_slot,
                                          struct PonkoVec3 *out_gyro,
                                          struct PonkoVec3 *out_accel,
                                          bool *out_has);

enum PonkoStatus ponko_ctx_feed_gamepad_motion(struct PonkoCtx *ctx,
                                               uint32_t pad_slot,
                                               struct PonkoVec3 gyro,
                                               struct PonkoVec3 accel);

enum PonkoStatus ponko_ctx_last_device(struct PonkoCtx *ctx,
                                       uint32_t *out_kind,
                                       uint32_t *out_pad,
                                       bool *out_changed);

enum PonkoStatus ponko_ctx_gamepads_changed(struct PonkoCtx *ctx,
                                            bool disconnected,
                                            uint32_t *out_pads,
                                            size_t capacity,
                                            size_t *out_count);

enum PonkoStatus ponko_ctx_assign_player(struct PonkoCtx *ctx,
                                         uint32_t player,
                                         uint32_t kind,
                                         uint32_t pad_slot);

enum PonkoStatus ponko_ctx_unassign_player(struct PonkoCtx *ctx, uint32_t player);

enum PonkoStatus ponko_ctx_player_device(struct PonkoCtx *ctx,
                                         uint32_t player,
                                         uint32_t *out_kind,
                                         uint32_t *out_pad,
                                         bool *out_connected,
                                         bool *out_has);

enum PonkoStatus ponko_ctx_free_player(struct PonkoCtx *ctx, uint32_t *out_player, bool *out_has);

enum PonkoStatus ponko_ctx_join_pressed(struct PonkoCtx *ctx,
                                        uint32_t button,
                                        uint32_t *out_pad,
                                        bool *out_has);

enum PonkoStatus ponko_ctx_action_glyphs(struct PonkoCtx *ctx,
                                         const char *name,
                                         uint32_t kind,
                                         uint32_t pad_slot,
                                         struct PonkoGlyph *out_glyphs,
                                         size_t capacity,
                                         size_t *out_count);

enum PonkoStatus ponko_glyph_label(struct PonkoGlyph glyph,
                                   char *buffer,
                                   size_t capacity,
                                   size_t *out_len);

struct PonkoGlyphAtlas *ponko_glyph_atlas_new(uint32_t cell);

void ponko_glyph_atlas_free(struct PonkoGlyphAtlas *atlas);

enum PonkoStatus ponko_glyph_atlas_pixels(struct PonkoGlyphAtlas *atlas,
                                          const uint8_t **out_pixels,
                                          uint32_t *out_width,
                                          uint32_t *out_height);

enum PonkoStatus ponko_glyph_atlas_region(struct PonkoGlyphAtlas *atlas,
                                          struct PonkoGlyph glyph,
                                          struct PonkoRect *out_rect);

enum PonkoStatus ponko_glyph_atlas_texture(struct PonkoGlyphAtlas *atlas,
                                           struct PonkoCtx *ctx,
                                           struct PonkoTexture *out_texture);

enum PonkoStatus ponko_glyph_atlas_draw_prompt(struct PonkoGlyphAtlas *atlas,
                                               struct PonkoCtx *ctx,
                                               struct PonkoVec2 pos,
                                               float height,
                                               const struct PonkoGlyph *glyphs,
                                               size_t count,
                                               const char *label,
                                               struct PonkoColor color,
                                               float *out_width);

float ponko_ctx_dt(const struct PonkoCtx *ctx);

uint64_t ponko_ctx_sim_tick(const struct PonkoCtx *ctx);

double ponko_ctx_sim_time(const struct PonkoCtx *ctx);

double ponko_ctx_time(const struct PonkoCtx *ctx);

uint64_t ponko_ctx_frame(const struct PonkoCtx *ctx);

float ponko_ctx_fps(const struct PonkoCtx *ctx);

uint64_t ponko_ctx_random_bits(const struct PonkoCtx *ctx);

struct PonkoVec2 ponko_ctx_window_size(const struct PonkoCtx *ctx);

struct PonkoWorld *ponko_ctx_world(struct PonkoCtx *ctx);

enum PonkoStatus ponko_ctx_quit(struct PonkoCtx *ctx);

enum PonkoStatus ponko_ctx_camera_zoom_at(struct PonkoCtx *ctx,
                                          struct PonkoVec2 screen,
                                          float factor);

enum PonkoStatus ponko_ctx_quit_requested(struct PonkoCtx *ctx, bool *out_requested);

enum PonkoStatus ponko_ctx_pause(struct PonkoCtx *ctx);

enum PonkoStatus ponko_ctx_resume(struct PonkoCtx *ctx);

enum PonkoStatus ponko_ctx_step(struct PonkoCtx *ctx);

enum PonkoStatus ponko_ctx_set_sim_speed(struct PonkoCtx *ctx, float speed);

enum PonkoStatus ponko_ctx_sim_speed(struct PonkoCtx *ctx, float *out_speed);

enum PonkoStatus ponko_ctx_is_paused(struct PonkoCtx *ctx, bool *out_paused);

enum PonkoStatus ponko_ctx_key_down(struct PonkoCtx *ctx, uint32_t input, bool *out_state);

enum PonkoStatus ponko_ctx_key_pressed(struct PonkoCtx *ctx, uint32_t input, bool *out_state);

enum PonkoStatus ponko_ctx_key_released(struct PonkoCtx *ctx, uint32_t input, bool *out_state);

enum PonkoStatus ponko_ctx_mouse_down(struct PonkoCtx *ctx, uint32_t input, bool *out_state);

enum PonkoStatus ponko_ctx_mouse_pressed(struct PonkoCtx *ctx, uint32_t input, bool *out_state);

enum PonkoStatus ponko_ctx_mouse_released(struct PonkoCtx *ctx, uint32_t input, bool *out_state);

enum PonkoStatus ponko_ctx_gamepad_count(struct PonkoCtx *ctx, uint32_t *out_count);

enum PonkoStatus ponko_ctx_gamepad_at(struct PonkoCtx *ctx, uint32_t index, uint32_t *out_pad);

enum PonkoStatus ponko_ctx_gamepad_down(struct PonkoCtx *ctx,
                                        uint32_t pad,
                                        uint32_t button,
                                        bool *out_state);

enum PonkoStatus ponko_ctx_gamepad_pressed(struct PonkoCtx *ctx,
                                           uint32_t pad,
                                           uint32_t button,
                                           bool *out_state);

enum PonkoStatus ponko_ctx_gamepad_released(struct PonkoCtx *ctx,
                                            uint32_t pad,
                                            uint32_t button,
                                            bool *out_state);

enum PonkoStatus ponko_ctx_gamepad_axis(struct PonkoCtx *ctx,
                                        uint32_t pad,
                                        uint32_t axis,
                                        float *out_value);

enum PonkoStatus ponko_ctx_mouse_pos(struct PonkoCtx *ctx, struct PonkoVec2 *out_pos);

enum PonkoStatus ponko_ctx_mouse_world(struct PonkoCtx *ctx, struct PonkoVec2 *out_pos);

enum PonkoStatus ponko_ctx_mouse_wheel(struct PonkoCtx *ctx, struct PonkoVec2 *out_wheel);

enum PonkoStatus ponko_ctx_mouse_motion(struct PonkoCtx *ctx, struct PonkoVec2 *out_motion);

enum PonkoStatus ponko_ctx_set_cursor_locked(struct PonkoCtx *ctx, bool locked);

enum PonkoStatus ponko_ctx_cursor_locked(struct PonkoCtx *ctx, bool *out_locked);

enum PonkoStatus ponko_ctx_load_texture(struct PonkoCtx *ctx,
                                        const char *path,
                                        struct PonkoTexture *out_texture);

enum PonkoStatus ponko_ctx_load_json(struct PonkoCtx *ctx,
                                     const char *path,
                                     uint64_t entity,
                                     uint32_t component);

enum PonkoStatus ponko_ctx_load_resource_json(struct PonkoCtx *ctx,
                                              const char *path,
                                              const char *name);

enum PonkoStatus ponko_ctx_was_reloaded(struct PonkoCtx *ctx, const char *path, bool *out_reloaded);

enum PonkoStatus ponko_ctx_create_texture(struct PonkoCtx *ctx,
                                          uint32_t width,
                                          uint32_t height,
                                          const uint8_t *rgba,
                                          size_t len,
                                          struct PonkoTexture *out_texture);

enum PonkoStatus ponko_ctx_update_texture(struct PonkoCtx *ctx,
                                          struct PonkoTexture texture,
                                          const uint8_t *rgba,
                                          size_t len);

enum PonkoStatus ponko_ctx_clear(struct PonkoCtx *ctx, struct PonkoColor color);

enum PonkoStatus ponko_ctx_draw(struct PonkoCtx *ctx,
                                struct PonkoTexture texture,
                                struct PonkoVec2 pos);

enum PonkoStatus ponko_ctx_draw_ex(struct PonkoCtx *ctx,
                                   struct PonkoTexture texture,
                                   struct PonkoVec2 pos,
                                   const struct PonkoDrawParams *params);

enum PonkoStatus ponko_ctx_draw_text(struct PonkoCtx *ctx,
                                     const char *string,
                                     struct PonkoVec2 pos,
                                     float size,
                                     struct PonkoColor color);

enum PonkoStatus ponko_ctx_measure_text(struct PonkoCtx *ctx,
                                        const char *string,
                                        float size,
                                        struct PonkoVec2 *out_size);

enum PonkoStatus ponko_ctx_camera(struct PonkoCtx *ctx, struct PonkoCamera *out_camera);

enum PonkoStatus ponko_ctx_set_camera(struct PonkoCtx *ctx, const struct PonkoCamera *camera);

enum PonkoStatus ponko_ctx_despawn_later(struct PonkoCtx *ctx, uint64_t entity);

enum PonkoStatus ponko_ctx_spawn_later(struct PonkoCtx *ctx,
                                       const struct PonkoValue *values,
                                       size_t count);

enum PonkoStatus ponko_ctx_spawn_later_entity(struct PonkoCtx *ctx,
                                              const struct PonkoValue *values,
                                              size_t count,
                                              uint64_t *out_entity);

enum PonkoStatus ponko_ctx_set_later(struct PonkoCtx *ctx,
                                     uint64_t entity,
                                     const struct PonkoValue *value);

enum PonkoStatus ponko_ctx_remove_later(struct PonkoCtx *ctx, uint64_t entity, uint32_t component);

enum PonkoStatus ponko_ctx_send_event(struct PonkoCtx *ctx,
                                      uint32_t type_id,
                                      const void *data,
                                      size_t size);

enum PonkoStatus ponko_ctx_read_events(struct PonkoCtx *ctx,
                                       uint32_t type_id,
                                       size_t size,
                                       const void **out_data,
                                       size_t *out_count);

enum PonkoStatus ponko_ctx_spatial_radius(const struct PonkoCtx *ctx,
                                          struct PonkoVec2 center,
                                          float radius,
                                          struct PonkoGridHit *out_hits,
                                          size_t capacity,
                                          size_t *out_count);

enum PonkoStatus ponko_ctx_spatial_rect(const struct PonkoCtx *ctx,
                                        struct PonkoVec2 min,
                                        struct PonkoVec2 max,
                                        struct PonkoGridHit *out_hits,
                                        size_t capacity,
                                        size_t *out_count);

enum PonkoStatus ponko_ctx_spatial_nearest(const struct PonkoCtx *ctx,
                                           struct PonkoVec2 center,
                                           float max_radius,
                                           uint64_t exclude,
                                           struct PonkoGridHit *out_hit);

enum PonkoStatus ponko_ctx_spatial_view(const struct PonkoCtx *ctx, struct PonkoGridView *out_view);

enum PonkoStatus ponko_ctx_cast_ray_filtered(struct PonkoCtx *ctx,
                                             struct PonkoVec2 origin,
                                             struct PonkoVec2 direction,
                                             float max_distance,
                                             struct PonkoQueryFilter2D filter,
                                             bool *out_found,
                                             struct PonkoRayHit *out_hit);

enum PonkoStatus ponko_ctx_overlap_ball(struct PonkoCtx *ctx,
                                        struct PonkoVec2 center,
                                        float radius,
                                        struct PonkoQueryFilter2D filter,
                                        struct PonkoEntity *out_entities,
                                        size_t capacity,
                                        size_t *out_count);

enum PonkoStatus ponko_ctx_spawn_prefab3d(struct PonkoCtx *ctx,
                                          const char *path,
                                          struct PonkoVec3 at,
                                          uint64_t *output);

enum PonkoStatus ponko_ctx_spawn_prefab_transform3d(struct PonkoCtx *ctx,
                                                    const char *path,
                                                    struct PonkoTransform3D at,
                                                    uint64_t *output);

enum PonkoStatus ponko_ctx_spawn_prefab_transform2d(struct PonkoCtx *ctx,
                                                    const char *path,
                                                    struct PonkoTransform2D at,
                                                    uint64_t *output);

struct PonkoFilter *ponko_filter_with(uint32_t component);

struct PonkoFilter *ponko_filter_without(uint32_t component);

struct PonkoFilter *ponko_filter_always(void);

struct PonkoFilter *ponko_filter_never(void);

struct PonkoFilter *ponko_filter_all(const struct PonkoFilter *const *parts, size_t count);

struct PonkoFilter *ponko_filter_any(const struct PonkoFilter *const *parts, size_t count);

void ponko_filter_free(struct PonkoFilter *filter);

enum PonkoStatus ponko_filter_check(struct PonkoWorld *world, const struct PonkoFilter *filter);

enum PonkoStatus ponko_query_filtered(struct PonkoWorld *world,
                                      const uint32_t *components,
                                      size_t count,
                                      const struct PonkoFilter *filter,
                                      const struct PonkoQueryOptions *options,
                                      PonkoQueryFn callback,
                                      void *user);

struct PonkoNavGrid *ponko_nav_new(uint32_t width, uint32_t height, float cell_size);

void ponko_nav_free(struct PonkoNavGrid *nav);

enum PonkoStatus ponko_nav_set_origin(struct PonkoNavGrid *nav, struct PonkoVec2 origin);

enum PonkoStatus ponko_nav_costs(struct PonkoNavGrid *nav, uint8_t **out_costs, size_t *out_len);

struct PonkoFlowField *ponko_flow_new(void);

void ponko_flow_free(struct PonkoFlowField *flow);

enum PonkoStatus ponko_flow_compute(struct PonkoFlowField *flow,
                                    const struct PonkoNavGrid *nav,
                                    const struct PonkoVec2 *goals,
                                    size_t count);

struct PonkoVec2 ponko_flow_direction(const struct PonkoFlowField *flow, struct PonkoVec2 pos);

enum PonkoStatus ponko_flow_distance(const struct PonkoFlowField *flow,
                                     struct PonkoVec2 pos,
                                     float *out_distance);

enum PonkoStatus ponko_flow_view(const struct PonkoFlowField *flow, struct PonkoFlowView *out_view);

struct PonkoGame *ponko_game_new(const char *title, uint32_t width, uint32_t height);

void ponko_game_free(struct PonkoGame *game);

enum PonkoStatus ponko_game_fixed_hz(struct PonkoGame *game, uint32_t hz);

enum PonkoStatus ponko_game_seed(struct PonkoGame *game, uint64_t seed);

enum PonkoStatus ponko_game_vsync(struct PonkoGame *game, bool on);

enum PonkoStatus ponko_game_msaa(struct PonkoGame *game, uint32_t samples);

enum PonkoStatus ponko_game_fullscreen(struct PonkoGame *game, bool on);

enum PonkoStatus ponko_game_spatial_grid(struct PonkoGame *game, float cell_size);

enum PonkoStatus ponko_game_camera_controls(struct PonkoGame *game);

enum PonkoStatus ponko_game_control_endpoint(struct PonkoGame *game, bool on);

enum PonkoStatus ponko_game_hot_reload(struct PonkoGame *game, bool on);

enum PonkoStatus ponko_game_inspector(struct PonkoGame *game, bool on);

enum PonkoStatus ponko_game_sim_system(struct PonkoGame *game,
                                       const char *name,
                                       uint32_t every,
                                       PonkoSystemFn system_fn,
                                       void *user);

enum PonkoStatus ponko_game_add_system(struct PonkoGame *game,
                                       uint32_t phase,
                                       const char *name,
                                       uint32_t every,
                                       PonkoSystemFn system_fn,
                                       void *user);

enum PonkoStatus ponko_game_add_system_id(struct PonkoGame *game,
                                          uint32_t phase,
                                          const char *name,
                                          uint32_t every,
                                          PonkoSystemFn system_fn,
                                          void *user,
                                          uint32_t *out_id);

enum PonkoStatus ponko_game_remove(struct PonkoGame *game, uint32_t system_id);

enum PonkoStatus ponko_game_setup(struct PonkoGame *game, PonkoSystemFn setup, void *user);

enum PonkoStatus ponko_game_plugin(struct PonkoGame *game,
                                   PonkoPluginBuildFn build_fn,
                                   PonkoSystemFn setup,
                                   void *user);

enum PonkoStatus ponko_game_late_system(struct PonkoGame *game,
                                        const char *name,
                                        PonkoSystemFn system_fn,
                                        void *user);

enum PonkoStatus ponko_game_system(struct PonkoGame *game,
                                   const char *name,
                                   PonkoSystemFn system_fn,
                                   void *user);

enum PonkoStatus ponko_game_remove_system(struct PonkoGame *game, const char *name_ptr);

enum PonkoStatus ponko_game_profile_headless(struct PonkoGame *game,
                                             uint64_t warmup,
                                             uint64_t ticks,
                                             PonkoSystemFn setup,
                                             void *user,
                                             struct PonkoWorld **out_world);

enum PonkoStatus ponko_game_run(struct PonkoGame *game, PonkoSystemFn setup, void *user);

enum PonkoStatus ponko_game_run_headless(struct PonkoGame *game,
                                         uint64_t ticks,
                                         PonkoSystemFn setup,
                                         void *user,
                                         struct PonkoWorld **out_world);

enum PonkoStatus ponko_ctx_spawn_gltf(struct PonkoCtx *ctx,
                                      const char *path,
                                      struct PonkoTransform3D at,
                                      bool without_saved_objects,
                                      uint64_t *out_root);

enum PonkoStatus ponko_ctx_despawn_gltf(struct PonkoCtx *ctx, uint64_t root);

enum PonkoStatus ponko_gltf_node_entity(struct PonkoWorld *world,
                                        uint64_t root,
                                        const char *path,
                                        uint64_t *out_entity);

enum PonkoStatus ponko_gltf_node_extras(struct PonkoWorld *world,
                                        uint64_t entity,
                                        char *buffer,
                                        size_t capacity,
                                        size_t *out_len);

enum PonkoStatus ponko_spawn_point_name(struct PonkoWorld *world,
                                        uint64_t entity,
                                        char *buffer,
                                        size_t capacity,
                                        size_t *out_len);

enum PonkoStatus ponko_gltf_scene_load(struct PonkoCtx *ctx,
                                       const char *path,
                                       char *buffer,
                                       size_t capacity,
                                       size_t *out_len);

enum PonkoStatus ponko_gltf_properties_schema(struct PonkoWorld *world,
                                              char *buffer,
                                              size_t capacity,
                                              size_t *out_len);

struct PonkoGrid *ponko_grid_new(float cell_size);

void ponko_grid_free(struct PonkoGrid *grid);

enum PonkoStatus ponko_grid_rebuild(struct PonkoGrid *grid,
                                    const struct PonkoGridItem *items,
                                    size_t count);

enum PonkoStatus ponko_grid_radius(const struct PonkoGrid *grid,
                                   struct PonkoVec2 center,
                                   float radius,
                                   struct PonkoGridItem *out_items,
                                   size_t capacity,
                                   size_t *out_count);

size_t ponko_grid_len(const struct PonkoGrid *grid);

enum PonkoStatus ponko_grid_view(const struct PonkoGrid *grid, struct PonkoGridView *out_view);

enum PonkoStatus ponko_ctx_handle_is_live(struct PonkoCtx *ctx,
                                          uint32_t kind,
                                          uint32_t id,
                                          bool *out_live);

enum PonkoStatus ponko_ctx_handle_free(struct PonkoCtx *ctx, uint32_t kind, uint32_t id);

enum PonkoStatus ponko_ctx_live_counts(struct PonkoCtx *ctx, struct PonkoLiveCounts *out_counts);

enum PonkoStatus ponko_ctx_asset_count(struct PonkoCtx *ctx,
                                       uint32_t asset_class_id,
                                       struct PonkoAssetCount *out_count);

enum PonkoStatus ponko_ctx_asset_handles(struct PonkoCtx *ctx,
                                         uint32_t asset_class_id,
                                         struct PonkoAssetRecord *records,
                                         size_t capacity,
                                         size_t *out_len);

enum PonkoStatus ponko_ctx_asset_owner(struct PonkoCtx *ctx,
                                       uint32_t asset_class_id,
                                       uint32_t id,
                                       char *buffer,
                                       size_t capacity,
                                       size_t *out_len);

enum PonkoStatus ponko_ctx_unload_mesh(struct PonkoCtx *ctx, const char *path, bool *out_unloaded);

enum PonkoStatus ponko_ctx_mesh_costs(struct PonkoCtx *ctx,
                                      struct PonkoMeshCost *records,
                                      size_t capacity,
                                      char *names,
                                      size_t name_capacity,
                                      size_t *out_len,
                                      size_t *out_names_len);

enum PonkoStatus ponko_ctx_streamed_assets(struct PonkoCtx *ctx,
                                           struct PonkoStreamedAsset *records,
                                           size_t capacity,
                                           char *names,
                                           size_t name_capacity,
                                           size_t *out_len,
                                           size_t *out_names_len);

enum PonkoStatus ponko_game_headless_frames(struct PonkoGame *game, bool on);

enum PonkoStatus ponko_input_scenario_click(struct PonkoInputScenario *scenario,
                                            uint64_t tick,
                                            const char *node);

enum PonkoStatus ponko_input_scenario_type_text(struct PonkoInputScenario *scenario,
                                                uint64_t tick,
                                                const char *node,
                                                const char *typed);

enum PonkoStatus ponko_input_scenario_expect_ui(struct PonkoInputScenario *scenario,
                                                uint64_t tick,
                                                const char *node,
                                                int32_t exists,
                                                int32_t shown,
                                                int32_t focused,
                                                const char *expected_text);

enum PonkoStatus ponko_ctx_ui_is_shown(struct PonkoCtx *ctx,
                                       uint64_t target,
                                       uint64_t id,
                                       bool *out_shown);

enum PonkoStatus ponko_set_parent(struct PonkoWorld *world, uint64_t child, uint64_t parent);

enum PonkoStatus ponko_remove_parent(struct PonkoWorld *world,
                                     uint64_t child,
                                     bool *out_had_parent);

enum PonkoStatus ponko_parent(struct PonkoWorld *world, uint64_t e, uint64_t *out_parent);

enum PonkoStatus ponko_children(struct PonkoWorld *world,
                                uint64_t e,
                                uint64_t *out_entities,
                                size_t capacity,
                                size_t *out_count);

enum PonkoStatus ponko_descendants(struct PonkoWorld *world,
                                   uint64_t e,
                                   uint64_t *out_entities,
                                   size_t capacity,
                                   size_t *out_count);

enum PonkoStatus ponko_ancestors(struct PonkoWorld *world,
                                 uint64_t e,
                                 uint64_t *out_entities,
                                 size_t capacity,
                                 size_t *out_count);

enum PonkoStatus ponko_set_child_index(struct PonkoWorld *world, uint64_t child, size_t index);

enum PonkoStatus ponko_despawn_recursive(struct PonkoWorld *world, uint64_t e, size_t *out_count);

enum PonkoStatus ponko_ctx_despawn_recursive_later(struct PonkoCtx *ctx, uint64_t entity);

enum PonkoStatus ponko_update_hierarchy(struct PonkoWorld *world);

enum PonkoStatus ponko_ctx_spawn_level(struct PonkoCtx *ctx,
                                       const char *path,
                                       struct PonkoVec2 at,
                                       const struct PonkoLevelOptions *options,
                                       uint64_t *out_root);

enum PonkoStatus ponko_ctx_spawn_ldtk_level(struct PonkoCtx *ctx,
                                            const char *path,
                                            struct PonkoVec2 at,
                                            const struct PonkoLdtkOptions *ldtk,
                                            uint64_t *out_root);

enum PonkoStatus ponko_ctx_despawn_level(struct PonkoCtx *ctx, uint64_t root);

enum PonkoStatus ponko_level_entities(struct PonkoWorld *world,
                                      uint64_t root,
                                      uint64_t *out_entities,
                                      size_t capacity,
                                      size_t *out_len);

enum PonkoStatus ponko_level_object_get(struct PonkoWorld *world,
                                        uint64_t entity,
                                        struct PonkoLevelObject *out_object);

enum PonkoStatus ponko_level_object_name(struct PonkoWorld *world,
                                         uint64_t entity,
                                         char *buffer,
                                         size_t capacity,
                                         size_t *out_len);

enum PonkoStatus ponko_level_object_class(struct PonkoWorld *world,
                                          uint64_t entity,
                                          char *buffer,
                                          size_t capacity,
                                          size_t *out_len);

enum PonkoStatus ponko_level_object_layer(struct PonkoWorld *world,
                                          uint64_t entity,
                                          char *buffer,
                                          size_t capacity,
                                          size_t *out_len);

enum PonkoStatus ponko_level_property_json(struct PonkoWorld *world,
                                           uint64_t entity,
                                           const char *name,
                                           char *buffer,
                                           size_t capacity,
                                           size_t *out_len);

enum PonkoStatus ponko_tilemap_set_parallax(struct PonkoWorld *world,
                                            uint64_t entity,
                                            struct PonkoVec2 parallax,
                                            struct PonkoVec2 origin);

enum PonkoStatus ponko_tilemap_animate(struct PonkoWorld *world,
                                       uint64_t entity,
                                       uint16_t tile,
                                       const uint16_t *frames,
                                       const float *seconds,
                                       size_t count);

enum PonkoStatus ponko_tilemap_set_flipped(struct PonkoWorld *world,
                                           uint64_t entity,
                                           uint32_t x,
                                           uint32_t y,
                                           uint16_t frame,
                                           uint8_t flip);

enum PonkoStatus ponko_tilemap_flip(struct PonkoWorld *world,
                                    uint64_t entity,
                                    uint32_t x,
                                    uint32_t y,
                                    uint8_t *out_flip);

enum PonkoStatus ponko_tilemap_autotile(struct PonkoWorld *world,
                                        uint64_t entity,
                                        uint32_t neighbours,
                                        const uint16_t *frames,
                                        size_t frames_len,
                                        bool outside_filled,
                                        const bool *filled);

enum PonkoStatus ponko_tilemap_paint_autotile(struct PonkoWorld *world,
                                              uint64_t entity,
                                              uint32_t neighbours,
                                              const uint16_t *frames,
                                              size_t frames_len,
                                              bool outside_filled,
                                              uint32_t x,
                                              uint32_t y,
                                              bool on);

enum PonkoStatus ponko_tilemap_solid_rects(struct PonkoWorld *world,
                                           uint64_t entity,
                                           const uint16_t *solid_tiles,
                                           size_t count,
                                           struct PonkoRect *out_rects,
                                           size_t capacity,
                                           size_t *out_len);

enum PonkoStatus ponko_tilemap_spawn_colliders(struct PonkoWorld *world,
                                               uint64_t entity,
                                               const uint16_t *solid_tiles,
                                               size_t count,
                                               size_t *out_count);

struct PonkoPointLight2D ponko_point_light2d_default(void);

struct PonkoSpotLight2D ponko_spot_light2d_default(void);

struct PonkoDirectionalLight2D ponko_directional_light2d_default(void);

enum PonkoStatus ponko_point_light2d_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_spot_light2d_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_directional_light2d_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_ctx_set_occluder2d_rect(struct PonkoCtx *ctx,
                                               uint64_t entity_bits,
                                               struct PonkoVec2 size);

enum PonkoStatus ponko_ctx_set_occluder2d_circle(struct PonkoCtx *ctx,
                                                 uint64_t entity_bits,
                                                 float radius);

enum PonkoStatus ponko_ctx_set_occluder2d_polygon(struct PonkoCtx *ctx,
                                                  uint64_t entity_bits,
                                                  const struct PonkoVec2 *points,
                                                  size_t count);

enum PonkoStatus ponko_ctx_set_occluder2d_line(struct PonkoCtx *ctx,
                                               uint64_t entity_bits,
                                               struct PonkoVec2 a,
                                               struct PonkoVec2 b);

enum PonkoStatus ponko_ctx_set_tilemap_occluder(struct PonkoCtx *ctx,
                                                uint64_t entity_bits,
                                                const uint16_t *solid_tiles,
                                                size_t count);

enum PonkoStatus ponko_ctx_remove_occluder2d(struct PonkoCtx *ctx, uint64_t entity_bits);

enum PonkoStatus ponko_ctx_occluder2d_edge_count(struct PonkoCtx *ctx,
                                                 uint64_t entity_bits,
                                                 size_t *out_count);

enum PonkoStatus ponko_ctx_set_ambient_light2d(struct PonkoCtx *ctx,
                                               bool on,
                                               struct PonkoColor color);

enum PonkoStatus ponko_ctx_ambient_light2d(struct PonkoCtx *ctx,
                                           bool *out_on,
                                           struct PonkoColor *out_color);

enum PonkoStatus ponko_ctx_set_normal_map(struct PonkoCtx *ctx,
                                          struct PonkoTexture texture,
                                          bool has_normal_map,
                                          struct PonkoTexture normal_map);

enum PonkoStatus ponko_ctx_normal_map(struct PonkoCtx *ctx,
                                      struct PonkoTexture texture,
                                      bool *out_has,
                                      struct PonkoTexture *out_normal_map);

enum PonkoStatus ponko_ctx_try_load_texture(struct PonkoCtx *ctx,
                                            const char *path,
                                            struct PonkoTexture *out_texture);

enum PonkoStatus ponko_ctx_try_load_font(struct PonkoCtx *ctx,
                                         const char *path,
                                         struct PonkoFont *out_font);

enum PonkoStatus ponko_ctx_try_load_sound(struct PonkoCtx *ctx,
                                          const char *path,
                                          struct PonkoSound *out_sound);

enum PonkoStatus ponko_ctx_try_load_stream(struct PonkoCtx *ctx,
                                           const char *path,
                                           struct PonkoSound *out_sound);

enum PonkoStatus ponko_ctx_try_load_mesh(struct PonkoCtx *ctx,
                                         const char *path,
                                         struct PonkoMesh *out_mesh);

enum PonkoStatus ponko_lobby_join(const char *addr, struct PonkoLobby **out_lobby);

void ponko_lobby_free(struct PonkoLobby *lobby);

enum PonkoStatus ponko_lobby_poll(struct PonkoLobby *lobby);

enum PonkoStatus ponko_lobby_next_event(struct PonkoLobby *lobby,
                                        struct PonkoLobbyEvent *out_event,
                                        bool *out_found);

enum PonkoStatus ponko_lobby_create_room(struct PonkoLobby *lobby, uint16_t size);

enum PonkoStatus ponko_lobby_join_room(struct PonkoLobby *lobby, const char *code);

enum PonkoStatus ponko_lobby_leave_room(struct PonkoLobby *lobby);

enum PonkoStatus ponko_lobby_queue(struct PonkoLobby *lobby,
                                   const char *mode,
                                   uint16_t size,
                                   int32_t rating);

enum PonkoStatus ponko_lobby_cancel_queue(struct PonkoLobby *lobby);

enum PonkoStatus ponko_lobby_set_meta(struct PonkoLobby *lobby, const char *meta);

enum PonkoStatus ponko_lobby_relay(struct PonkoLobby *lobby,
                                   uint32_t to,
                                   enum PonkoLane lane,
                                   const uint8_t *bytes,
                                   size_t len);

enum PonkoStatus ponko_lobby_disconnect(struct PonkoLobby *lobby);

enum PonkoStatus ponko_lobby_state(struct PonkoLobby *lobby, struct PonkoLobbyState *out_state);

enum PonkoStatus ponko_lobby_members(struct PonkoLobby *lobby,
                                     uint32_t *members,
                                     size_t capacity,
                                     size_t *out_count);

enum PonkoStatus ponko_lobby_room_meta(struct PonkoLobby *lobby,
                                       char *buffer,
                                       size_t capacity,
                                       size_t *out_len);

struct PonkoTextParams ponko_text_params_default(void);

struct PonkoUiStyle ponko_ui_style_default(void);

struct PonkoSoundParams ponko_sound_params_default(void);

enum PonkoStatus ponko_ctx_draw_rect(struct PonkoCtx *ctx,
                                     struct PonkoRect rect,
                                     struct PonkoColor color);

enum PonkoStatus ponko_ctx_draw_line(struct PonkoCtx *ctx,
                                     struct PonkoVec2 a,
                                     struct PonkoVec2 b,
                                     float width,
                                     struct PonkoColor color);

enum PonkoStatus ponko_ctx_draw_circle(struct PonkoCtx *ctx,
                                       struct PonkoVec2 center,
                                       float radius,
                                       struct PonkoColor color);

enum PonkoStatus ponko_ctx_ui_style(struct PonkoCtx *ctx, struct PonkoUiStyle *out_style);

enum PonkoStatus ponko_ctx_set_ui_style(struct PonkoCtx *ctx, const struct PonkoUiStyle *style);

enum PonkoStatus ponko_ctx_mouse_over_ui(struct PonkoCtx *ctx, bool *out_over);

enum PonkoStatus ponko_ctx_ui_panel(struct PonkoCtx *ctx, struct PonkoRect rect);

enum PonkoStatus ponko_ctx_ui_label(struct PonkoCtx *ctx,
                                    struct PonkoVec2 pos,
                                    const char *string,
                                    struct PonkoVec2 *out_size);

enum PonkoStatus ponko_ctx_ui_button(struct PonkoCtx *ctx,
                                     struct PonkoRect rect,
                                     const char *label,
                                     bool *out_clicked);

enum PonkoStatus ponko_ctx_load_font(struct PonkoCtx *ctx,
                                     const char *path,
                                     struct PonkoFont *out_font);

enum PonkoStatus ponko_ctx_draw_text_ex(struct PonkoCtx *ctx,
                                        const char *string,
                                        struct PonkoVec2 pos,
                                        const struct PonkoTextParams *params);

enum PonkoStatus ponko_ctx_measure_text_ex(struct PonkoCtx *ctx,
                                           const char *string,
                                           struct PonkoFont font,
                                           float size,
                                           struct PonkoVec2 *out_size);

enum PonkoStatus ponko_ctx_load_sound(struct PonkoCtx *ctx,
                                      const char *path,
                                      struct PonkoSound *out_sound);

enum PonkoStatus ponko_ctx_create_sound(struct PonkoCtx *ctx,
                                        uint32_t sample_rate,
                                        const float *samples,
                                        size_t len,
                                        struct PonkoSound *out_sound);

enum PonkoStatus ponko_ctx_play_sound(struct PonkoCtx *ctx,
                                      struct PonkoSound sound,
                                      const struct PonkoSoundParams *params,
                                      struct PonkoPlaying *out_playing);

enum PonkoStatus ponko_ctx_stop_sound(struct PonkoCtx *ctx, struct PonkoPlaying playing);

enum PonkoStatus ponko_ctx_save_screenshot(struct PonkoCtx *ctx, const char *path);

enum PonkoStatus ponko_ctx_reloaded_textures(const struct PonkoCtx *ctx,
                                             const struct PonkoTexture **out_textures,
                                             size_t *out_count);

enum PonkoStatus ponko_ctx_turns(const struct PonkoCtx *ctx,
                                 uint32_t every,
                                 struct PonkoTurns *out_turns);

enum PonkoStatus ponko_net_messages_host(const char *addr,
                                         size_t max_clients,
                                         struct PonkoNetMessages **out_endpoint);

enum PonkoStatus ponko_net_messages_join(const char *addr, struct PonkoNetMessages **out_endpoint);

enum PonkoStatus ponko_net_messages_loopback(size_t players,
                                             struct PonkoNetMessages **out_endpoints);

void ponko_net_messages_free(struct PonkoNetMessages *endpoint);

enum PonkoStatus ponko_net_messages_set_timeout(struct PonkoNetMessages *endpoint, uint64_t ms);

enum PonkoStatus ponko_net_messages_local_port(struct PonkoNetMessages *endpoint,
                                               uint16_t *out_port);

enum PonkoStatus ponko_net_messages_send(struct PonkoNetMessages *endpoint,
                                         uint64_t peer,
                                         enum PonkoLane lane_,
                                         const uint8_t *bytes,
                                         size_t len);

enum PonkoStatus ponko_net_messages_send_to_all(struct PonkoNetMessages *endpoint,
                                                enum PonkoLane lane_,
                                                const uint8_t *bytes,
                                                size_t len);

enum PonkoStatus ponko_net_messages_poll(struct PonkoNetMessages *endpoint);

enum PonkoStatus ponko_net_messages_next_event(struct PonkoNetMessages *endpoint,
                                               struct PonkoMessageEvent *out_event,
                                               bool *out_found);

enum PonkoStatus ponko_net_messages_disconnect(struct PonkoNetMessages *endpoint, uint64_t peer);

enum PonkoStatus ponko_net_messages_stats(struct PonkoNetMessages *endpoint,
                                          struct PonkoMessageStats *out_stats);

enum PonkoStatus ponko_net_messages_steam_host(uint32_t app_id,
                                               size_t max_clients,
                                               struct PonkoNetMessages **out_endpoint);

enum PonkoStatus ponko_net_messages_steam_join(uint32_t app_id,
                                               uint64_t lobby,
                                               struct PonkoNetMessages **out_endpoint);

enum PonkoStatus ponko_net_messages_steam_peer(struct PonkoNetMessages *endpoint,
                                               uint64_t peer,
                                               uint64_t *output);

enum PonkoStatus ponko_mesh_data_add_morph_target(struct PonkoMeshData *data,
                                                  const char *name,
                                                  const struct PonkoVec3 *positions,
                                                  const struct PonkoVec3 *normals,
                                                  size_t count);

enum PonkoStatus ponko_mesh_data_clear_morph_targets(struct PonkoMeshData *data);

size_t ponko_mesh_data_morph_target_count(const struct PonkoMeshData *data);

enum PonkoStatus ponko_ctx_morph_target(struct PonkoCtx *ctx,
                                        struct PonkoMesh mesh,
                                        const char *name,
                                        size_t *out_target);

enum PonkoStatus ponko_ctx_set_morph_weight(struct PonkoCtx *ctx,
                                            uint64_t entity_bits,
                                            size_t target,
                                            float weight);

enum PonkoStatus ponko_ctx_morph_weight(struct PonkoCtx *ctx,
                                        uint64_t entity_bits,
                                        size_t target,
                                        float *out_weight);

enum PonkoStatus ponko_ctx_clear_morph_weights(struct PonkoCtx *ctx, uint64_t entity_bits);

enum PonkoStatus ponko_mesh_data_add_morph_frame(struct PonkoMeshData *data,
                                                 size_t channel,
                                                 size_t target,
                                                 float weight);

enum PonkoStatus ponko_clip_new(const char *name, struct PonkoClip **output);

enum PonkoStatus ponko_clip_set_morph_track(struct PonkoClip *clip,
                                            const char *name,
                                            const float *times,
                                            const float *values,
                                            size_t count,
                                            uint32_t interpolation);

enum PonkoStatus ponko_ctx_write_animated_morphs(struct PonkoCtx *ctx);

struct PonkoNavBakeSettings ponko_nav_bake_settings_default(void);

struct PonkoNavSources *ponko_nav_sources_new(void);

void ponko_nav_sources_free(struct PonkoNavSources *sources);

enum PonkoStatus ponko_nav_sources_add_triangles(struct PonkoNavSources *sources_,
                                                 const struct PonkoVec3 *corners,
                                                 size_t count);

enum PonkoStatus ponko_nav_sources_add_mesh(struct PonkoNavSources *sources_,
                                            const struct PonkoMeshData *data,
                                            struct PonkoTransform3D transform);

enum PonkoStatus ponko_ctx_bake_navmesh(struct PonkoCtx *ctx,
                                        const struct PonkoNavSources *sources_,
                                        const struct PonkoNavBakeSettings *settings);

enum PonkoStatus ponko_ctx_navmesh_sample_position(struct PonkoCtx *ctx,
                                                   struct PonkoVec3 point,
                                                   float max_distance,
                                                   struct PonkoVec3 *out_point,
                                                   bool *out_found);

enum PonkoStatus ponko_ctx_navmesh_find_path(struct PonkoCtx *ctx,
                                             struct PonkoVec3 from,
                                             struct PonkoVec3 to,
                                             struct PonkoVec3 *out_corners,
                                             size_t capacity,
                                             size_t *out_len);

enum PonkoStatus ponko_ctx_set_nav_agent(struct PonkoCtx *ctx,
                                         uint64_t entity_bits,
                                         float speed,
                                         float radius,
                                         float acceleration);

enum PonkoStatus ponko_ctx_set_nav_destination(struct PonkoCtx *ctx,
                                               uint64_t entity_bits,
                                               const struct PonkoVec3 *destination);

enum PonkoStatus ponko_ctx_set_nav_obstacle(struct PonkoCtx *ctx,
                                            uint64_t entity_bits,
                                            struct PonkoVec3 center,
                                            struct PonkoVec3 half_extents);

enum PonkoStatus ponko_ctx_set_nav_carve_obstacle(struct PonkoCtx *ctx,
                                                  uint64_t entity_bits,
                                                  struct PonkoVec3 center,
                                                  struct PonkoVec3 half_extents);

enum PonkoStatus ponko_ctx_navmesh_update_carving(struct PonkoCtx *ctx,
                                                  size_t budget,
                                                  size_t *steps,
                                                  bool *pending);

enum PonkoStatus ponko_ctx_remove_nav(struct PonkoCtx *ctx, uint64_t entity_bits);

enum PonkoStatus ponko_game_navmesh3d(struct PonkoGame *game);

enum PonkoStatus ponko_ctx_set_nav_avoidance(struct PonkoCtx *ctx,
                                             uint64_t entity_bits,
                                             bool enabled,
                                             float weight,
                                             float neighbour_distance);

enum PonkoStatus ponko_game_lockstep_host(struct PonkoGame *game,
                                          const char *addr,
                                          uint32_t players,
                                          uint32_t input_delay,
                                          uint32_t checksum_every,
                                          uint16_t *out_port);

enum PonkoStatus ponko_game_lockstep_join(struct PonkoGame *game, const char *addr);

enum PonkoStatus ponko_ctx_net_player(struct PonkoCtx *ctx,
                                      uint32_t *out_player,
                                      uint32_t *out_players);

enum PonkoStatus ponko_ctx_net_state(struct PonkoCtx *ctx, uint32_t *out_state);

enum PonkoStatus ponko_ctx_set_input(struct PonkoCtx *ctx, const void *data, size_t size);

enum PonkoStatus ponko_ctx_input(struct PonkoCtx *ctx,
                                 uint32_t player,
                                 const void **out_data,
                                 size_t *out_size);

enum PonkoStatus ponko_ctx_net_desync(struct PonkoCtx *ctx,
                                      bool *out_found,
                                      uint64_t *out_tick,
                                      uint32_t *out_player);

struct PonkoParticleSettings3D ponko_particle_settings3d_default(void);

enum PonkoStatus ponko_ctx_set_particle_emitter3d(struct PonkoCtx *ctx,
                                                  uint64_t entity_bits,
                                                  const struct PonkoParticleSettings3D *settings);

enum PonkoStatus ponko_ctx_particle_burst(struct PonkoCtx *ctx,
                                          uint64_t entity_bits,
                                          uint32_t count);

enum PonkoStatus ponko_ctx_particle_live_count(struct PonkoCtx *ctx,
                                               uint64_t entity_bits,
                                               uint32_t *out_count);

enum PonkoStatus ponko_ctx_remove_particle_emitter3d(struct PonkoCtx *ctx, uint64_t entity_bits);

struct PonkoParticleSettings2D ponko_particle_settings2d_default(void);

enum PonkoStatus ponko_ctx_set_particle_emitter2d(struct PonkoCtx *ctx,
                                                  uint64_t entity_bits,
                                                  const struct PonkoParticleSettings2D *settings);

enum PonkoStatus ponko_ctx_particle_burst2d(struct PonkoCtx *ctx,
                                            uint64_t entity_bits,
                                            uint32_t count);

enum PonkoStatus ponko_ctx_particle_live_count2d(struct PonkoCtx *ctx,
                                                 uint64_t entity_bits,
                                                 uint32_t *out_count);

enum PonkoStatus ponko_ctx_particle_update2d(struct PonkoCtx *ctx, uint64_t entity_bits, float dt);

enum PonkoStatus ponko_ctx_remove_particle_emitter2d(struct PonkoCtx *ctx, uint64_t entity_bits);

enum PonkoStatus ponko_ctx_frame_stats(const struct PonkoCtx *ctx,
                                       struct PonkoFrameStats *out_stats);

enum PonkoStatus ponko_ctx_gpu_uploads(const struct PonkoCtx *ctx,
                                       struct PonkoGpuUploads *out_uploads);

enum PonkoStatus ponko_ctx_set_perf_hud(struct PonkoCtx *ctx, bool on);

enum PonkoStatus ponko_ctx_record_trace(struct PonkoCtx *ctx, const char *path, uint32_t frames);

enum PonkoStatus ponko_last_perf_report(char *buffer, size_t capacity, size_t *out_len);

const struct PonkoProfileScope *ponko_profile_scope(const char *name);

uint64_t ponko_profile_begin(const struct PonkoProfileScope *scope);

void ponko_profile_end(const struct PonkoProfileScope *scope, uint64_t token);

void ponko_note_allocation(size_t bytes);

enum PonkoStatus ponko_ctx_set_profiling(struct PonkoCtx *ctx, bool on);

enum PonkoStatus ponko_ctx_perf_report(const struct PonkoCtx *ctx,
                                       char *buffer,
                                       size_t capacity,
                                       size_t *out_len);

enum PonkoStatus ponko_perf_report_text(const char *report_json,
                                        char *buffer,
                                        size_t capacity,
                                        size_t *out_len);

enum PonkoStatus ponko_perf_check(const char *report_json,
                                  const char *budget_json,
                                  char *buffer,
                                  size_t capacity,
                                  size_t *out_len);

enum PonkoStatus ponko_perf_compare(const char *report_json,
                                    const char *baseline_json,
                                    float tolerance,
                                    char *buffer,
                                    size_t capacity,
                                    size_t *out_len);

struct PonkoRigidBody ponko_rigid_body(uint32_t kind);

struct PonkoCollider ponko_collider_circle(float radius);

struct PonkoCollider ponko_collider_rect(struct PonkoVec2 size);

struct PonkoCollider ponko_collider_capsule(float height, float radius);

enum PonkoStatus ponko_set_polygon_collider(struct PonkoWorld *world,
                                            uint64_t entity,
                                            const struct PonkoVec2 *points,
                                            uint32_t count,
                                            struct PonkoCollider settings);

enum PonkoStatus ponko_collider_points(struct PonkoWorld *world,
                                       uint64_t entity,
                                       struct PonkoVec2 *out_points,
                                       uint32_t *out_count);

enum PonkoStatus ponko_rigid_body_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_collider_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_velocity_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_game_physics(struct PonkoGame *game, struct PonkoVec2 gravity);

enum PonkoStatus ponko_ctx_set_gravity(struct PonkoCtx *ctx, struct PonkoVec2 gravity);

enum PonkoStatus ponko_ctx_cast_ray(struct PonkoCtx *ctx,
                                    struct PonkoVec2 origin,
                                    struct PonkoVec2 direction,
                                    float max_distance,
                                    bool *out_found,
                                    struct PonkoRayHit *out_hit);

enum PonkoStatus ponko_ctx_physics_at(struct PonkoCtx *ctx,
                                      struct PonkoVec2 point,
                                      struct PonkoEntity *out_entities,
                                      size_t capacity,
                                      size_t *out_count);

enum PonkoStatus ponko_ctx_physics_in_circle(struct PonkoCtx *ctx,
                                             struct PonkoVec2 center,
                                             float radius,
                                             struct PonkoEntity *out_entities,
                                             size_t capacity,
                                             size_t *out_count);

enum PonkoStatus ponko_ctx_apply_impulse(struct PonkoCtx *ctx,
                                         uint64_t entity,
                                         struct PonkoVec2 impulse);

enum PonkoStatus ponko_ctx_body_mass(struct PonkoCtx *ctx, uint64_t entity, float *out_mass);

enum PonkoStatus ponko_ctx_collisions(struct PonkoCtx *ctx,
                                      bool started,
                                      const struct PonkoCollision **out_data,
                                      size_t *out_count);

struct PonkoJoint3D ponko_joint3d_generic(uint64_t body_a, uint64_t body_b);

struct PonkoJoint3D ponko_joint3d_fixed(uint64_t body_a, uint64_t body_b);

struct PonkoJoint3D ponko_joint3d_spherical(uint64_t body_a, uint64_t body_b);

struct PonkoJoint3D ponko_joint3d_revolute(uint64_t body_a, uint64_t body_b);

struct PonkoJoint3D ponko_joint3d_prismatic(uint64_t body_a, uint64_t body_b);

struct PonkoCollisionGroups ponko_collision_groups_all(void);

struct PonkoQueryFilter3D ponko_query_filter3d(void);

struct PonkoRigidBody3D ponko_rigid_body3d(uint32_t kind);

struct PonkoCollider3D ponko_collider3d_sphere(float radius);

struct PonkoCollider3D ponko_collider3d_box(struct PonkoVec3 size);

struct PonkoCollider3D ponko_collider3d_capsule(float height, float radius);

struct PonkoCollider3D ponko_collider3d_mesh(struct PonkoCollisionMesh mesh);

struct PonkoCollider3D ponko_collider3d_scaled_mesh(struct PonkoCollisionMesh mesh,
                                                    struct PonkoVec3 scale);

struct PonkoCharacter3D ponko_character3d(float height, float radius);

enum PonkoStatus ponko_rigid_body3d_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_collider3d_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_velocity3d_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_character3d_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_joint3d_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_ctx_physics3d_joint_count(struct PonkoCtx *ctx, size_t *out_count);

enum PonkoStatus ponko_game_physics3d(struct PonkoGame *game, struct PonkoVec3 gravity);

enum PonkoStatus ponko_ctx_set_gravity3d(struct PonkoCtx *ctx, struct PonkoVec3 gravity);

enum PonkoStatus ponko_ctx_physics3d_heightfield(struct PonkoCtx *ctx,
                                                 const float *heights,
                                                 size_t count,
                                                 float size,
                                                 struct PonkoCollisionMesh *out_mesh);

enum PonkoStatus ponko_ctx_physics3d_convex_hull(struct PonkoCtx *ctx,
                                                 const struct PonkoVec3 *points,
                                                 size_t count,
                                                 struct PonkoCollisionMesh *out_mesh);

enum PonkoStatus ponko_ctx_physics3d_trimesh(struct PonkoCtx *ctx,
                                             const struct PonkoVec3 *vertices,
                                             size_t vertex_count,
                                             const uint32_t *indices,
                                             size_t index_count,
                                             struct PonkoCollisionMesh *out_mesh);

enum PonkoStatus ponko_ctx_physics3d_free_mesh(struct PonkoCtx *ctx,
                                               struct PonkoCollisionMesh mesh);

enum PonkoStatus ponko_ctx_cast_ray3d(struct PonkoCtx *ctx,
                                      struct PonkoVec3 origin,
                                      struct PonkoVec3 direction,
                                      float max_distance,
                                      struct PonkoQueryFilter3D filter,
                                      bool *out_found,
                                      struct PonkoHit3D *out_hit);

enum PonkoStatus ponko_ctx_cast_ray3d_all(struct PonkoCtx *ctx,
                                          struct PonkoVec3 origin,
                                          struct PonkoVec3 direction,
                                          float max_distance,
                                          struct PonkoQueryFilter3D filter,
                                          struct PonkoHit3D *out_hits,
                                          size_t capacity,
                                          size_t *out_count);

enum PonkoStatus ponko_ctx_cast_sphere3d(struct PonkoCtx *ctx,
                                         struct PonkoVec3 origin,
                                         float radius,
                                         struct PonkoVec3 direction,
                                         float max_distance,
                                         struct PonkoQueryFilter3D filter,
                                         bool *out_found,
                                         struct PonkoHit3D *out_hit);

enum PonkoStatus ponko_ctx_cast_capsule3d(struct PonkoCtx *ctx,
                                          struct PonkoVec3 origin,
                                          float height,
                                          float radius,
                                          struct PonkoVec3 direction,
                                          float max_distance,
                                          struct PonkoQueryFilter3D filter,
                                          bool *out_found,
                                          struct PonkoHit3D *out_hit);

enum PonkoStatus ponko_ctx_overlap_sphere3d(struct PonkoCtx *ctx,
                                            struct PonkoVec3 center,
                                            float radius,
                                            struct PonkoQueryFilter3D filter,
                                            struct PonkoEntity *out_entities,
                                            size_t capacity,
                                            size_t *out_count);

enum PonkoStatus ponko_ctx_overlap_box3d(struct PonkoCtx *ctx,
                                         struct PonkoVec3 center,
                                         struct PonkoVec3 size,
                                         struct PonkoQuat rotation,
                                         struct PonkoQueryFilter3D filter,
                                         struct PonkoEntity *out_entities,
                                         size_t capacity,
                                         size_t *out_count);

enum PonkoStatus ponko_ctx_overlap_capsule3d(struct PonkoCtx *ctx,
                                             struct PonkoVec3 center,
                                             float height,
                                             float radius,
                                             struct PonkoQueryFilter3D filter,
                                             struct PonkoEntity *out_entities,
                                             size_t capacity,
                                             size_t *out_count);

enum PonkoStatus ponko_ctx_apply_impulse3d(struct PonkoCtx *ctx,
                                           uint64_t entity,
                                           struct PonkoVec3 impulse);

enum PonkoStatus ponko_ctx_body_mass3d(struct PonkoCtx *ctx, uint64_t entity, float *out_mass);

enum PonkoStatus ponko_ctx_body_center_of_mass3d(struct PonkoCtx *ctx,
                                                 uint64_t entity,
                                                 struct PonkoVec3 *out_center);

struct PonkoTerrainCollider ponko_terrain_collider(struct PonkoCollisionMesh mesh);

struct PonkoColliderFocus ponko_collider_focus(float radius, float keep_radius);

enum PonkoStatus ponko_terrain_collider_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_collider_focus_component(struct PonkoWorld *world, uint32_t *out_id);

struct PonkoCollisionMeshData *ponko_collision_mesh_data_heightfield(const float *heights,
                                                                     size_t count,
                                                                     float size);

struct PonkoCollisionMeshData *ponko_collision_mesh_data_trimesh(const struct PonkoVec3 *vertices,
                                                                 size_t vertex_count,
                                                                 const uint32_t *indices,
                                                                 size_t index_count);

struct PonkoCollisionMeshData *ponko_collision_mesh_data_convex_hull(const struct PonkoVec3 *points,
                                                                     size_t count);

void ponko_collision_mesh_data_free(struct PonkoCollisionMeshData *data);

enum PonkoStatus ponko_ctx_physics3d_add_mesh(struct PonkoCtx *ctx,
                                              const struct PonkoCollisionMeshData *data,
                                              struct PonkoCollisionMesh *out_mesh);

enum PonkoStatus ponko_ctx_physics3d_stream_colliders(struct PonkoCtx *ctx,
                                                      bool enabled,
                                                      size_t max_new);

enum PonkoStatus ponko_ctx_physics3d_is_collider_active(struct PonkoCtx *ctx,
                                                        uint64_t entity,
                                                        bool *out_active);

enum PonkoStatus ponko_ctx_set_physics_paused(struct PonkoCtx *ctx, bool paused);

enum PonkoStatus ponko_ctx_is_physics_paused(struct PonkoCtx *ctx, bool *out_paused);

enum PonkoStatus ponko_ctx_set_physics3d_paused(struct PonkoCtx *ctx, bool paused);

enum PonkoStatus ponko_ctx_is_physics3d_paused(struct PonkoCtx *ctx, bool *out_paused);

enum PonkoStatus ponko_set_collider_groups(struct PonkoWorld *world,
                                           uint64_t entity,
                                           struct PonkoCollisionGroups groups);

enum PonkoStatus ponko_collider_groups(struct PonkoWorld *world,
                                       uint64_t entity,
                                       struct PonkoCollisionGroups *out_groups);

enum PonkoStatus ponko_ctx_cast_ray2d(struct PonkoCtx *ctx,
                                      struct PonkoVec2 origin,
                                      struct PonkoVec2 direction,
                                      float max_distance,
                                      struct PonkoQueryFilter2D filter,
                                      struct PonkoCollisionGroups groups,
                                      bool *out_found,
                                      struct PonkoRayHit *out_hit);

enum PonkoStatus ponko_ctx_cast_ray2d_all(struct PonkoCtx *ctx,
                                          struct PonkoVec2 origin,
                                          struct PonkoVec2 direction,
                                          float max_distance,
                                          struct PonkoQueryFilter2D filter,
                                          struct PonkoCollisionGroups groups,
                                          struct PonkoRayHit *out_hits,
                                          size_t capacity,
                                          size_t *out_count);

enum PonkoStatus ponko_ctx_cast_circle2d(struct PonkoCtx *ctx,
                                         struct PonkoVec2 origin,
                                         float radius,
                                         struct PonkoVec2 direction,
                                         float max_distance,
                                         struct PonkoQueryFilter2D filter,
                                         struct PonkoCollisionGroups groups,
                                         bool *out_found,
                                         struct PonkoRayHit *out_hit);

enum PonkoStatus ponko_ctx_cast_box2d(struct PonkoCtx *ctx,
                                      struct PonkoVec2 origin,
                                      struct PonkoVec2 size,
                                      float rotation,
                                      struct PonkoVec2 direction,
                                      float max_distance,
                                      struct PonkoQueryFilter2D filter,
                                      struct PonkoCollisionGroups groups,
                                      bool *out_found,
                                      struct PonkoRayHit *out_hit);

enum PonkoStatus ponko_ctx_cast_capsule2d(struct PonkoCtx *ctx,
                                          struct PonkoVec2 origin,
                                          float height,
                                          float radius,
                                          struct PonkoVec2 direction,
                                          float max_distance,
                                          struct PonkoQueryFilter2D filter,
                                          struct PonkoCollisionGroups groups,
                                          bool *out_found,
                                          struct PonkoRayHit *out_hit);

enum PonkoStatus ponko_ctx_overlap_point2d(struct PonkoCtx *ctx,
                                           struct PonkoVec2 point,
                                           struct PonkoQueryFilter2D filter,
                                           struct PonkoCollisionGroups groups,
                                           struct PonkoEntity *out_entities,
                                           size_t capacity,
                                           size_t *out_count);

enum PonkoStatus ponko_ctx_overlap_circle2d(struct PonkoCtx *ctx,
                                            struct PonkoVec2 center,
                                            float radius,
                                            struct PonkoQueryFilter2D filter,
                                            struct PonkoCollisionGroups groups,
                                            struct PonkoEntity *out_entities,
                                            size_t capacity,
                                            size_t *out_count);

enum PonkoStatus ponko_ctx_overlap_box2d(struct PonkoCtx *ctx,
                                         struct PonkoVec2 center,
                                         struct PonkoVec2 size,
                                         float rotation,
                                         struct PonkoQueryFilter2D filter,
                                         struct PonkoCollisionGroups groups,
                                         struct PonkoEntity *out_entities,
                                         size_t capacity,
                                         size_t *out_count);

enum PonkoStatus ponko_ctx_overlap_capsule2d(struct PonkoCtx *ctx,
                                             struct PonkoVec2 center,
                                             float height,
                                             float radius,
                                             struct PonkoQueryFilter2D filter,
                                             struct PonkoCollisionGroups groups,
                                             struct PonkoEntity *out_entities,
                                             size_t capacity,
                                             size_t *out_count);

enum PonkoStatus ponko_ctx_closest_point2d(struct PonkoCtx *ctx,
                                           struct PonkoVec2 point,
                                           float max_distance,
                                           struct PonkoQueryFilter2D filter,
                                           struct PonkoCollisionGroups groups,
                                           bool *out_found,
                                           struct PonkoPointHit2D *out_hit);

enum PonkoStatus ponko_ctx_penetrations2d(struct PonkoCtx *ctx,
                                          struct PonkoCollider shape,
                                          struct PonkoVec2 position,
                                          float rotation,
                                          struct PonkoQueryFilter2D filter,
                                          struct PonkoCollisionGroups groups,
                                          struct PonkoPenetration2D *out_penetrations,
                                          size_t capacity,
                                          size_t *out_count);

enum PonkoStatus ponko_ctx_cast_box3d(struct PonkoCtx *ctx,
                                      struct PonkoVec3 origin,
                                      struct PonkoVec3 size,
                                      struct PonkoQuat rotation,
                                      struct PonkoVec3 direction,
                                      float max_distance,
                                      struct PonkoQueryFilter3D filter,
                                      bool *out_found,
                                      struct PonkoHit3D *out_hit);

enum PonkoStatus ponko_ctx_overlap_point3d(struct PonkoCtx *ctx,
                                           struct PonkoVec3 point,
                                           struct PonkoQueryFilter3D filter,
                                           struct PonkoEntity *out_entities,
                                           size_t capacity,
                                           size_t *out_count);

enum PonkoStatus ponko_ctx_closest_point3d(struct PonkoCtx *ctx,
                                           struct PonkoVec3 point,
                                           float max_distance,
                                           struct PonkoQueryFilter3D filter,
                                           bool *out_found,
                                           struct PonkoPointHit3D *out_hit);

enum PonkoStatus ponko_ctx_penetrations3d(struct PonkoCtx *ctx,
                                          struct PonkoCollider3D shape,
                                          struct PonkoVec3 position,
                                          struct PonkoQuat rotation,
                                          struct PonkoQueryFilter3D filter,
                                          struct PonkoPenetration3D *out_penetrations,
                                          size_t capacity,
                                          size_t *out_count);

enum PonkoStatus ponko_net_messages_rtc_host(struct PonkoLobby *lobby,
                                             const char *stun_urls,
                                             const char *bind,
                                             size_t max_clients,
                                             struct PonkoNetMessages **out_endpoint);

enum PonkoStatus ponko_net_messages_rtc_join(struct PonkoLobby *lobby,
                                             uint32_t host,
                                             const char *stun_urls,
                                             const char *bind,
                                             struct PonkoNetMessages **out_endpoint);

enum PonkoStatus ponko_net_messages_rtc_peer(struct PonkoNetMessages *endpoint,
                                             uint64_t peer,
                                             uint32_t *out_member);

enum PonkoStatus ponko_game_state(struct PonkoGame *game, uint32_t initial);

enum PonkoStatus ponko_game_on_enter(struct PonkoGame *game,
                                     uint32_t state,
                                     PonkoSystemFn system_fn,
                                     void *user);

enum PonkoStatus ponko_game_on_exit(struct PonkoGame *game,
                                    uint32_t state,
                                    PonkoSystemFn system_fn,
                                    void *user);

enum PonkoStatus ponko_game_state_screen(struct PonkoGame *game,
                                         uint32_t state,
                                         uint32_t mode,
                                         PonkoUiBuildFn build_fn,
                                         void *user);

enum PonkoStatus ponko_game_add_system_in_state(struct PonkoGame *game,
                                                uint32_t phase,
                                                const char *name,
                                                uint32_t every,
                                                uint32_t state,
                                                PonkoSystemFn system_fn,
                                                void *user);

enum PonkoStatus ponko_game_add_system_in_stack(struct PonkoGame *game,
                                                uint32_t phase,
                                                const char *name,
                                                uint32_t every,
                                                uint32_t state,
                                                PonkoSystemFn system_fn,
                                                void *user);

enum PonkoStatus ponko_game_add_system_if(struct PonkoGame *game,
                                          uint32_t phase,
                                          const char *name,
                                          uint32_t every,
                                          PonkoConditionFn condition,
                                          void *condition_user,
                                          PonkoSystemFn system_fn,
                                          void *user);

enum PonkoStatus ponko_ctx_state(struct PonkoCtx *ctx, uint32_t *out_state);

enum PonkoStatus ponko_ctx_state_ticks(struct PonkoCtx *ctx, uint64_t *out_ticks);

enum PonkoStatus ponko_ctx_state_in_stack(struct PonkoCtx *ctx, uint32_t state, bool *out_in_stack);

enum PonkoStatus ponko_ctx_set_state(struct PonkoCtx *ctx, uint32_t state);

enum PonkoStatus ponko_ctx_push_state(struct PonkoCtx *ctx, uint32_t state);

enum PonkoStatus ponko_ctx_pop_state(struct PonkoCtx *ctx);

enum PonkoStatus ponko_ctx_load_collision_mesh(struct PonkoCtx *ctx,
                                               const char *path,
                                               struct PonkoVec3 scale,
                                               bool convex,
                                               struct PonkoCollisionMesh *out_mesh);

enum PonkoStatus ponko_ctx_prefab_parts(struct PonkoCtx *ctx,
                                        uint64_t root,
                                        uint64_t *out_entities,
                                        size_t capacity,
                                        size_t *out_count);

enum PonkoStatus ponko_ctx_despawn_prefab(struct PonkoCtx *ctx, uint64_t root, size_t *out_count);

enum PonkoStatus ponko_ctx_pressed_this_frame(struct PonkoCtx *ctx,
                                              float axis_threshold,
                                              struct PonkoActionBinding *out,
                                              size_t capacity,
                                              size_t *out_count);

struct PonkoQueryCachePolicy ponko_query_cache_policy_default(void);

struct PonkoQueryCachePolicy ponko_query_cache_policy_off(void);

enum PonkoStatus ponko_world_set_query_cache_policy(struct PonkoWorld *world,
                                                    const struct PonkoQueryCachePolicy *policy);

enum PonkoStatus ponko_world_query_cache_policy(struct PonkoWorld *world,
                                                struct PonkoQueryCachePolicy *out_policy);

enum PonkoStatus ponko_world_query_cache_stats(struct PonkoWorld *world,
                                               struct PonkoQueryCacheStats *out_stats);

enum PonkoStatus ponko_query_cached(struct PonkoWorld *world,
                                    const uint32_t *components,
                                    size_t count,
                                    const struct PonkoFilter *filter,
                                    const struct PonkoQueryOptions *options,
                                    uint32_t cache,
                                    PonkoQueryFn callback,
                                    void *user);

struct PonkoRagdollBone ponko_ragdoll_bone(const char *joint, float radius);

enum PonkoStatus ponko_ctx_start_ragdoll(struct PonkoCtx *ctx,
                                         uint64_t entity,
                                         const struct PonkoRagdollBone *bones,
                                         size_t count);

enum PonkoStatus ponko_ctx_stop_ragdoll(struct PonkoCtx *ctx, uint64_t entity, float seconds);

enum PonkoStatus ponko_ctx_ragdoll_body(struct PonkoCtx *ctx,
                                        uint64_t entity,
                                        size_t joint,
                                        uint64_t *out_body);

enum PonkoStatus ponko_ctx_ragdoll_state(struct PonkoCtx *ctx,
                                         uint64_t entity,
                                         bool *out_ragdolled,
                                         bool *out_blending);

enum PonkoStatus ponko_component_reflect(struct PonkoWorld *world,
                                         uint32_t component_id,
                                         const struct PonkoField *fields,
                                         size_t len);

enum PonkoStatus ponko_world_save(struct PonkoWorld *world,
                                  char *buffer,
                                  size_t capacity,
                                  size_t *out_len);

enum PonkoStatus ponko_world_hash(struct PonkoWorld *world, uint64_t *out_hash);

enum PonkoStatus ponko_world_load(struct PonkoWorld *world, const char *json);

enum PonkoStatus ponko_world_describe_component(struct PonkoWorld *world,
                                                const char *name,
                                                const char *schema);

enum PonkoStatus ponko_world_describe_resource(struct PonkoWorld *world,
                                               const char *name,
                                               const char *schema);

enum PonkoStatus ponko_ctx_capabilities_json(const struct PonkoCtx *ctx,
                                             char *buffer,
                                             size_t capacity,
                                             size_t *out_len);

bool ponko_seq_newer(uint16_t a, uint16_t b);

int16_t ponko_seq_diff(uint16_t a, uint16_t b);

bool ponko_stamp_newer(struct PonkoUpdateStamp a, struct PonkoUpdateStamp b);

struct PonkoAuthority ponko_authority_new(uint64_t owner);

enum PonkoStatus ponko_authority_handoff(struct PonkoAuthority *authority,
                                         uint64_t new_owner,
                                         uint16_t *out_epoch);

enum PonkoStatus ponko_authority_adopt(struct PonkoAuthority *authority,
                                       uint64_t owner,
                                       uint16_t epoch,
                                       bool *out_changed);

enum PonkoStatus ponko_authority_check(const struct PonkoAuthority *authority,
                                       uint64_t from,
                                       uint16_t epoch,
                                       enum PonkoAuthorityCheck *out_check);

struct PonkoLatestWins *ponko_latest_wins_new(void);

void ponko_latest_wins_free(struct PonkoLatestWins *filter);

enum PonkoStatus ponko_latest_wins_accept(struct PonkoLatestWins *f,
                                          uint64_t key,
                                          struct PonkoUpdateStamp stamp,
                                          bool *out_accepted);

enum PonkoStatus ponko_latest_wins_forget(struct PonkoLatestWins *f, uint64_t key);

enum PonkoStatus ponko_latest_wins_clear(struct PonkoLatestWins *f);

size_t ponko_latest_wins_len(const struct PonkoLatestWins *f);

struct PonkoSnapshotConfig ponko_snapshot_config_default(void);

enum PonkoStatus ponko_snapshot_stream_new(const struct PonkoSnapshotConfig *config,
                                           struct PonkoSnapshotStream **out_stream);

void ponko_snapshot_stream_free(struct PonkoSnapshotStream *s);

enum PonkoStatus ponko_snapshot_stream_push(struct PonkoSnapshotStream *s,
                                            double remote_time,
                                            double local_now,
                                            const struct PonkoPose *pose,
                                            enum PonkoSnapshotPush *out_result);

enum PonkoStatus ponko_snapshot_stream_sample(const struct PonkoSnapshotStream *s,
                                              double now,
                                              struct PonkoPose *out_pose,
                                              enum PonkoSnapshotState *out_state,
                                              bool *out_found);

enum PonkoStatus ponko_snapshot_stream_set_delay(struct PonkoSnapshotStream *s, double delay);

enum PonkoStatus ponko_snapshot_stream_clear(struct PonkoSnapshotStream *s);

enum PonkoStatus ponko_snapshot_stream_reset(struct PonkoSnapshotStream *s);

size_t ponko_snapshot_stream_len(const struct PonkoSnapshotStream *s);

enum PonkoStatus ponko_snapshot_stream_clock_offset(const struct PonkoSnapshotStream *s,
                                                    double *out_offset,
                                                    bool *out_known);

struct PonkoInterestConfig ponko_interest_config_default(void);

enum PonkoStatus ponko_interest_new(const struct PonkoInterestConfig *config,
                                    struct PonkoInterest **out_interest);

void ponko_interest_free(struct PonkoInterest *i);

enum PonkoStatus ponko_interest_update(struct PonkoInterest *i,
                                       uint64_t tick,
                                       const struct PonkoInterestViewer *viewers,
                                       size_t viewer_count,
                                       const struct PonkoInterestEntity *entities,
                                       size_t entity_count);

enum PonkoStatus ponko_interest_entries(const struct PonkoInterest *i,
                                        uint64_t peer,
                                        struct PonkoInterestEntry *out_entries,
                                        size_t capacity,
                                        size_t *out_count);

enum PonkoStatus ponko_interest_changes(const struct PonkoInterest *i,
                                        struct PonkoInterestChange *out_changes,
                                        size_t capacity,
                                        size_t *out_count);

enum PonkoStatus ponko_interest_remove_peer(struct PonkoInterest *i, uint64_t peer);

struct PonkoReplicated ponko_replicated_default(void);

enum PonkoStatus ponko_game_messages(struct PonkoGame *game, struct PonkoNetMessages *endpoint);

enum PonkoStatus ponko_net_messages_game_clock(struct PonkoNetMessages *endpoint);

enum PonkoStatus ponko_ctx_replication(struct PonkoCtx *ctx,
                                       const struct PonkoReplicatedComponent *components,
                                       size_t count,
                                       uint32_t budget,
                                       const struct PonkoInterestConfig *interest);

enum PonkoStatus ponko_ctx_replicate(struct PonkoCtx *ctx,
                                     uint64_t entity,
                                     const struct PonkoReplicated *how);

enum PonkoStatus ponko_ctx_replication_status(struct PonkoCtx *ctx,
                                              struct PonkoReplicationStatus *out_status);

enum PonkoStatus ponko_ctx_net_id(struct PonkoCtx *ctx, uint64_t entity, uint32_t *out_id);

enum PonkoStatus ponko_ctx_net_entity(struct PonkoCtx *ctx, uint32_t id, uint64_t *out_entity);

enum PonkoStatus ponko_ctx_replica(struct PonkoCtx *ctx,
                                   uint64_t entity,
                                   struct PonkoReplica *out_replica);

enum PonkoStatus ponko_ctx_replication_clients(struct PonkoCtx *ctx,
                                               uint64_t *out_peers,
                                               size_t capacity,
                                               size_t *out_count);

enum PonkoStatus ponko_ctx_replication_set_viewer(struct PonkoCtx *ctx,
                                                  uint64_t peer,
                                                  const struct PonkoVec3 *position);

enum PonkoStatus ponko_ctx_replication_peer_stats(struct PonkoCtx *ctx,
                                                  uint64_t peer,
                                                  struct PonkoReplicationPeerStats *out_stats);

enum PonkoStatus ponko_ctx_replication_resend_stats(struct PonkoCtx *ctx,
                                                    uint64_t peer,
                                                    struct PonkoReplicationResendStats *out_stats);

enum PonkoStatus ponko_ctx_replication_client_stats(struct PonkoCtx *ctx,
                                                    struct PonkoReplicationClientStats *out_stats);

enum PonkoStatus ponko_component_require(struct PonkoWorld *world,
                                         uint32_t component,
                                         uint32_t required,
                                         const void *default_value);

enum PonkoStatus ponko_ctx_set_residency_budget(struct PonkoCtx *ctx,
                                                uint32_t kind,
                                                bool has_budget,
                                                uint64_t bytes);

enum PonkoStatus ponko_ctx_residency(struct PonkoCtx *ctx,
                                     uint32_t kind,
                                     struct PonkoResidencyStats *out_stats);

enum PonkoStatus ponko_ctx_is_resident(struct PonkoCtx *ctx,
                                       uint32_t kind,
                                       const char *name,
                                       bool *out_resident);

enum PonkoStatus ponko_ctx_asset_state(struct PonkoCtx *ctx,
                                       uint32_t kind,
                                       const char *name,
                                       uint32_t *out_state);

enum PonkoStatus ponko_ctx_asset_error(struct PonkoCtx *ctx,
                                       uint32_t kind,
                                       const char *name,
                                       char *buffer,
                                       size_t capacity,
                                       size_t *out_len);

enum PonkoStatus ponko_ctx_resident_texture(struct PonkoCtx *ctx,
                                            const char *path,
                                            bool pin,
                                            struct PonkoTexture *out_texture);

enum PonkoStatus ponko_ctx_resident_mesh(struct PonkoCtx *ctx,
                                         const char *path,
                                         bool pin,
                                         struct PonkoMesh *out_mesh);

enum PonkoStatus ponko_ctx_unpin_resident(struct PonkoCtx *ctx, uint32_t kind, const char *path);

enum PonkoStatus ponko_ctx_stream_texture(struct PonkoCtx *ctx,
                                          const char *path,
                                          struct PonkoTexture *out_texture,
                                          bool *out_ready);

enum PonkoStatus ponko_ctx_stream_mesh(struct PonkoCtx *ctx,
                                       const char *path,
                                       struct PonkoMesh *out_mesh,
                                       bool *out_ready);

enum PonkoStatus ponko_ctx_set_upload_budget(struct PonkoCtx *ctx, bool has_budget, uint64_t bytes);

enum PonkoStatus ponko_ctx_set_deterministic_uploads(struct PonkoCtx *ctx, bool on);

enum PonkoStatus ponko_ctx_upload_stats(struct PonkoCtx *ctx, struct PonkoUploadStats *out_stats);

enum PonkoStatus ponko_ctx_finish_uploads(struct PonkoCtx *ctx);

enum PonkoStatus ponko_ctx_set_streamed_lod(struct PonkoCtx *ctx,
                                            uint64_t entity,
                                            const char *const *files,
                                            size_t count);

enum PonkoStatus ponko_resource_set(struct PonkoWorld *world,
                                    const char *name,
                                    const void *data,
                                    size_t size,
                                    size_t alignment);

enum PonkoStatus ponko_resource_get(struct PonkoWorld *world,
                                    const char *name,
                                    void *out,
                                    size_t size);

enum PonkoStatus ponko_resource_remove(struct PonkoWorld *world, const char *name);

enum PonkoStatus ponko_resource_reflect(struct PonkoWorld *world,
                                        const char *name,
                                        size_t size,
                                        size_t alignment,
                                        const struct PonkoField *fields,
                                        size_t len);

enum PonkoStatus ponko_resource_scope(struct PonkoWorld *world,
                                      const char *name,
                                      size_t size,
                                      PonkoResourceScopeFn scope,
                                      void *user);

enum PonkoStatus ponko_world_set_save_version(struct PonkoWorld *world, uint32_t version);

enum PonkoStatus ponko_world_save_version(struct PonkoWorld *world, uint32_t *out_version);

enum PonkoStatus ponko_save_migration_rename_component(struct PonkoWorld *world,
                                                       uint32_t from_version,
                                                       const char *from,
                                                       const char *to);

enum PonkoStatus ponko_save_migration_remove_component(struct PonkoWorld *world,
                                                       uint32_t from_version,
                                                       const char *name);

enum PonkoStatus ponko_save_migration_rename_field(struct PonkoWorld *world,
                                                   uint32_t from_version,
                                                   const char *component,
                                                   const char *from,
                                                   const char *to);

enum PonkoStatus ponko_save_migration_add_field(struct PonkoWorld *world,
                                                uint32_t from_version,
                                                const char *component,
                                                const char *field,
                                                const char *value_json);

enum PonkoStatus ponko_save_migration_remove_field(struct PonkoWorld *world,
                                                   uint32_t from_version,
                                                   const char *component,
                                                   const char *field);

enum PonkoStatus ponko_save_migration_split_component(struct PonkoWorld *world,
                                                      uint32_t from_version,
                                                      const char *from,
                                                      const char *parts_json);

enum PonkoStatus ponko_save_migration_merge_components(struct PonkoWorld *world,
                                                       uint32_t from_version,
                                                       const char *const *from,
                                                       size_t count,
                                                       const char *into);

enum PonkoStatus ponko_save_migration_rename_resource(struct PonkoWorld *world,
                                                      uint32_t from_version,
                                                      const char *from,
                                                      const char *to);

enum PonkoStatus ponko_save_migration_remove_resource(struct PonkoWorld *world,
                                                      uint32_t from_version,
                                                      const char *name);

enum PonkoStatus ponko_save_migration_convert_field(struct PonkoWorld *world,
                                                    uint32_t from_version,
                                                    const char *component,
                                                    const char *field,
                                                    PonkoJsonConvertFn convert_fn,
                                                    void *user,
                                                    PonkoUserFreeFn free);

enum PonkoStatus ponko_save_migration_convert_component(struct PonkoWorld *world,
                                                        uint32_t from_version,
                                                        const char *component,
                                                        PonkoJsonConvertFn convert_fn,
                                                        void *user,
                                                        PonkoUserFreeFn free);

enum PonkoStatus ponko_save_migration_convert_resource(struct PonkoWorld *world,
                                                       uint32_t from_version,
                                                       const char *resource,
                                                       PonkoJsonConvertFn convert_fn,
                                                       void *user,
                                                       PonkoUserFreeFn free);

enum PonkoStatus ponko_save_migration_change_entities(struct PonkoWorld *world,
                                                      uint32_t from_version,
                                                      PonkoJsonConvertFn convert_fn,
                                                      void *user,
                                                      PonkoUserFreeFn free);

enum PonkoStatus ponko_save_migration_change_save(struct PonkoWorld *world,
                                                  uint32_t from_version,
                                                  PonkoJsonConvertFn convert_fn,
                                                  void *user,
                                                  PonkoUserFreeFn free);

enum PonkoStatus ponko_world_migrate_save(struct PonkoWorld *world,
                                          const char *json,
                                          char *buffer,
                                          size_t capacity,
                                          size_t *out_len);

enum PonkoStatus ponko_ctx_check_old_saves(struct PonkoCtx *ctx,
                                           const char *folder,
                                           size_t *out_count);

enum PonkoStatus ponko_world_set_saved(struct PonkoWorld *world, uint64_t entity, bool saved);

enum PonkoStatus ponko_world_is_saved(struct PonkoWorld *world, uint64_t entity, bool *out_saved);

enum PonkoStatus ponko_ctx_spawn_level_without_saved_objects(struct PonkoCtx *ctx,
                                                             const char *path,
                                                             struct PonkoVec2 at,
                                                             const struct PonkoLevelOptions *options,
                                                             uint64_t *out_root);

struct PonkoInputScenario *ponko_input_scenario_new(void);

void ponko_input_scenario_free(struct PonkoInputScenario *scenario);

enum PonkoStatus ponko_input_scenario_from_json(const char *json,
                                                struct PonkoInputScenario **out_scenario);

enum PonkoStatus ponko_input_scenario_to_json(const struct PonkoInputScenario *scenario,
                                              char *buffer,
                                              size_t capacity,
                                              size_t *out_len);

enum PonkoStatus ponko_input_scenario_at(struct PonkoInputScenario *scenario,
                                         uint64_t tick,
                                         uint32_t kind,
                                         uint32_t slot,
                                         uint32_t input,
                                         float value);

enum PonkoStatus ponko_input_scenario_mouse_move(struct PonkoInputScenario *scenario,
                                                 uint64_t tick,
                                                 struct PonkoVec2 position);

enum PonkoStatus ponko_input_scenario_mouse_wheel(struct PonkoInputScenario *scenario,
                                                  uint64_t tick,
                                                  struct PonkoVec2 lines);

enum PonkoStatus ponko_input_scenario_expect_action(struct PonkoInputScenario *scenario,
                                                    uint64_t tick,
                                                    const char *name,
                                                    int32_t pressed,
                                                    int32_t down,
                                                    int32_t released);

enum PonkoStatus ponko_input_scenario_expect_resource(struct PonkoInputScenario *scenario,
                                                      uint64_t tick,
                                                      const char *name,
                                                      const char *pointer,
                                                      const char *equals_json);

enum PonkoStatus ponko_input_scenario_expect_count(struct PonkoInputScenario *scenario,
                                                   uint64_t tick,
                                                   const char *component,
                                                   uint64_t count);

enum PonkoStatus ponko_input_scenario_named(struct PonkoInputScenario *scenario, const char *id);

enum PonkoStatus ponko_input_scenario_duration(struct PonkoInputScenario *scenario, uint64_t ticks);

enum PonkoStatus ponko_input_scenario_release_when_done(struct PonkoInputScenario *scenario);

enum PonkoStatus ponko_input_scenario_info(const struct PonkoInputScenario *scenario,
                                           uint64_t *out_ticks,
                                           size_t *out_steps);

enum PonkoStatus ponko_game_input_scenario(struct PonkoGame *game,
                                           const struct PonkoInputScenario *scenario);

enum PonkoStatus ponko_ctx_start_scenario(struct PonkoCtx *ctx,
                                          const struct PonkoInputScenario *scenario);

enum PonkoStatus ponko_ctx_cancel_scenario(struct PonkoCtx *ctx, bool *out_was_running);

enum PonkoStatus ponko_ctx_start_input_recording(struct PonkoCtx *ctx);

enum PonkoStatus ponko_ctx_stop_input_recording(struct PonkoCtx *ctx,
                                                struct PonkoInputScenario **out_scenario);

enum PonkoStatus ponko_world_scenario_report(struct PonkoWorld *world,
                                             struct PonkoScenarioReport *out_report);

enum PonkoStatus ponko_world_scenario_report_json(struct PonkoWorld *world,
                                                  char *buffer,
                                                  size_t capacity,
                                                  size_t *out_len);

enum PonkoStatus ponko_ctx_load_scene(struct PonkoCtx *ctx, const char *path);

enum PonkoStatus ponko_ctx_load_scene_lockstep(struct PonkoCtx *ctx, const char *path);

enum PonkoStatus ponko_ctx_save_scene(struct PonkoCtx *ctx, const char *path);

enum PonkoStatus ponko_ctx_spawn_prefab(struct PonkoCtx *ctx,
                                        const char *path,
                                        struct PonkoVec2 at,
                                        uint64_t *out_entity);

enum PonkoStatus ponko_ctx_save_prefab(struct PonkoCtx *ctx, uint64_t entity, const char *path);

enum PonkoStatus ponko_ctx_shader_status_count(const struct PonkoCtx *ctx, size_t *out_count);

enum PonkoStatus ponko_ctx_shader_status(const struct PonkoCtx *ctx,
                                         size_t index,
                                         struct PonkoShaderStatus *out_status);

enum PonkoStatus ponko_ctx_shader_status_name(const struct PonkoCtx *ctx,
                                              size_t index,
                                              char *buffer,
                                              size_t capacity,
                                              size_t *out_len);

enum PonkoStatus ponko_ctx_shader_status_text(const struct PonkoCtx *ctx,
                                              size_t index,
                                              char *buffer,
                                              size_t capacity,
                                              size_t *out_len);

enum PonkoStatus ponko_ctx_shader_status_json(const struct PonkoCtx *ctx,
                                              char *buffer,
                                              size_t capacity,
                                              size_t *out_len);

enum PonkoStatus ponko_ctx_load_skeleton2d(struct PonkoCtx *ctx,
                                           const char *path,
                                           size_t *out_warnings);

enum PonkoStatus ponko_ctx_skeleton2d_warning(struct PonkoCtx *ctx,
                                              const char *path,
                                              size_t index,
                                              char *buffer,
                                              size_t capacity,
                                              size_t *out_len);

enum PonkoStatus ponko_ctx_set_skeleton2d(struct PonkoCtx *ctx,
                                          uint64_t entity_bits,
                                          const char *path);

enum PonkoStatus ponko_ctx_remove_skeleton2d(struct PonkoCtx *ctx, uint64_t entity_bits);

enum PonkoStatus ponko_ctx_skeleton2d_play(struct PonkoCtx *ctx,
                                           uint64_t entity_bits,
                                           const char *clip,
                                           bool looping);

enum PonkoStatus ponko_ctx_skeleton2d_crossfade(struct PonkoCtx *ctx,
                                                uint64_t entity_bits,
                                                const char *clip,
                                                float duration);

enum PonkoStatus ponko_ctx_skeleton2d_crossfade_state(struct PonkoCtx *ctx,
                                                      uint64_t entity_bits,
                                                      size_t state,
                                                      float duration,
                                                      float offset);

enum PonkoStatus ponko_ctx_skeleton2d_state(struct PonkoCtx *ctx,
                                            uint64_t entity_bits,
                                            size_t *out_state,
                                            float *out_normalized_time,
                                            bool *out_finished);

enum PonkoStatus ponko_ctx_skeleton2d_normalized_time_f64(struct PonkoCtx *ctx,
                                                          uint64_t entity_bits,
                                                          double *out_time);

enum PonkoStatus ponko_ctx_skeleton2d_state_index(struct PonkoCtx *ctx,
                                                  uint64_t entity_bits,
                                                  const char *name,
                                                  size_t *out_state);

enum PonkoStatus ponko_ctx_skeleton2d_parameter_index(struct PonkoCtx *ctx,
                                                      uint64_t entity_bits,
                                                      const char *name,
                                                      size_t *out_parameter);

enum PonkoStatus ponko_ctx_skeleton2d_set_float(struct PonkoCtx *ctx,
                                                uint64_t entity_bits,
                                                size_t parameter,
                                                float value);

enum PonkoStatus ponko_ctx_skeleton2d_set_bool(struct PonkoCtx *ctx,
                                               uint64_t entity_bits,
                                               size_t parameter,
                                               bool value);

enum PonkoStatus ponko_ctx_skeleton2d_set_trigger(struct PonkoCtx *ctx,
                                                  uint64_t entity_bits,
                                                  size_t parameter,
                                                  bool value);

enum PonkoStatus ponko_ctx_skeleton2d_next_event(struct PonkoCtx *ctx,
                                                 uint64_t entity_bits,
                                                 char *buffer,
                                                 size_t capacity,
                                                 bool *out_found);

enum PonkoStatus ponko_ctx_skeleton2d_ik_index(struct PonkoCtx *ctx,
                                               uint64_t entity_bits,
                                               const char *name,
                                               size_t *out_ik);

enum PonkoStatus ponko_ctx_skeleton2d_set_ik_target(struct PonkoCtx *ctx,
                                                    uint64_t entity_bits,
                                                    size_t ik,
                                                    struct PonkoVec2 target);

enum PonkoStatus ponko_ctx_skeleton2d_clear_ik_target(struct PonkoCtx *ctx,
                                                      uint64_t entity_bits,
                                                      size_t ik);

enum PonkoStatus ponko_ctx_skeleton2d_set_ik_mix(struct PonkoCtx *ctx,
                                                 uint64_t entity_bits,
                                                 size_t ik,
                                                 float mix);

enum PonkoStatus ponko_ctx_skeleton2d_look_at(struct PonkoCtx *ctx,
                                              uint64_t entity_bits,
                                              size_t bone,
                                              struct PonkoVec2 target,
                                              float mix);

enum PonkoStatus ponko_ctx_skeleton2d_two_bone_ik(struct PonkoCtx *ctx,
                                                  uint64_t entity_bits,
                                                  size_t upper,
                                                  size_t lower,
                                                  struct PonkoVec2 target,
                                                  bool bend_positive,
                                                  float mix);

enum PonkoStatus ponko_ctx_skeleton2d_bone_index(struct PonkoCtx *ctx,
                                                 uint64_t entity_bits,
                                                 const char *name,
                                                 size_t *out_bone);

enum PonkoStatus ponko_ctx_skeleton2d_bone(struct PonkoCtx *ctx,
                                           uint64_t entity_bits,
                                           size_t bone,
                                           struct PonkoVec2 *out_position,
                                           float *out_rotation);

enum PonkoStatus ponko_ctx_skeleton2d_slot_index(struct PonkoCtx *ctx,
                                                 uint64_t entity_bits,
                                                 const char *name,
                                                 size_t *out_slot);

enum PonkoStatus ponko_ctx_skeleton2d_set_attachment(struct PonkoCtx *ctx,
                                                     uint64_t entity_bits,
                                                     size_t slot,
                                                     const char *name);

enum PonkoStatus ponko_ctx_skeleton2d_attachment(struct PonkoCtx *ctx,
                                                 uint64_t entity_bits,
                                                 size_t slot,
                                                 char *buffer,
                                                 size_t capacity,
                                                 size_t *out_len);

enum PonkoStatus ponko_ctx_skeleton2d_reset_attachment(struct PonkoCtx *ctx,
                                                       uint64_t entity_bits,
                                                       size_t slot);

enum PonkoStatus ponko_ctx_skeleton2d_set_skin(struct PonkoCtx *ctx,
                                               uint64_t entity_bits,
                                               const char *name);

enum PonkoStatus ponko_ctx_skeleton2d_set_look(struct PonkoCtx *ctx,
                                               uint64_t entity_bits,
                                               float z,
                                               struct PonkoColor color,
                                               float speed);

enum PonkoStatus ponko_ctx_skeleton2d_set_manual(struct PonkoCtx *ctx,
                                                 uint64_t entity_bits,
                                                 bool manual);

enum PonkoStatus ponko_ctx_skeleton2d_update(struct PonkoCtx *ctx, uint64_t entity_bits, float dt);

enum PonkoStatus ponko_mesh_data_set_skin(struct PonkoMeshData *data,
                                          const uint16_t *joints,
                                          const float *weights,
                                          size_t count);

enum PonkoStatus ponko_ctx_set_skeleton(struct PonkoCtx *ctx,
                                        uint64_t entity_bits,
                                        const struct PonkoJoint *joints,
                                        size_t count);

enum PonkoStatus ponko_ctx_load_skeleton(struct PonkoCtx *ctx,
                                         uint64_t entity_bits,
                                         const char *path);

enum PonkoStatus ponko_ctx_create_skinned_mesh(struct PonkoCtx *ctx,
                                               const struct PonkoMeshData *data,
                                               struct PonkoMaterial material,
                                               uint64_t skeleton_entity,
                                               struct PonkoMesh *out_mesh);

enum PonkoStatus ponko_ctx_joint_count(struct PonkoCtx *ctx,
                                       uint64_t entity_bits,
                                       size_t *out_count);

enum PonkoStatus ponko_ctx_find_joint(struct PonkoCtx *ctx,
                                      uint64_t entity_bits,
                                      const char *name,
                                      bool *out_found,
                                      size_t *out_joint);

enum PonkoStatus ponko_ctx_joint_pose(struct PonkoCtx *ctx,
                                      uint64_t entity_bits,
                                      size_t joint,
                                      struct PonkoTransform3D *out_pose);

enum PonkoStatus ponko_ctx_set_joint_pose(struct PonkoCtx *ctx,
                                          uint64_t entity_bits,
                                          size_t joint,
                                          const struct PonkoTransform3D *pose);

enum PonkoStatus ponko_ctx_reset_pose(struct PonkoCtx *ctx, uint64_t entity_bits);

enum PonkoStatus ponko_ctx_joint_matrix(struct PonkoCtx *ctx,
                                        uint64_t entity_bits,
                                        size_t joint,
                                        float *out_matrix);

enum PonkoStatus ponko_ctx_set_skin_of(struct PonkoCtx *ctx,
                                       uint64_t entity_bits,
                                       uint64_t skeleton_entity);

enum PonkoStatus ponko_ctx_attach_to_joint(struct PonkoCtx *ctx,
                                           uint64_t entity_bits,
                                           uint64_t skeleton_entity,
                                           size_t joint,
                                           const struct PonkoTransform3D *offset);

enum PonkoStatus ponko_ctx_load_clip(struct PonkoCtx *ctx,
                                     const char *path,
                                     const char *name,
                                     struct PonkoClip **out_clip);

void ponko_clip_free(struct PonkoClip *clip);

float ponko_clip_duration(const struct PonkoClip *clip);

enum PonkoStatus ponko_ctx_sample_clip(struct PonkoCtx *ctx,
                                       const struct PonkoClip *clip,
                                       uint64_t entity_bits,
                                       float time,
                                       float weight);

enum PonkoStatus ponko_ctx_two_bone_ik(struct PonkoCtx *ctx,
                                       uint64_t entity_bits,
                                       size_t upper,
                                       size_t middle,
                                       size_t end,
                                       struct PonkoVec3 target,
                                       struct PonkoVec3 pole,
                                       float weight);

enum PonkoStatus ponko_ctx_look_at_joint(struct PonkoCtx *ctx,
                                         uint64_t entity_bits,
                                         size_t joint,
                                         struct PonkoVec3 axis,
                                         struct PonkoVec3 target,
                                         float max_angle,
                                         float weight);

enum PonkoStatus ponko_ctx_draw_skeleton(struct PonkoCtx *ctx,
                                         uint64_t entity_bits,
                                         struct PonkoColor color);

enum PonkoStatus ponko_ctx_sounds_started(struct PonkoCtx *ctx, size_t *out_count);

enum PonkoStatus ponko_ctx_sound_started(struct PonkoCtx *ctx,
                                         size_t index,
                                         struct PonkoSoundStarted *out_started);

struct PonkoSpriteMaterialDesc ponko_sprite_material_desc_default(void);

enum PonkoStatus ponko_ctx_load_sprite_shader(struct PonkoCtx *ctx,
                                              const char *path,
                                              struct PonkoSpriteShader *out_shader);

enum PonkoStatus ponko_ctx_create_sprite_shader(struct PonkoCtx *ctx,
                                                const char *name,
                                                const char *source,
                                                struct PonkoSpriteShader *out_shader);

enum PonkoStatus ponko_ctx_create_sprite_material(struct PonkoCtx *ctx,
                                                  const struct PonkoSpriteMaterialDesc *desc,
                                                  struct PonkoSpriteMaterial *out_material);

enum PonkoStatus ponko_ctx_update_sprite_material(struct PonkoCtx *ctx,
                                                  struct PonkoSpriteMaterial material,
                                                  const struct PonkoSpriteMaterialDesc *desc);

enum PonkoStatus ponko_ctx_sprite_material_desc(struct PonkoCtx *ctx,
                                                struct PonkoSpriteMaterial material,
                                                struct PonkoSpriteMaterialDesc *out_desc,
                                                bool *out_found);

enum PonkoStatus ponko_ctx_draw_styled(struct PonkoCtx *ctx,
                                       struct PonkoTexture texture,
                                       struct PonkoVec2 pos,
                                       const struct PonkoDrawParams *params,
                                       uint32_t blend,
                                       struct PonkoSpriteMaterial material);

enum PonkoStatus ponko_blend_mode_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_sprite_material_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_game_lockstep_steam_host(struct PonkoGame *game,
                                                uint32_t app_id,
                                                uint32_t players,
                                                uint32_t input_delay,
                                                uint32_t checksum_every);

enum PonkoStatus ponko_game_lockstep_steam_join(struct PonkoGame *game,
                                                uint32_t app_id,
                                                uint64_t lobby);

enum PonkoStatus ponko_steam_lobby_from_args(uint64_t *out_lobby);

enum PonkoStatus ponko_steam_wait_for_invite(uint32_t app_id,
                                             uint32_t timeout_ms,
                                             uint64_t *out_lobby);

enum PonkoStatus ponko_ctx_steam_lobby(struct PonkoCtx *ctx, uint64_t *out_lobby);

enum PonkoStatus ponko_ctx_steam_invite(struct PonkoCtx *ctx);

enum PonkoStatus ponko_steam_lobby_create(uint32_t app_id,
                                          uint32_t visibility,
                                          uint32_t max_members,
                                          struct PonkoSteamLobby **output);

enum PonkoStatus ponko_steam_lobby_join(uint32_t app_id,
                                        uint64_t id,
                                        struct PonkoSteamLobby **output);

void ponko_steam_lobby_free(struct PonkoSteamLobby *lobby);

enum PonkoStatus ponko_steam_lobby_poll(struct PonkoSteamLobby *lobby);

enum PonkoStatus ponko_steam_lobby_leave(struct PonkoSteamLobby *lobby);

enum PonkoStatus ponko_steam_lobby_id(struct PonkoSteamLobby *lobby, uint64_t *output);

enum PonkoStatus ponko_steam_lobby_invite(struct PonkoSteamLobby *lobby);

enum PonkoStatus ponko_steam_lobby_set_metadata(struct PonkoSteamLobby *lobby,
                                                const char *key,
                                                const char *value);

enum PonkoStatus ponko_steam_lobby_metadata(struct PonkoSteamLobby *lobby,
                                            const char *key,
                                            char *buffer,
                                            size_t capacity,
                                            size_t *needed);

enum PonkoStatus ponko_steam_lobby_members(struct PonkoSteamLobby *lobby,
                                           uint64_t *members,
                                           size_t capacity,
                                           size_t *needed);

enum PonkoStatus ponko_net_messages_steam_lobby_id(struct PonkoNetMessages *endpoint,
                                                   uint64_t *output);

enum PonkoStatus ponko_net_messages_steam_lobby_invite(struct PonkoNetMessages *endpoint);

enum PonkoStatus ponko_net_messages_steam_lobby_set_metadata(struct PonkoNetMessages *endpoint,
                                                             const char *key,
                                                             const char *value);

enum PonkoStatus ponko_net_messages_steam_lobby_metadata(struct PonkoNetMessages *endpoint,
                                                         const char *key,
                                                         char *buffer,
                                                         size_t capacity,
                                                         size_t *needed);

enum PonkoStatus ponko_net_messages_steam_lobby_members(struct PonkoNetMessages *endpoint,
                                                        uint64_t *members,
                                                        size_t capacity,
                                                        size_t *needed);

struct PonkoStencil ponko_stencil_default(void);

struct PonkoStencil ponko_stencil_mark(uint8_t reference);

struct PonkoStencil ponko_stencil_inside(uint8_t reference);

struct PonkoStencil ponko_stencil_outside(uint8_t reference);

struct PonkoStencil ponko_stencil_once(uint8_t reference);

enum PonkoStatus ponko_ctx_create_material_with_stencil(struct PonkoCtx *ctx,
                                                        const struct PonkoMaterialDesc *desc,
                                                        const struct PonkoStencil *stencil,
                                                        struct PonkoMaterial *out_material);

enum PonkoStatus ponko_ctx_update_material_with_stencil(struct PonkoCtx *ctx,
                                                        struct PonkoMaterial material,
                                                        const struct PonkoMaterialDesc *desc,
                                                        const struct PonkoStencil *stencil);

enum PonkoStatus ponko_game_app_id(struct PonkoGame *game, const char *app_id);

enum PonkoStatus ponko_game_data_dir(struct PonkoGame *game, const char *dir);

enum PonkoStatus ponko_ctx_storage_write(struct PonkoCtx *ctx,
                                         const char *name,
                                         const uint8_t *bytes,
                                         size_t len);

enum PonkoStatus ponko_ctx_storage_read(struct PonkoCtx *ctx,
                                        const char *name,
                                        uint8_t *buffer,
                                        size_t capacity,
                                        size_t *out_len);

enum PonkoStatus ponko_ctx_storage_read_backup(struct PonkoCtx *ctx,
                                               const char *name,
                                               uint8_t *buffer,
                                               size_t capacity,
                                               size_t *out_len);

enum PonkoStatus ponko_ctx_storage_exists(struct PonkoCtx *ctx, const char *name, bool *out_exists);

enum PonkoStatus ponko_ctx_storage_list(struct PonkoCtx *ctx,
                                        char *buffer,
                                        size_t capacity,
                                        size_t *out_len);

enum PonkoStatus ponko_ctx_storage_delete(struct PonkoCtx *ctx, const char *name);

enum PonkoStatus ponko_ctx_storage_dir(struct PonkoCtx *ctx,
                                       char *buffer,
                                       size_t capacity,
                                       size_t *out_len);

enum PonkoStatus ponko_ctx_save_world(struct PonkoCtx *ctx, const char *slot);

enum PonkoStatus ponko_ctx_load_world(struct PonkoCtx *ctx, const char *slot);

struct PonkoCellStreamer *ponko_cells_new(float cell_size, float load_radius, float keep_radius);

void ponko_cells_free(struct PonkoCellStreamer *cells);

enum PonkoStatus ponko_cells_update(struct PonkoCellStreamer *cells,
                                    struct PonkoVec3 at,
                                    size_t max_new);

enum PonkoStatus ponko_cells_to_load(struct PonkoCellStreamer *cells,
                                     const struct PonkoCell **out_cells,
                                     size_t *out_len);

enum PonkoStatus ponko_cells_to_drop(struct PonkoCellStreamer *cells,
                                     const struct PonkoCell **out_cells,
                                     size_t *out_len);

enum PonkoStatus ponko_cells_is_loaded(struct PonkoCellStreamer *cells,
                                       struct PonkoCell cell,
                                       bool *out_loaded,
                                       size_t *out_count);

enum PonkoStatus ponko_cells_clear(struct PonkoCellStreamer *cells);

struct PonkoTerrainTileData *ponko_terrain_tile_data_new(const float *heights,
                                                         size_t count,
                                                         float size);

void ponko_terrain_tile_data_free(struct PonkoTerrainTileData *data);

enum PonkoStatus ponko_ctx_upload_terrain_tile(struct PonkoCtx *ctx,
                                               const struct PonkoTerrainTileData *data,
                                               struct PonkoMaterial material,
                                               struct PonkoTerrainTile *out_tile);

enum PonkoStatus ponko_ctx_free_mesh(struct PonkoCtx *ctx, struct PonkoMesh mesh);

enum PonkoStatus ponko_ctx_is_mesh_live(struct PonkoCtx *ctx,
                                        struct PonkoMesh mesh,
                                        bool *out_live);

enum PonkoStatus ponko_ctx_free_terrain_tile(struct PonkoCtx *ctx, struct PonkoTerrainTile tile);

struct PonkoFarTerrain *ponko_far_terrain_new(const float *heights,
                                              size_t count,
                                              size_t columns,
                                              struct PonkoVec2 corner,
                                              float spacing,
                                              float cell_size,
                                              struct PonkoMaterial material);

void ponko_far_terrain_free(struct PonkoFarTerrain *far);

enum PonkoStatus ponko_far_terrain_cut(struct PonkoFarTerrain *far,
                                       struct PonkoCell cell,
                                       bool cut);

enum PonkoStatus ponko_far_terrain_is_cut(struct PonkoFarTerrain *far,
                                          struct PonkoCell cell,
                                          bool *out_cut,
                                          size_t *out_meshes);

enum PonkoStatus ponko_far_terrain_update(struct PonkoFarTerrain *far, struct PonkoCtx *ctx);

enum PonkoStatus ponko_far_terrain_clear(struct PonkoFarTerrain *far, struct PonkoCtx *ctx);

struct PonkoTrail3D ponko_trail3d_new(float lifetime, float width);

enum PonkoStatus ponko_ctx_set_trail3d(struct PonkoCtx *ctx,
                                       uint64_t entity_bits,
                                       const struct PonkoTrail3D *trail);

enum PonkoStatus ponko_ctx_trail3d_point_count(struct PonkoCtx *ctx,
                                               uint64_t entity_bits,
                                               uint32_t *out_count);

enum PonkoStatus ponko_ctx_clear_trail3d(struct PonkoCtx *ctx, uint64_t entity_bits);

enum PonkoStatus ponko_ctx_set_trail3d_emitting(struct PonkoCtx *ctx,
                                                uint64_t entity_bits,
                                                bool emitting);

enum PonkoStatus ponko_ctx_remove_trail3d(struct PonkoCtx *ctx, uint64_t entity_bits);

struct PonkoLine3D ponko_line3d_new(float width, struct PonkoColor color);

enum PonkoStatus ponko_ctx_set_line3d(struct PonkoCtx *ctx,
                                      uint64_t entity_bits,
                                      const struct PonkoLine3D *line);

enum PonkoStatus ponko_ctx_remove_line3d(struct PonkoCtx *ctx, uint64_t entity_bits);

struct PonkoMeshData *ponko_mesh_data_decal(const struct PonkoMeshData *data,
                                            const float *projector,
                                            float lift);

enum PonkoStatus ponko_ctx_load_mesh_data(struct PonkoCtx *ctx,
                                          const char *path,
                                          struct PonkoMeshData **out_data);

struct PonkoText3D ponko_text3d_default(void);

enum PonkoStatus ponko_ctx_set_text3d(struct PonkoCtx *ctx,
                                      uint64_t entity,
                                      const char *string,
                                      const struct PonkoText3D *value);

enum PonkoStatus ponko_ctx_text3d(struct PonkoCtx *ctx,
                                  uint64_t entity,
                                  struct PonkoText3D *output);

enum PonkoStatus ponko_ctx_text3d_text(struct PonkoCtx *ctx,
                                       uint64_t entity,
                                       char *buffer,
                                       size_t capacity,
                                       size_t *length);

enum PonkoStatus ponko_ctx_remove_text3d(struct PonkoCtx *ctx, uint64_t entity);

struct PonkoVec3 ponko_sun_direction_at(float hour);

struct PonkoQuat ponko_quat_identity(void);

struct PonkoQuat ponko_quat_from_axis_angle(struct PonkoVec3 axis, float angle);

struct PonkoQuat ponko_quat_mul(struct PonkoQuat a, struct PonkoQuat b);

struct PonkoVec3 ponko_quat_rotate(struct PonkoQuat q, struct PonkoVec3 v);

struct PonkoTransform3D ponko_transform3d_at(struct PonkoVec3 position);

struct PonkoTransform3D ponko_transform3d_looking_at(struct PonkoTransform3D t,
                                                     struct PonkoVec3 target);

struct PonkoVec3 ponko_transform3d_forward(struct PonkoTransform3D t);

struct PonkoModel ponko_model(struct PonkoMesh mesh);

struct PonkoMaterialDesc ponko_material_desc(struct PonkoColor color);

struct PonkoTerrainMaterialDesc ponko_terrain_material_desc(struct PonkoTextureArray layers,
                                                            struct PonkoTextureArray weights,
                                                            struct PonkoVec2 corner,
                                                            struct PonkoVec2 size);

enum PonkoStatus ponko_ctx_create_texture_array(struct PonkoCtx *ctx,
                                                uint32_t width,
                                                uint32_t height,
                                                const uint8_t *rgba,
                                                size_t len,
                                                uint32_t kind,
                                                struct PonkoTextureArray *out_array);

enum PonkoStatus ponko_ctx_load_texture_array(struct PonkoCtx *ctx,
                                              const char *const *paths,
                                              size_t count,
                                              uint32_t kind,
                                              struct PonkoTextureArray *out_array);

enum PonkoStatus ponko_ctx_create_terrain_material(struct PonkoCtx *ctx,
                                                   const struct PonkoTerrainMaterialDesc *desc,
                                                   struct PonkoMaterial *out_material);

enum PonkoStatus ponko_transform3d_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_model_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_terrain_tile_component(struct PonkoWorld *world, uint32_t *out_id);

struct PonkoMeshData *ponko_mesh_data_new(const struct PonkoVec3 *positions,
                                          const struct PonkoVec3 *normals,
                                          const struct PonkoVec2 *uvs,
                                          size_t vertex_count,
                                          const uint32_t *indices,
                                          size_t index_count);

struct PonkoMeshData *ponko_mesh_data_cube(float size);

struct PonkoMeshData *ponko_mesh_data_plane(float size);

struct PonkoMeshData *ponko_mesh_data_sphere(float radius, uint32_t segments);

struct PonkoMeshData *ponko_mesh_data_heightfield(const float *heights,
                                                  size_t count,
                                                  float size,
                                                  uint32_t step);

enum PonkoStatus ponko_mesh_data_set_colors(struct PonkoMeshData *data,
                                            const struct PonkoColor *colors,
                                            size_t count);

void ponko_mesh_data_free(struct PonkoMeshData *data);

enum PonkoStatus ponko_mesh_data_view(const struct PonkoMeshData *data,
                                      struct PonkoMeshView *out_view);

enum PonkoStatus ponko_ctx_create_material(struct PonkoCtx *ctx,
                                           const struct PonkoMaterialDesc *desc,
                                           struct PonkoMaterial *out_material);

enum PonkoStatus ponko_ctx_update_material(struct PonkoCtx *ctx,
                                           struct PonkoMaterial material,
                                           const struct PonkoMaterialDesc *desc);

enum PonkoStatus ponko_ctx_loaded_materials(struct PonkoCtx *ctx,
                                            const char *name,
                                            struct PonkoMaterial *out_materials,
                                            struct PonkoMaterialDesc *out_descs,
                                            size_t capacity,
                                            size_t *out_count);

enum PonkoStatus ponko_ctx_load_shader(struct PonkoCtx *ctx,
                                       const char *path,
                                       struct PonkoShader *out_shader);

enum PonkoStatus ponko_ctx_create_shader(struct PonkoCtx *ctx,
                                         const char *name,
                                         const char *source,
                                         struct PonkoShader *out_shader);

enum PonkoStatus ponko_ctx_load_compute(struct PonkoCtx *ctx,
                                        const char *path,
                                        struct PonkoCompute *out_compute);

enum PonkoStatus ponko_ctx_create_compute(struct PonkoCtx *ctx,
                                          const char *name,
                                          const char *source,
                                          struct PonkoCompute *out_compute);

enum PonkoStatus ponko_ctx_set_compute_passes(struct PonkoCtx *ctx,
                                              const struct PonkoComputePass *passes,
                                              size_t count);

enum PonkoStatus ponko_ctx_compute_passes(struct PonkoCtx *ctx,
                                          struct PonkoComputePass *out_passes,
                                          size_t capacity,
                                          size_t *out_count);

enum PonkoStatus ponko_ctx_load_effect(struct PonkoCtx *ctx,
                                       const char *path,
                                       struct PonkoEffect *out_effect);

enum PonkoStatus ponko_ctx_create_effect(struct PonkoCtx *ctx,
                                         const char *name,
                                         const char *source,
                                         struct PonkoEffect *out_effect);

enum PonkoStatus ponko_ctx_set_effects(struct PonkoCtx *ctx,
                                       const struct PonkoScreenEffect *effects,
                                       size_t count);

enum PonkoStatus ponko_ctx_effects(struct PonkoCtx *ctx,
                                   struct PonkoScreenEffect *out_effects,
                                   size_t capacity,
                                   size_t *out_count);

enum PonkoStatus ponko_ctx_default_material(struct PonkoCtx *ctx,
                                            struct PonkoMaterial *out_material);

enum PonkoStatus ponko_ctx_create_mesh(struct PonkoCtx *ctx,
                                       const struct PonkoMeshData *data,
                                       struct PonkoMaterial material,
                                       struct PonkoMesh *out_mesh);

enum PonkoStatus ponko_ctx_create_plain_mesh(struct PonkoCtx *ctx,
                                             const struct PonkoMeshData *data,
                                             struct PonkoMesh *out_mesh);

enum PonkoStatus ponko_ctx_create_terrain_tile(struct PonkoCtx *ctx,
                                               const float *heights,
                                               size_t count,
                                               float size,
                                               struct PonkoMaterial material,
                                               struct PonkoTerrainTile *out_tile);

enum PonkoStatus ponko_ctx_load_mesh(struct PonkoCtx *ctx,
                                     const char *path,
                                     struct PonkoMesh *out_mesh);

enum PonkoStatus ponko_ctx_models_drawn(struct PonkoCtx *ctx, size_t *out_count);

enum PonkoStatus ponko_ctx_create_render_target(struct PonkoCtx *ctx,
                                                uint32_t width,
                                                uint32_t height,
                                                struct PonkoTexture *out_texture);

enum PonkoStatus ponko_ctx_reflection_camera(struct PonkoCtx *ctx,
                                             const struct PonkoCamera3D *camera,
                                             struct PonkoTexture target,
                                             float level,
                                             struct PonkoTargetCamera *out_camera);

enum PonkoStatus ponko_ctx_set_target_cameras(struct PonkoCtx *ctx,
                                              const struct PonkoTargetCamera *cameras,
                                              size_t count);

enum PonkoStatus ponko_ctx_target_cameras(struct PonkoCtx *ctx,
                                          struct PonkoTargetCamera *out_cameras,
                                          size_t capacity,
                                          size_t *out_count);

enum PonkoStatus ponko_ctx_camera3d(struct PonkoCtx *ctx, struct PonkoCamera3D *out_camera);

enum PonkoStatus ponko_ctx_set_camera3d(struct PonkoCtx *ctx, const struct PonkoCamera3D *camera);

enum PonkoStatus ponko_ctx_set_occlusion_culling(struct PonkoCtx *ctx, bool on);

enum PonkoStatus ponko_ctx_occlusion_culling(struct PonkoCtx *ctx, bool *out_on);

enum PonkoStatus ponko_ctx_camera3d_look_at(struct PonkoCtx *ctx, struct PonkoVec3 target);

enum PonkoStatus ponko_ctx_screen_ray(struct PonkoCtx *ctx,
                                      struct PonkoVec2 screen,
                                      struct PonkoVec3 *out_origin,
                                      struct PonkoVec3 *out_direction);

enum PonkoStatus ponko_ctx_lighting(struct PonkoCtx *ctx, struct PonkoLighting *out_lighting);

enum PonkoStatus ponko_ctx_set_lighting(struct PonkoCtx *ctx, const struct PonkoLighting *lighting);

enum PonkoStatus ponko_ctx_sky(struct PonkoCtx *ctx, struct PonkoSky *out_sky);

enum PonkoStatus ponko_ctx_set_sky(struct PonkoCtx *ctx, const struct PonkoSky *sky);

enum PonkoStatus ponko_ctx_post(struct PonkoCtx *ctx, struct PonkoPost *out_post);

enum PonkoStatus ponko_ctx_set_post(struct PonkoCtx *ctx, const struct PonkoPost *post);

enum PonkoStatus ponko_graphics_quality_preset(uint32_t level,
                                               struct PonkoGraphicsQuality *out_quality);

enum PonkoStatus ponko_graphics_quality_level(const struct PonkoGraphicsQuality *quality,
                                              uint32_t *out_level);

enum PonkoStatus ponko_ctx_graphics_quality(struct PonkoCtx *ctx,
                                            struct PonkoGraphicsQuality *out_quality);

enum PonkoStatus ponko_ctx_set_graphics_quality(struct PonkoCtx *ctx,
                                                const struct PonkoGraphicsQuality *quality);

enum PonkoStatus ponko_ctx_reset_temporal_history(struct PonkoCtx *ctx);

struct PonkoHeightfield *ponko_heightfield_new(const float *heights, size_t count, float size);

void ponko_heightfield_free(struct PonkoHeightfield *ground);

enum PonkoStatus ponko_heightfield_at(const struct PonkoHeightfield *ground,
                                      float x,
                                      float z,
                                      bool *out_found,
                                      float *out_height,
                                      struct PonkoVec3 *out_normal);

enum PonkoStatus ponko_ctx_set_shadow_mesh(struct PonkoCtx *ctx,
                                           uint64_t entity,
                                           struct PonkoMesh mesh,
                                           bool casts);

enum PonkoStatus ponko_ctx_set_model_material(struct PonkoCtx *ctx,
                                              uint64_t entity,
                                              struct PonkoMaterial material,
                                              bool enabled);

enum PonkoStatus ponko_ctx_model_material(struct PonkoCtx *ctx,
                                          uint64_t entity,
                                          struct PonkoMaterial *out_material,
                                          bool *out_has);

enum PonkoStatus ponko_ctx_set_model_part_materials(struct PonkoCtx *ctx,
                                                    uint64_t entity,
                                                    const uint32_t *parts,
                                                    const struct PonkoMaterial *materials,
                                                    size_t count);

enum PonkoStatus ponko_ctx_model_part_materials(struct PonkoCtx *ctx,
                                                uint64_t entity,
                                                uint32_t *out_parts,
                                                struct PonkoMaterial *out_materials,
                                                size_t capacity,
                                                size_t *out_count);

enum PonkoStatus ponko_ctx_set_mesh_shadows(struct PonkoCtx *ctx,
                                            struct PonkoMesh mesh,
                                            bool casts);

enum PonkoStatus ponko_ctx_mesh_shadows(struct PonkoCtx *ctx,
                                        struct PonkoMesh mesh,
                                        bool *out_casts);

enum PonkoStatus ponko_point_light3d_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_spot_light3d_component(struct PonkoWorld *world, uint32_t *out_id);

struct PonkoReflectionProbe3D ponko_reflection_probe3d_default(void);

enum PonkoStatus ponko_reflection_probe3d_component(struct PonkoWorld *world, uint32_t *out_id);

struct PonkoDecal3D ponko_decal3d_default(void);

enum PonkoStatus ponko_decal3d_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_receives_decals_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_world_set_shadow_only(struct PonkoWorld *world,
                                             uint64_t entity,
                                             bool shadow_only);

enum PonkoStatus ponko_world_is_shadow_only(struct PonkoWorld *world,
                                            uint64_t entity,
                                            bool *out_shadow_only);

enum PonkoStatus ponko_ctx_set_decal_textures(struct PonkoCtx *ctx, struct PonkoTextureArray array);

enum PonkoStatus ponko_ctx_clear_decal_textures(struct PonkoCtx *ctx);

enum PonkoStatus ponko_ctx_supports_decals(struct PonkoCtx *ctx, bool *out_supported);

enum PonkoStatus ponko_ctx_warm_render_pipelines(struct PonkoCtx *ctx);

enum PonkoStatus ponko_ctx_bake_reflection_probe(struct PonkoCtx *ctx, uint64_t entity);

enum PonkoStatus ponko_ctx_load_reflection_probe(struct PonkoCtx *ctx,
                                                 uint64_t entity,
                                                 const char *name);

struct PonkoLodRenderer ponko_lod_renderer(struct PonkoMesh mesh);

enum PonkoStatus ponko_ctx_set_lod_group(struct PonkoCtx *ctx,
                                         uint64_t entity,
                                         float size,
                                         struct PonkoVec3 center,
                                         const struct PonkoLodLevel *levels,
                                         size_t count);

enum PonkoStatus ponko_ctx_lod_pick(struct PonkoCtx *ctx,
                                    uint64_t entity,
                                    struct PonkoLodPick *out_pick);

enum PonkoStatus ponko_ctx_lod_settings(struct PonkoCtx *ctx,
                                        struct PonkoLodSettings *out_settings);

enum PonkoStatus ponko_ctx_set_lod_settings(struct PonkoCtx *ctx, struct PonkoLodSettings settings);

float ponko_lod_screen_height_at(float size, float distance, float fov_y);

float ponko_lod_distance_at(float size, float height, float fov_y);

enum PonkoStatus ponko_ctx_create_gpu_buffer(struct PonkoCtx *ctx,
                                             uint32_t len,
                                             const float (*data)[4],
                                             size_t count,
                                             struct PonkoGpuBuffer *output);

enum PonkoStatus ponko_ctx_write_gpu_buffer(struct PonkoCtx *ctx,
                                            struct PonkoGpuBuffer buffer,
                                            uint32_t offset,
                                            const float (*data)[4],
                                            size_t count);

enum PonkoStatus ponko_ctx_free_gpu_buffer(struct PonkoCtx *ctx, struct PonkoGpuBuffer buffer);

enum PonkoStatus ponko_ctx_set_mesh_gpu_bounds(struct PonkoCtx *ctx,
                                               struct PonkoMesh mesh,
                                               struct PonkoVec3 center,
                                               float radius);

enum PonkoStatus ponko_ctx_set_mesh_gpu_buffer(struct PonkoCtx *ctx,
                                               struct PonkoMesh mesh,
                                               struct PonkoGpuBuffer buffer,
                                               uint32_t count);

enum PonkoStatus ponko_ctx_set_instance_batch_culling(struct PonkoCtx *ctx,
                                                      struct PonkoInstanceBatch batch,
                                                      uint32_t culling);

enum PonkoStatus ponko_ctx_create_instance_batch(struct PonkoCtx *ctx,
                                                 struct PonkoMesh mesh,
                                                 const float (*matrices)[16],
                                                 size_t count,
                                                 struct PonkoInstanceBatchOptions options,
                                                 struct PonkoInstanceBatch *output);

enum PonkoStatus ponko_ctx_update_instance_batch(struct PonkoCtx *ctx,
                                                 struct PonkoInstanceBatch batch,
                                                 const float (*matrices)[16],
                                                 size_t count);

enum PonkoStatus ponko_ctx_free_instance_batch(struct PonkoCtx *ctx,
                                               struct PonkoInstanceBatch batch);

enum PonkoStatus ponko_ctx_create_instance_batch_levels(struct PonkoCtx *ctx,
                                                        const struct PonkoInstanceLevel *levels,
                                                        size_t level_count,
                                                        const float (*matrices)[16],
                                                        size_t count,
                                                        const struct PonkoColor *colors,
                                                        size_t color_count,
                                                        struct PonkoInstanceBatchOptions options,
                                                        struct PonkoInstanceBatch *output);

enum PonkoStatus ponko_ctx_update_instance_batch_colors(struct PonkoCtx *ctx,
                                                        struct PonkoInstanceBatch batch,
                                                        const float (*matrices)[16],
                                                        size_t count,
                                                        const struct PonkoColor *colors,
                                                        size_t color_count);

enum PonkoStatus ponko_ctx_gpu_capabilities(const struct PonkoCtx *ctx,
                                            char *buffer,
                                            size_t capacity,
                                            size_t *out_len);

enum PonkoStatus ponko_ctx_gpu_tier(const struct PonkoCtx *ctx, uint32_t *out_tier);

enum PonkoStatus ponko_ctx_gpu_feature(const struct PonkoCtx *ctx,
                                       uint32_t feature_id,
                                       bool *out_available);

enum PonkoStatus ponko_gpu_tier_report(uint32_t tier_id,
                                       char *buffer,
                                       size_t capacity,
                                       size_t *out_len);

enum PonkoStatus ponko_gpu_tier_feature(uint32_t tier_id, uint32_t feature_id, bool *out_available);

enum PonkoStatus ponko_graphics_quality_for_tier(uint32_t tier_id,
                                                 struct PonkoGraphicsQuality *out_quality);

enum PonkoStatus ponko_ctx_recommended_graphics_quality(const struct PonkoCtx *ctx,
                                                        struct PonkoGraphicsQuality *out_quality);

enum PonkoStatus ponko_ctx_add_instance_cell(struct PonkoCtx *ctx,
                                             struct PonkoInstanceBatch batch,
                                             const float (*matrices)[16],
                                             size_t count,
                                             const struct PonkoColor *colors,
                                             size_t color_count,
                                             struct PonkoInstanceCell *output);

enum PonkoStatus ponko_ctx_replace_instance_cell(struct PonkoCtx *ctx,
                                                 struct PonkoInstanceCell cell,
                                                 const float (*matrices)[16],
                                                 size_t count,
                                                 const struct PonkoColor *colors,
                                                 size_t color_count);

enum PonkoStatus ponko_ctx_instance_cell_len(struct PonkoCtx *ctx,
                                             struct PonkoInstanceCell cell,
                                             size_t *output);

enum PonkoStatus ponko_ctx_free_instance_cell(struct PonkoCtx *ctx, struct PonkoInstanceCell cell);

struct PonkoCrowdInstance ponko_crowd_instance_at(struct PonkoVec3 position);

enum PonkoStatus ponko_ctx_bake_crowd_animation(struct PonkoCtx *ctx,
                                                uint64_t skeleton_entity,
                                                const struct PonkoCrowdClip *clips,
                                                size_t clip_count,
                                                float fps,
                                                struct PonkoCrowdAnimation **output);

void ponko_crowd_animation_free(struct PonkoCrowdAnimation *animation);

enum PonkoStatus ponko_crowd_animation_clip(const struct PonkoCrowdAnimation *animation,
                                            const char *name,
                                            uint16_t *out_index);

enum PonkoStatus ponko_ctx_create_crowd(struct PonkoCtx *ctx,
                                        const struct PonkoCrowdAnimation *animation,
                                        const struct PonkoCrowdLevel *levels,
                                        size_t level_count,
                                        const struct PonkoCrowdInstance *instances,
                                        size_t count,
                                        struct PonkoCrowdOptions options,
                                        struct PonkoCrowd *output);

enum PonkoStatus ponko_ctx_update_crowd(struct PonkoCtx *ctx,
                                        struct PonkoCrowd crowd,
                                        const struct PonkoCrowdInstance *instances,
                                        size_t count);

enum PonkoStatus ponko_ctx_crowd_len(struct PonkoCtx *ctx,
                                     struct PonkoCrowd crowd,
                                     size_t *out_count);

enum PonkoStatus ponko_ctx_free_crowd(struct PonkoCtx *ctx, struct PonkoCrowd crowd);

enum PonkoStatus ponko_sprite_sheet(struct PonkoTexture texture,
                                    uint32_t frame_width,
                                    uint32_t frame_height,
                                    struct PonkoSpriteSheet *out_sheet);

struct PonkoRect ponko_sprite_sheet_frame(struct PonkoSpriteSheet sheet, uint32_t index);

enum PonkoStatus ponko_animation_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_ctx_set_texture_filter(struct PonkoCtx *ctx,
                                              struct PonkoTexture texture,
                                              bool nearest);

enum PonkoStatus ponko_tilemap_add(struct PonkoWorld *world,
                                   uint64_t entity,
                                   struct PonkoSpriteSheet sheet,
                                   uint32_t width,
                                   uint32_t height);

enum PonkoStatus ponko_tilemap_get(struct PonkoWorld *world,
                                   uint64_t entity,
                                   struct PonkoTileMap *out_map);

enum PonkoStatus ponko_tilemap_update(struct PonkoWorld *world,
                                      uint64_t entity,
                                      const struct PonkoTileMap *map);

uint64_t ponko_entity_bits(struct PonkoEntity entity);

struct PonkoDrawParams ponko_draw_params_with_space(struct PonkoDrawParams params,
                                                    enum PonkoDrawSpace space);

struct PonkoDrawParams ponko_draw_params_default(void);

enum PonkoStatus ponko_ctx_ui_set_accessible_name(struct PonkoCtx *ctx,
                                                  uint64_t target,
                                                  uint64_t id,
                                                  const char *name);

enum PonkoStatus ponko_ctx_ui_accessible_name(struct PonkoCtx *ctx,
                                              uint64_t target,
                                              uint64_t id,
                                              char *buffer,
                                              size_t capacity,
                                              size_t *out_len);

enum PonkoStatus ponko_ctx_ui_set_accessible_description(struct PonkoCtx *ctx,
                                                         uint64_t target,
                                                         uint64_t id,
                                                         const char *description);

enum PonkoStatus ponko_ctx_ui_accessible_description(struct PonkoCtx *ctx,
                                                     uint64_t target,
                                                     uint64_t id,
                                                     char *buffer,
                                                     size_t capacity,
                                                     size_t *out_len);

enum PonkoStatus ponko_ctx_ui_set_live_region(struct PonkoCtx *ctx,
                                              uint64_t target,
                                              uint64_t id,
                                              uint32_t live);

enum PonkoStatus ponko_ctx_ui_live_region(struct PonkoCtx *ctx,
                                          uint64_t target,
                                          uint64_t id,
                                          enum PonkoUiLive *out_live);

enum PonkoStatus ponko_ctx_ui_access_action(struct PonkoCtx *ctx,
                                            uint64_t target,
                                            uint32_t kind,
                                            uint64_t id,
                                            float value,
                                            const char *text);

enum PonkoStatus ponko_ctx_ui_access_text(struct PonkoCtx *ctx,
                                          uint64_t target,
                                          char *buffer,
                                          size_t capacity,
                                          size_t *out_len);

struct PonkoUiAccess *ponko_ui_access_new(void);

void ponko_ui_access_free(struct PonkoUiAccess *access);

enum PonkoStatus ponko_ui_access_update(struct PonkoUiAccess *access,
                                        struct PonkoCtx *ctx,
                                        uint64_t target,
                                        struct PonkoUiAccessChanges *out_changes);

enum PonkoStatus ponko_ui_access_reset(struct PonkoUiAccess *access);

enum PonkoStatus ponko_ui_access_count(struct PonkoUiAccess *access, size_t *out_count);

enum PonkoStatus ponko_ui_access_node(struct PonkoUiAccess *access,
                                      size_t index,
                                      struct PonkoUiAccessNode *out_node);

enum PonkoStatus ponko_ui_access_find(struct PonkoUiAccess *access,
                                      uint64_t id,
                                      struct PonkoUiAccessNode *out_node);

enum PonkoStatus ponko_ui_access_child(struct PonkoUiAccess *access,
                                       uint64_t id,
                                       size_t index,
                                       uint64_t *out_child);

enum PonkoStatus ponko_ui_access_name(struct PonkoUiAccess *access,
                                      uint64_t id,
                                      char *buffer,
                                      size_t capacity,
                                      size_t *out_len);

enum PonkoStatus ponko_ui_access_description(struct PonkoUiAccess *access,
                                             uint64_t id,
                                             char *buffer,
                                             size_t capacity,
                                             size_t *out_len);

enum PonkoStatus ponko_ui_access_value_text(struct PonkoUiAccess *access,
                                            uint64_t id,
                                            char *buffer,
                                            size_t capacity,
                                            size_t *out_len);

enum PonkoStatus ponko_ui_access_focus(struct PonkoUiAccess *access, uint64_t *out_id);

enum PonkoStatus ponko_ui_access_changed(struct PonkoUiAccess *access,
                                         size_t index,
                                         uint64_t *out_id);

enum PonkoStatus ponko_ui_access_removed(struct PonkoUiAccess *access,
                                         size_t index,
                                         uint64_t *out_id);

enum PonkoStatus ponko_ui_access_state_ex(struct PonkoUiAccess *access,
                                          uint64_t id,
                                          struct PonkoUiAccessStateEx *out_state);

struct PonkoUiEase ponko_ui_ease(uint32_t name);

struct PonkoUiTransition ponko_ui_transition(float seconds);

enum PonkoStatus ponko_ui_transition_progress(const struct PonkoUiTransition *transition,
                                              float elapsed,
                                              float *out_progress);

struct PonkoUiScreenTransition ponko_ui_screen_fade(float seconds);

struct PonkoUiScreenTransition ponko_ui_screen_slide(struct PonkoVec2 offset, float seconds);

struct PonkoUiScreenTransition ponko_ui_screen_zoom(float scale, float seconds);

struct PonkoUiTween ponko_ui_tween_defaults(uint32_t property);

enum PonkoStatus ponko_ctx_ui_animate(struct PonkoCtx *ctx,
                                      uint64_t target,
                                      uint64_t id,
                                      const struct PonkoUiTween *tween);

enum PonkoStatus ponko_ctx_ui_play(struct PonkoCtx *ctx,
                                   uint64_t target,
                                   uint64_t id,
                                   const struct PonkoUiTween *tweens,
                                   size_t count,
                                   uint32_t repeat,
                                   uint32_t times);

enum PonkoStatus ponko_ctx_ui_stop_animations(struct PonkoCtx *ctx, uint64_t target, uint64_t id);

enum PonkoStatus ponko_ctx_ui_is_animating(struct PonkoCtx *ctx,
                                           uint64_t target,
                                           uint64_t id,
                                           bool *out_animating);

enum PonkoStatus ponko_ctx_ui_set_screen_transition(struct PonkoCtx *ctx,
                                                    uint64_t target,
                                                    uint64_t root,
                                                    const struct PonkoUiScreenTransition *enter,
                                                    const struct PonkoUiScreenTransition *exit);

enum PonkoStatus ponko_ctx_ui_screen_transition(struct PonkoCtx *ctx,
                                                uint64_t target,
                                                uint64_t root,
                                                struct PonkoUiScreenTransition *out_enter,
                                                struct PonkoUiScreenTransition *out_exit);

enum PonkoStatus ponko_ctx_ui_drawn(struct PonkoCtx *ctx,
                                    uint64_t target,
                                    uint64_t id,
                                    struct PonkoUiDrawn *out_drawn);

enum PonkoStatus ponko_ctx_ui_clock(struct PonkoCtx *ctx, uint64_t target, double *out_seconds);

enum PonkoStatus ponko_ctx_ui_set_reduced_motion(struct PonkoCtx *ctx, uint64_t target, bool on);

enum PonkoStatus ponko_ctx_ui_reduced_motion(struct PonkoCtx *ctx, uint64_t target, bool *out_on);

enum PonkoStatus ponko_ctx_ui_set_focus(struct PonkoCtx *ctx, uint64_t target, uint64_t id);

enum PonkoStatus ponko_ctx_ui_clear_focus(struct PonkoCtx *ctx, uint64_t target);

enum PonkoStatus ponko_ctx_ui_set_initial_focus(struct PonkoCtx *ctx,
                                                uint64_t target,
                                                uint64_t id,
                                                bool on);

enum PonkoStatus ponko_ctx_ui_set_closes_screen(struct PonkoCtx *ctx,
                                                uint64_t target,
                                                uint64_t id,
                                                bool on);

enum PonkoStatus ponko_ctx_ui_closes_screen(struct PonkoCtx *ctx,
                                            uint64_t target,
                                            uint64_t id,
                                            bool *out_on);

enum PonkoStatus ponko_ctx_ui_show_hud(struct PonkoCtx *ctx, uint64_t target, uint64_t root);

enum PonkoStatus ponko_ctx_ui_hide_hud(struct PonkoCtx *ctx, uint64_t target, uint64_t root);

enum PonkoStatus ponko_ctx_ui_is_hud(struct PonkoCtx *ctx,
                                     uint64_t target,
                                     uint64_t id,
                                     bool *out_hud);

enum PonkoStatus ponko_ctx_create_ui_panel(struct PonkoCtx *ctx,
                                           uint64_t entity_bits,
                                           uint32_t width,
                                           uint32_t height,
                                           struct PonkoVec2 size,
                                           struct PonkoTexture *out_texture);

enum PonkoStatus ponko_ctx_remove_ui_panel(struct PonkoCtx *ctx, uint64_t entity_bits);

enum PonkoStatus ponko_ctx_set_ui_panel_interactive(struct PonkoCtx *ctx,
                                                    uint64_t entity_bits,
                                                    bool interactive);

enum PonkoStatus ponko_ctx_ui_panel_pointer(struct PonkoCtx *ctx,
                                            uint64_t entity_bits,
                                            bool *out_over,
                                            struct PonkoVec2 *out_pixel);

enum PonkoStatus ponko_ctx_set_ui_pointer_ray(struct PonkoCtx *ctx,
                                              struct PonkoVec3 origin,
                                              struct PonkoVec3 direction);

enum PonkoStatus ponko_ctx_clear_ui_pointer_ray(struct PonkoCtx *ctx);

struct PonkoUiLayout ponko_ui_layout_default(void);

struct PonkoUiPaint ponko_ui_paint_default(void);

struct PonkoUiInput ponko_ui_input_default(void);

enum PonkoStatus ponko_ctx_ui_add_panel(struct PonkoCtx *ctx,
                                        uint64_t target,
                                        uint64_t parent,
                                        const struct PonkoUiLayout *layout,
                                        uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_add_label(struct PonkoCtx *ctx,
                                        uint64_t target,
                                        uint64_t parent,
                                        const char *text,
                                        const struct PonkoUiLayout *layout,
                                        uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_add_button(struct PonkoCtx *ctx,
                                         uint64_t target,
                                         uint64_t parent,
                                         const char *text,
                                         const struct PonkoUiLayout *layout,
                                         uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_add_image(struct PonkoCtx *ctx,
                                        uint64_t target,
                                        uint64_t parent,
                                        struct PonkoTexture texture,
                                        const struct PonkoUiLayout *layout,
                                        uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_add_scroll(struct PonkoCtx *ctx,
                                         uint64_t target,
                                         uint64_t parent,
                                         const struct PonkoUiLayout *layout,
                                         uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_add_text_input(struct PonkoCtx *ctx,
                                             uint64_t target,
                                             uint64_t parent,
                                             const char *text,
                                             const struct PonkoUiLayout *layout,
                                             uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_add_toggle(struct PonkoCtx *ctx,
                                         uint64_t target,
                                         uint64_t parent,
                                         const char *text,
                                         bool on,
                                         const struct PonkoUiLayout *layout,
                                         uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_add_slider(struct PonkoCtx *ctx,
                                         uint64_t target,
                                         uint64_t parent,
                                         float value,
                                         const struct PonkoUiLayout *layout,
                                         uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_remove(struct PonkoCtx *ctx, uint64_t target, uint64_t id);

enum PonkoStatus ponko_ctx_ui_set_layout(struct PonkoCtx *ctx,
                                         uint64_t target,
                                         uint64_t id,
                                         const struct PonkoUiLayout *layout);

enum PonkoStatus ponko_ctx_ui_set_grid_template_areas(struct PonkoCtx *ctx,
                                                      uint64_t target,
                                                      uint64_t id,
                                                      const char *const *rows,
                                                      size_t count);

enum PonkoStatus ponko_ctx_ui_set_grid_line_names(struct PonkoCtx *ctx,
                                                  uint64_t target,
                                                  uint64_t id,
                                                  uint32_t axis,
                                                  const char *const *names,
                                                  size_t count);

enum PonkoStatus ponko_ctx_ui_set_grid_area(struct PonkoCtx *ctx,
                                            uint64_t target,
                                            uint64_t id,
                                            const char *name);

enum PonkoStatus ponko_ctx_ui_set_grid_named_line(struct PonkoCtx *ctx,
                                                  uint64_t target,
                                                  uint64_t id,
                                                  uint32_t axis,
                                                  bool end,
                                                  const char *name,
                                                  int32_t occurrence);

enum PonkoStatus ponko_ctx_ui_set_grid_named_span(struct PonkoCtx *ctx,
                                                  uint64_t target,
                                                  uint64_t id,
                                                  uint32_t axis,
                                                  bool end,
                                                  const char *name,
                                                  uint32_t count);

enum PonkoStatus ponko_ctx_ui_set_paint(struct PonkoCtx *ctx,
                                        uint64_t target,
                                        uint64_t id,
                                        const struct PonkoUiPaint *paint);

enum PonkoStatus ponko_ctx_ui_set_text(struct PonkoCtx *ctx,
                                       uint64_t target,
                                       uint64_t id,
                                       const char *text);

enum PonkoStatus ponko_ctx_ui_text(struct PonkoCtx *ctx,
                                   uint64_t target,
                                   uint64_t id,
                                   char *buffer,
                                   size_t capacity,
                                   size_t *out_len);

enum PonkoStatus ponko_ctx_ui_set_enabled(struct PonkoCtx *ctx,
                                          uint64_t target,
                                          uint64_t id,
                                          bool enabled);

enum PonkoStatus ponko_ctx_ui_set_value(struct PonkoCtx *ctx,
                                        uint64_t target,
                                        uint64_t id,
                                        float value);

enum PonkoStatus ponko_ctx_ui_value(struct PonkoCtx *ctx,
                                    uint64_t target,
                                    uint64_t id,
                                    float *out_value);

enum PonkoStatus ponko_ctx_ui_set_scroll(struct PonkoCtx *ctx,
                                         uint64_t target,
                                         uint64_t id,
                                         struct PonkoVec2 offset);

enum PonkoStatus ponko_ctx_ui_scroll(struct PonkoCtx *ctx,
                                     uint64_t target,
                                     uint64_t id,
                                     struct PonkoVec2 *out_offset);

enum PonkoStatus ponko_ctx_ui_push_screen_with_mode(struct PonkoCtx *ctx,
                                                    uint64_t target,
                                                    uint64_t root,
                                                    uint32_t mode);

enum PonkoStatus ponko_ctx_ui_push_screen(struct PonkoCtx *ctx,
                                          uint64_t target,
                                          uint64_t root,
                                          bool modal);

enum PonkoStatus ponko_ctx_ui_pop_screen(struct PonkoCtx *ctx, uint64_t target, uint64_t *out_root);

enum PonkoStatus ponko_ctx_ui_top_screen(struct PonkoCtx *ctx, uint64_t target, uint64_t *out_root);

enum PonkoStatus ponko_ctx_ui_focus(struct PonkoCtx *ctx, uint64_t target, uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_layout_passes(struct PonkoCtx *ctx,
                                            uint64_t target,
                                            uint64_t *out_passes);

enum PonkoStatus ponko_ctx_ui_set_scale(struct PonkoCtx *ctx, uint64_t target, float scale);

enum PonkoStatus ponko_ctx_ui_rect(struct PonkoCtx *ctx,
                                   uint64_t target,
                                   uint64_t id,
                                   struct PonkoRect *out_rect);

enum PonkoStatus ponko_ctx_ui_hit_test(struct PonkoCtx *ctx,
                                       uint64_t target,
                                       struct PonkoVec2 point,
                                       uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_wants_pointer(struct PonkoCtx *ctx,
                                            uint64_t target,
                                            bool *out_wanted);

enum PonkoStatus ponko_ctx_ui_wants_keyboard(struct PonkoCtx *ctx, bool *out_wanted);

enum PonkoStatus ponko_ctx_ui_next_event(struct PonkoCtx *ctx,
                                         uint64_t target,
                                         struct PonkoUiEvent *out_event);

enum PonkoStatus ponko_ctx_ui_layout(struct PonkoCtx *ctx,
                                     uint64_t target,
                                     struct PonkoVec2 viewport);

enum PonkoStatus ponko_ctx_ui_frame(struct PonkoCtx *ctx,
                                    uint64_t target,
                                    struct PonkoVec2 viewport,
                                    const struct PonkoUiInput *input);

enum PonkoStatus ponko_ctx_ui_insert_text(struct PonkoCtx *ctx, const char *text);

struct PonkoUiLayout ponko_ui_layout_row(float height);

struct PonkoUiLayout ponko_ui_layout_column(float width);

struct PonkoUiLayout ponko_ui_layout_fill(void);

struct PonkoUiLayout ponko_ui_layout_fixed(float width, float height);

struct PonkoUiLayout ponko_ui_layout_anchored(float x, float y, float width, float height);

struct PonkoUiLayout ponko_ui_layout_centered(float width, float height);

struct PonkoUiTheme ponko_ui_theme_default(void);

enum PonkoStatus ponko_ctx_ui_theme(struct PonkoCtx *ctx,
                                    uint64_t target,
                                    struct PonkoUiTheme *out_theme);

enum PonkoStatus ponko_ctx_ui_set_theme(struct PonkoCtx *ctx,
                                        uint64_t target,
                                        const struct PonkoUiTheme *theme);

enum PonkoStatus ponko_ctx_ui_paint(struct PonkoCtx *ctx,
                                    uint64_t target,
                                    uint64_t id,
                                    struct PonkoUiPaint *out_paint);

enum PonkoStatus ponko_ctx_ui_add_title(struct PonkoCtx *ctx,
                                        uint64_t target,
                                        uint64_t parent,
                                        const char *text,
                                        const struct PonkoUiLayout *layout,
                                        uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_set_name(struct PonkoCtx *ctx,
                                       uint64_t target,
                                       uint64_t id,
                                       const char *name);

enum PonkoStatus ponko_ctx_ui_find(struct PonkoCtx *ctx,
                                   uint64_t target,
                                   const char *name,
                                   uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_name(struct PonkoCtx *ctx,
                                   uint64_t target,
                                   uint64_t id,
                                   char *buffer,
                                   size_t capacity,
                                   size_t *out_len);

enum PonkoStatus ponko_ctx_ui_take_click(struct PonkoCtx *ctx, uint64_t target, uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_add_dropdown(struct PonkoCtx *ctx,
                                           uint64_t target,
                                           uint64_t parent,
                                           const char *const *options,
                                           size_t count,
                                           const struct PonkoUiLayout *layout,
                                           uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_add_tabs(struct PonkoCtx *ctx,
                                       uint64_t target,
                                       uint64_t parent,
                                       const struct PonkoUiLayout *layout,
                                       uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_add_tab(struct PonkoCtx *ctx,
                                      uint64_t target,
                                      uint64_t tabs,
                                      const char *title,
                                      uint64_t *out_page);

enum PonkoStatus ponko_ctx_ui_tab_button(struct PonkoCtx *ctx,
                                         uint64_t target,
                                         uint64_t tabs,
                                         uint64_t index,
                                         uint64_t *out_tab);

enum PonkoStatus ponko_ctx_ui_add_radio_group(struct PonkoCtx *ctx,
                                              uint64_t target,
                                              uint64_t parent,
                                              const struct PonkoUiLayout *layout,
                                              uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_add_radio(struct PonkoCtx *ctx,
                                        uint64_t target,
                                        uint64_t group,
                                        const char *text,
                                        const struct PonkoUiLayout *layout,
                                        uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_add_progress_bar(struct PonkoCtx *ctx,
                                               uint64_t target,
                                               uint64_t parent,
                                               float value,
                                               const struct PonkoUiLayout *layout,
                                               uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_add_spinner(struct PonkoCtx *ctx,
                                          uint64_t target,
                                          uint64_t parent,
                                          const struct PonkoUiLayout *layout,
                                          uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_add_number_field(struct PonkoCtx *ctx,
                                               uint64_t target,
                                               uint64_t parent,
                                               double value,
                                               const struct PonkoUiLayout *layout,
                                               uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_add_list(struct PonkoCtx *ctx,
                                       uint64_t target,
                                       uint64_t parent,
                                       uint32_t columns,
                                       float row_height,
                                       const struct PonkoUiLayout *layout,
                                       uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_add_split(struct PonkoCtx *ctx,
                                        uint64_t target,
                                        uint64_t parent,
                                        enum PonkoUiAxis axis,
                                        const struct PonkoUiLayout *layout,
                                        uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_split_panes(struct PonkoCtx *ctx,
                                          uint64_t target,
                                          uint64_t split,
                                          uint64_t *out_first,
                                          uint64_t *out_second);

enum PonkoStatus ponko_ctx_ui_add_key_binding(struct PonkoCtx *ctx,
                                              uint64_t target,
                                              uint64_t parent,
                                              const struct PonkoActionBinding *binding,
                                              const struct PonkoUiLayout *layout,
                                              uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_set_items(struct PonkoCtx *ctx,
                                        uint64_t target,
                                        uint64_t id,
                                        const char *const *items,
                                        size_t count);

enum PonkoStatus ponko_ctx_ui_item(struct PonkoCtx *ctx,
                                   uint64_t target,
                                   uint64_t id,
                                   uint64_t index,
                                   char *buffer,
                                   size_t capacity,
                                   size_t *out_len);

enum PonkoStatus ponko_ctx_ui_selected(struct PonkoCtx *ctx,
                                       uint64_t target,
                                       uint64_t id,
                                       uint64_t *out_index);

enum PonkoStatus ponko_ctx_ui_set_selected(struct PonkoCtx *ctx,
                                           uint64_t target,
                                           uint64_t id,
                                           uint64_t index);

enum PonkoStatus ponko_ctx_ui_number(struct PonkoCtx *ctx,
                                     uint64_t target,
                                     uint64_t id,
                                     double *out_value);

enum PonkoStatus ponko_ctx_ui_set_number(struct PonkoCtx *ctx,
                                         uint64_t target,
                                         uint64_t id,
                                         double value);

enum PonkoStatus ponko_ctx_ui_set_number_range(struct PonkoCtx *ctx,
                                               uint64_t target,
                                               uint64_t id,
                                               double min,
                                               double max,
                                               double step);

enum PonkoStatus ponko_ctx_ui_set_list_len(struct PonkoCtx *ctx,
                                           uint64_t target,
                                           uint64_t id,
                                           uint64_t len);

enum PonkoStatus ponko_ctx_ui_list_item(struct PonkoCtx *ctx,
                                        uint64_t target,
                                        uint64_t row,
                                        uint64_t *out_index);

enum PonkoStatus ponko_ctx_ui_scroll_to_item(struct PonkoCtx *ctx,
                                             uint64_t target,
                                             uint64_t id,
                                             uint64_t index);

enum PonkoStatus ponko_ctx_ui_binding(struct PonkoCtx *ctx,
                                      uint64_t target,
                                      uint64_t id,
                                      struct PonkoActionBinding *out_binding,
                                      bool *out_bound);

enum PonkoStatus ponko_ctx_ui_set_binding(struct PonkoCtx *ctx,
                                          uint64_t target,
                                          uint64_t id,
                                          const struct PonkoActionBinding *binding);

enum PonkoStatus ponko_ctx_ui_is_capturing(struct PonkoCtx *ctx,
                                           uint64_t target,
                                           uint64_t id,
                                           bool *out_capturing);

enum PonkoStatus ponko_ctx_ui_set_tooltip(struct PonkoCtx *ctx,
                                          uint64_t target,
                                          uint64_t id,
                                          const char *text);

enum PonkoStatus ponko_ctx_ui_shown_tooltip(struct PonkoCtx *ctx,
                                            uint64_t target,
                                            uint64_t *out_id,
                                            struct PonkoRect *out_rect);

enum PonkoStatus ponko_ctx_ui_set_context_menu(struct PonkoCtx *ctx,
                                               uint64_t target,
                                               uint64_t id,
                                               const char *const *entries,
                                               size_t count);

enum PonkoStatus ponko_ctx_ui_open_context_menu(struct PonkoCtx *ctx, uint64_t target, uint64_t id);

enum PonkoStatus ponko_ctx_ui_popup_owner(struct PonkoCtx *ctx, uint64_t target, uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_close_popup(struct PonkoCtx *ctx, uint64_t target);

enum PonkoStatus ponko_ctx_ui_set_draggable(struct PonkoCtx *ctx,
                                            uint64_t target,
                                            uint64_t id,
                                            bool on);

enum PonkoStatus ponko_ctx_ui_set_drop_target(struct PonkoCtx *ctx,
                                              uint64_t target,
                                              uint64_t id,
                                              bool on);

enum PonkoStatus ponko_ctx_ui_dragging(struct PonkoCtx *ctx, uint64_t target, uint64_t *out_id);

enum PonkoStatus ponko_ctx_ui_set_parent(struct PonkoCtx *ctx,
                                         uint64_t target,
                                         uint64_t id,
                                         uint64_t parent);

enum PonkoStatus ponko_ctx_ui_next_event_ex(struct PonkoCtx *ctx,
                                            uint64_t target,
                                            struct PonkoUiEventEx *out_event);

enum PonkoStatus ponko_ui_theme_ex_dark(struct PonkoUiThemeEx *out_theme);

enum PonkoStatus ponko_ui_theme_ex_light(struct PonkoUiThemeEx *out_theme);

enum PonkoStatus ponko_ctx_ui_theme_ex(struct PonkoCtx *ctx,
                                       uint64_t target,
                                       struct PonkoUiThemeEx *out_theme);

enum PonkoStatus ponko_ctx_ui_set_theme_ex(struct PonkoCtx *ctx,
                                           uint64_t target,
                                           const struct PonkoUiThemeEx *theme);

enum PonkoStatus ponko_ui_input_ex_default(struct PonkoUiInputEx *out_input);

enum PonkoStatus ponko_ctx_ui_frame_ex(struct PonkoCtx *ctx,
                                       uint64_t target,
                                       struct PonkoVec2 viewport,
                                       const struct PonkoUiInputEx *input);

enum PonkoStatus ponko_ctx_ui_toggle(struct PonkoCtx *ctx,
                                     struct PonkoRect rect,
                                     const char *label,
                                     bool *on,
                                     bool *out_changed);

enum PonkoStatus ponko_ctx_ui_slider(struct PonkoCtx *ctx,
                                     struct PonkoRect rect,
                                     float *value,
                                     bool *out_changed);

enum PonkoStatus ponko_ctx_ui_progress_bar(struct PonkoCtx *ctx,
                                           struct PonkoRect rect,
                                           float value);

struct PonkoFrictionCurve ponko_friction_curve_forward(void);

struct PonkoFrictionCurve ponko_friction_curve_sideways(void);

float ponko_friction_curve_evaluate(struct PonkoFrictionCurve curve, float slip);

struct PonkoWheel3D ponko_wheel3d(struct PonkoVec3 position, float radius);

struct PonkoVehicle3D ponko_vehicle3d(void);

enum PonkoStatus ponko_vehicle3d_add_wheel(struct PonkoVehicle3D *vehicle,
                                           struct PonkoWheel3D wheel);

struct PonkoTransform3D ponko_wheel3d_pose(const struct PonkoWheel3D *wheel,
                                           struct PonkoTransform3D chassis);

enum PonkoStatus ponko_vehicle3d_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_action_map_count(const struct PonkoActionMap *map, size_t *out_count);

enum PonkoStatus ponko_action_map_name(const struct PonkoActionMap *map,
                                       size_t index,
                                       char *buffer,
                                       size_t capacity,
                                       size_t *out_len);

enum PonkoStatus ponko_action_map_get(const struct PonkoActionMap *map,
                                      const char *name,
                                      uint32_t *out_kind,
                                      struct PonkoActionBinding *out_bindings,
                                      size_t capacity,
                                      size_t *out_count);

enum PonkoStatus ponko_ctx_set_action(struct PonkoCtx *ctx,
                                      const char *name,
                                      uint32_t kind,
                                      const struct PonkoActionBinding *bindings,
                                      size_t count);

enum PonkoStatus ponko_ctx_play_sound_at_keyed(struct PonkoCtx *ctx,
                                               struct PonkoSound sound,
                                               struct PonkoVec3 position,
                                               const struct PonkoAudioSettings3D *settings,
                                               const struct PonkoAudioKey *keys,
                                               size_t count,
                                               struct PonkoPlaying *out_playing);

enum PonkoStatus ponko_ctx_set_audio_source3d_keyed(struct PonkoCtx *ctx,
                                                    uint64_t entity_bits,
                                                    struct PonkoSound sound,
                                                    const struct PonkoAudioSettings3D *settings,
                                                    const struct PonkoAudioKey *keys,
                                                    size_t count);

enum PonkoStatus ponko_ctx_audio_unlocked(struct PonkoCtx *ctx, bool *out_unlocked);

enum PonkoStatus ponko_game_add_builtin(struct PonkoGame *game,
                                        uint32_t phase_raw,
                                        uint32_t system_id);

enum PonkoStatus ponko_builtin_access(uint32_t system_id, struct PonkoAccess **out_access);

enum PonkoStatus ponko_access_clone(const struct PonkoAccess *access,
                                    struct PonkoAccess **out_access);

enum PonkoStatus ponko_ctx_ecs_diagnostics(struct PonkoCtx *ctx,
                                           char *buffer,
                                           size_t capacity,
                                           size_t *out_len);

enum PonkoStatus ponko_ctx_set_lod_hysteresis(struct PonkoCtx *ctx,
                                              uint64_t entity,
                                              float fraction);

enum PonkoStatus ponko_ctx_lod_hysteresis(struct PonkoCtx *ctx,
                                          uint64_t entity,
                                          float *out_fraction);

enum PonkoStatus ponko_lod_instance_levels(float size,
                                           struct PonkoVec3 center,
                                           const struct PonkoLodLevel *levels,
                                           size_t count,
                                           float scale,
                                           float fov_y,
                                           struct PonkoLodSettings settings,
                                           struct PonkoInstanceLevel *out_levels,
                                           size_t capacity,
                                           size_t *out_count);

enum PonkoStatus ponko_ctx_set_scene_compute_passes(struct PonkoCtx *ctx,
                                                    const struct PonkoComputePass *passes,
                                                    size_t count);

enum PonkoStatus ponko_ctx_scene_compute_passes(struct PonkoCtx *ctx,
                                                struct PonkoComputePass *out_passes,
                                                size_t capacity,
                                                size_t *out_count);

enum PonkoStatus ponko_ctx_set_target_effects(struct PonkoCtx *ctx,
                                              const struct PonkoTargetEffect *effects,
                                              size_t count);

enum PonkoStatus ponko_ctx_target_effects(struct PonkoCtx *ctx,
                                          struct PonkoTargetEffect *out_effects,
                                          size_t capacity,
                                          size_t *out_count);

enum PonkoStatus ponko_ctx_storage_world_slots(struct PonkoCtx *ctx,
                                               char *buffer,
                                               size_t capacity,
                                               size_t *out_len);

enum PonkoStatus ponko_ctx_delete_world(struct PonkoCtx *ctx, const char *slot);

enum PonkoStatus ponko_ctx_storage_is_persistent(struct PonkoCtx *ctx, bool *out_persistent);

enum PonkoStatus ponko_ctx_storage_is_saving(struct PonkoCtx *ctx, bool *out_saving);

enum PonkoStatus ponko_ctx_ui_immediate_font(struct PonkoCtx *ctx, struct PonkoFont *out_font);

enum PonkoStatus ponko_ctx_set_ui_immediate_font(struct PonkoCtx *ctx, struct PonkoFont font);

enum PonkoStatus ponko_ctx_ui_immediate_border_width(struct PonkoCtx *ctx, float *out_width);

enum PonkoStatus ponko_ctx_set_ui_immediate_border_width(struct PonkoCtx *ctx, float width);

enum PonkoStatus ponko_ctx_ui_immediate_scale(struct PonkoCtx *ctx, float *out_scale);

enum PonkoStatus ponko_ctx_set_font_fallbacks(struct PonkoCtx *ctx,
                                              struct PonkoFont font,
                                              const struct PonkoFont *fallbacks,
                                              size_t count);

enum PonkoStatus ponko_ctx_font_fallbacks(struct PonkoCtx *ctx,
                                          struct PonkoFont font,
                                          struct PonkoFont *out_fonts,
                                          size_t capacity,
                                          size_t *out_count);

enum PonkoStatus ponko_ctx_set_text_shaping(struct PonkoCtx *ctx, uint32_t mode);

enum PonkoStatus ponko_ctx_text_shaping(struct PonkoCtx *ctx, uint32_t *out_mode);

enum PonkoStatus ponko_ctx_try_load_system_font(struct PonkoCtx *ctx,
                                                const char *family,
                                                struct PonkoFont *out_font);

enum PonkoStatus ponko_ctx_set_system_font_fallback(struct PonkoCtx *ctx,
                                                    bool on,
                                                    bool *out_available);

enum PonkoStatus ponko_ctx_set_gpu_buffer_motion(struct PonkoCtx *ctx,
                                                 struct PonkoGpuBuffer buffer,
                                                 bool keep);

enum PonkoStatus ponko_ctx_reset_motion(struct PonkoCtx *ctx, uint64_t entity);

enum PonkoStatus ponko_ctx_feed_ime(struct PonkoCtx *ctx, const struct PonkoImeEvent *event);

enum PonkoStatus ponko_ctx_preedit(struct PonkoCtx *ctx,
                                   char *buffer,
                                   size_t capacity,
                                   size_t *out_len);

enum PonkoStatus ponko_ctx_typed_text(struct PonkoCtx *ctx,
                                      char *buffer,
                                      size_t capacity,
                                      size_t *out_len);

enum PonkoStatus ponko_ctx_preedit_cursor(struct PonkoCtx *ctx,
                                          bool *out_has,
                                          size_t *out_start,
                                          size_t *out_end);

enum PonkoStatus ponko_ctx_composing(struct PonkoCtx *ctx, bool *out_composing);

enum PonkoStatus ponko_ctx_set_ime_allowed(struct PonkoCtx *ctx, bool allowed);

enum PonkoStatus ponko_ctx_ime_allowed(struct PonkoCtx *ctx, bool *out_allowed);

enum PonkoStatus ponko_ctx_set_ime_cursor_area(struct PonkoCtx *ctx, struct PonkoRect area);

enum PonkoStatus ponko_ctx_ime_cursor_area(struct PonkoCtx *ctx,
                                           bool *out_has,
                                           struct PonkoRect *out_area);

enum PonkoStatus ponko_ctx_cancel_composition(struct PonkoCtx *ctx);

enum PonkoStatus ponko_ctx_ui_set_ime_allowed(struct PonkoCtx *ctx,
                                              uint64_t target,
                                              uint64_t id,
                                              bool allowed);

enum PonkoStatus ponko_ctx_ui_ime_allowed(struct PonkoCtx *ctx,
                                          uint64_t target,
                                          uint64_t id,
                                          bool *out_allowed);

enum PonkoStatus ponko_ctx_ui_wants_ime(struct PonkoCtx *ctx, uint64_t target, bool *out_wants);

enum PonkoStatus ponko_ctx_ui_preedit(struct PonkoCtx *ctx,
                                      uint64_t target,
                                      uint64_t id,
                                      char *buffer,
                                      size_t capacity,
                                      size_t *out_len);

enum PonkoStatus ponko_ctx_ui_ime_area(struct PonkoCtx *ctx,
                                       uint64_t target,
                                       bool *out_has,
                                       struct PonkoRect *out_area);

enum PonkoStatus ponko_ctx_ui_composition_interrupted(struct PonkoCtx *ctx,
                                                      uint64_t target,
                                                      bool *out_interrupted);

enum PonkoStatus ponko_ctx_ui_frame_ime(struct PonkoCtx *ctx,
                                        uint64_t target,
                                        struct PonkoVec2 viewport,
                                        const struct PonkoUiInput *input,
                                        const struct PonkoUiComposition *composition);

enum PonkoStatus ponko_input_scenario_ime(struct PonkoInputScenario *scenario,
                                          uint64_t tick,
                                          const struct PonkoImeEvent *event);

enum PonkoStatus ponko_ctx_load_strings(struct PonkoCtx *ctx, const char *path);

enum PonkoStatus ponko_ctx_strings_insert_json(struct PonkoCtx *ctx, const char *json);

enum PonkoStatus ponko_ctx_strings_set_language(struct PonkoCtx *ctx,
                                                const char *language,
                                                bool *out_known);

enum PonkoStatus ponko_ctx_strings_language(struct PonkoCtx *ctx,
                                            char *buffer,
                                            size_t capacity,
                                            size_t *out_len);

enum PonkoStatus ponko_ctx_strings_language_count(struct PonkoCtx *ctx, size_t *out_count);

enum PonkoStatus ponko_ctx_strings_language_at(struct PonkoCtx *ctx,
                                               size_t index,
                                               char *code_buffer,
                                               size_t code_capacity,
                                               size_t *out_code_len,
                                               char *name_buffer,
                                               size_t name_capacity,
                                               size_t *out_name_len);

enum PonkoStatus ponko_ctx_strings_is_rtl(struct PonkoCtx *ctx, bool *out_rtl);

enum PonkoStatus ponko_ctx_strings_set_font(struct PonkoCtx *ctx,
                                            const char *language,
                                            struct PonkoFont font);

enum PonkoStatus ponko_ctx_strings_font(struct PonkoCtx *ctx,
                                        bool *out_has,
                                        struct PonkoFont *out_font);

enum PonkoStatus ponko_ctx_strings_format(struct PonkoCtx *ctx,
                                          const char *key,
                                          const struct PonkoStringArg *args,
                                          size_t count,
                                          char *buffer,
                                          size_t capacity,
                                          size_t *out_len);

enum PonkoStatus ponko_ctx_strings_missing_report(struct PonkoCtx *ctx,
                                                  char *buffer,
                                                  size_t capacity,
                                                  size_t *out_len);

enum PonkoStatus ponko_plural_category(const char *language,
                                       double count,
                                       PonkoPluralCategory *out_category);

enum PonkoStatus ponko_ctx_ui_set_text_key(struct PonkoCtx *ctx,
                                           uint64_t target,
                                           uint64_t id,
                                           const char *key,
                                           const struct PonkoStringArg *args,
                                           size_t count);

enum PonkoStatus ponko_ctx_ui_localize(struct PonkoCtx *ctx, uint64_t target);

struct PonkoDebugStyle ponko_debug_style_default(void);

enum PonkoStatus ponko_debug_style_ex_defaults(struct PonkoDebugStyleEx *style);

struct PonkoDebugStyle ponko_debug_style_with_space(struct PonkoDebugStyle style,
                                                    enum PonkoDrawSpace space);

enum PonkoStatus ponko_ctx_debug_set_enabled(struct PonkoCtx *ctx, bool on);

enum PonkoStatus ponko_ctx_debug_enabled(struct PonkoCtx *ctx, bool *out_on);

enum PonkoStatus ponko_ctx_debug_clear(struct PonkoCtx *ctx);

enum PonkoStatus ponko_ctx_debug_show(struct PonkoCtx *ctx, uint32_t view, bool on);

enum PonkoStatus ponko_ctx_debug_showing(struct PonkoCtx *ctx, uint32_t view, bool *out_on);

enum PonkoStatus ponko_ctx_debug_set_collider_distance(struct PonkoCtx *ctx, float distance);

enum PonkoStatus ponko_ctx_debug_collider_distance(struct PonkoCtx *ctx, float *out_distance);

enum PonkoStatus ponko_ctx_debug_stats(struct PonkoCtx *ctx, struct PonkoDebugStats *out_stats);

enum PonkoStatus ponko_ctx_debug_line2d(struct PonkoCtx *ctx,
                                        struct PonkoVec2 a,
                                        struct PonkoVec2 b,
                                        struct PonkoColor color,
                                        const struct PonkoDebugStyle *style);

enum PonkoStatus ponko_ctx_debug_ray2d(struct PonkoCtx *ctx,
                                       struct PonkoVec2 origin,
                                       struct PonkoVec2 direction,
                                       struct PonkoColor color,
                                       const struct PonkoDebugStyle *style);

enum PonkoStatus ponko_ctx_debug_arrow2d(struct PonkoCtx *ctx,
                                         struct PonkoVec2 from,
                                         struct PonkoVec2 to,
                                         struct PonkoColor color,
                                         const struct PonkoDebugStyle *style);

enum PonkoStatus ponko_ctx_debug_box2d(struct PonkoCtx *ctx,
                                       struct PonkoVec2 center,
                                       struct PonkoVec2 size,
                                       struct PonkoColor color,
                                       const struct PonkoDebugStyle *style);

enum PonkoStatus ponko_ctx_debug_circle2d(struct PonkoCtx *ctx,
                                          struct PonkoVec2 center,
                                          float radius,
                                          struct PonkoColor color,
                                          const struct PonkoDebugStyle *style);

enum PonkoStatus ponko_ctx_debug_capsule2d(struct PonkoCtx *ctx,
                                           struct PonkoVec2 a,
                                           struct PonkoVec2 b,
                                           float radius,
                                           struct PonkoColor color,
                                           const struct PonkoDebugStyle *style);

enum PonkoStatus ponko_ctx_debug_polyline2d(struct PonkoCtx *ctx,
                                            const struct PonkoVec2 *points,
                                            size_t count,
                                            bool closed,
                                            struct PonkoColor color,
                                            const struct PonkoDebugStyle *style);

enum PonkoStatus ponko_ctx_debug_grid2d(struct PonkoCtx *ctx,
                                        struct PonkoVec2 center,
                                        float cell,
                                        uint32_t cells,
                                        struct PonkoColor color,
                                        const struct PonkoDebugStyle *style);

enum PonkoStatus ponko_ctx_debug_text2d(struct PonkoCtx *ctx,
                                        struct PonkoVec2 at,
                                        const char *label,
                                        struct PonkoColor color,
                                        const struct PonkoDebugStyle *style);

enum PonkoStatus ponko_ctx_debug_line2d_ex(struct PonkoCtx *ctx,
                                           struct PonkoVec2 a,
                                           struct PonkoVec2 b,
                                           struct PonkoColor color,
                                           const struct PonkoDebugStyleEx *style);

enum PonkoStatus ponko_ctx_debug_ray2d_ex(struct PonkoCtx *ctx,
                                          struct PonkoVec2 origin,
                                          struct PonkoVec2 direction,
                                          struct PonkoColor color,
                                          const struct PonkoDebugStyleEx *style);

enum PonkoStatus ponko_ctx_debug_arrow2d_ex(struct PonkoCtx *ctx,
                                            struct PonkoVec2 from,
                                            struct PonkoVec2 to,
                                            struct PonkoColor color,
                                            const struct PonkoDebugStyleEx *style);

enum PonkoStatus ponko_ctx_debug_box2d_ex(struct PonkoCtx *ctx,
                                          struct PonkoVec2 center,
                                          struct PonkoVec2 size,
                                          struct PonkoColor color,
                                          const struct PonkoDebugStyleEx *style);

enum PonkoStatus ponko_ctx_debug_circle2d_ex(struct PonkoCtx *ctx,
                                             struct PonkoVec2 center,
                                             float radius,
                                             struct PonkoColor color,
                                             const struct PonkoDebugStyleEx *style);

enum PonkoStatus ponko_ctx_debug_capsule2d_ex(struct PonkoCtx *ctx,
                                              struct PonkoVec2 a,
                                              struct PonkoVec2 b,
                                              float radius,
                                              struct PonkoColor color,
                                              const struct PonkoDebugStyleEx *style);

enum PonkoStatus ponko_ctx_debug_polyline2d_ex(struct PonkoCtx *ctx,
                                               const struct PonkoVec2 *points,
                                               size_t count,
                                               bool closed,
                                               struct PonkoColor color,
                                               const struct PonkoDebugStyleEx *style);

enum PonkoStatus ponko_ctx_debug_grid2d_ex(struct PonkoCtx *ctx,
                                           struct PonkoVec2 center,
                                           float cell,
                                           uint32_t cells,
                                           struct PonkoColor color,
                                           const struct PonkoDebugStyleEx *style);

enum PonkoStatus ponko_ctx_debug_text2d_ex(struct PonkoCtx *ctx,
                                           struct PonkoVec2 at,
                                           const char *label,
                                           struct PonkoColor color,
                                           const struct PonkoDebugStyleEx *style);

enum PonkoStatus ponko_ctx_debug_line3d(struct PonkoCtx *ctx,
                                        struct PonkoVec3 a,
                                        struct PonkoVec3 b,
                                        struct PonkoColor color,
                                        const struct PonkoDebugStyle *style);

enum PonkoStatus ponko_ctx_debug_ray3d(struct PonkoCtx *ctx,
                                       struct PonkoVec3 origin,
                                       struct PonkoVec3 direction,
                                       struct PonkoColor color,
                                       const struct PonkoDebugStyle *style);

enum PonkoStatus ponko_ctx_debug_arrow3d(struct PonkoCtx *ctx,
                                         struct PonkoVec3 from,
                                         struct PonkoVec3 to,
                                         struct PonkoColor color,
                                         const struct PonkoDebugStyle *style);

enum PonkoStatus ponko_ctx_debug_box3d(struct PonkoCtx *ctx,
                                       struct PonkoVec3 center,
                                       struct PonkoVec3 size,
                                       struct PonkoQuat rotation,
                                       struct PonkoColor color,
                                       const struct PonkoDebugStyle *style);

enum PonkoStatus ponko_ctx_debug_sphere3d(struct PonkoCtx *ctx,
                                          struct PonkoVec3 center,
                                          float radius,
                                          struct PonkoColor color,
                                          const struct PonkoDebugStyle *style);

enum PonkoStatus ponko_ctx_debug_capsule3d(struct PonkoCtx *ctx,
                                           struct PonkoVec3 a,
                                           struct PonkoVec3 b,
                                           float radius,
                                           struct PonkoColor color,
                                           const struct PonkoDebugStyle *style);

enum PonkoStatus ponko_ctx_debug_polyline3d(struct PonkoCtx *ctx,
                                            const struct PonkoVec3 *points,
                                            size_t count,
                                            bool closed,
                                            struct PonkoColor color,
                                            const struct PonkoDebugStyle *style);

enum PonkoStatus ponko_ctx_debug_grid3d(struct PonkoCtx *ctx,
                                        struct PonkoVec3 center,
                                        float cell,
                                        uint32_t cells,
                                        struct PonkoColor color,
                                        const struct PonkoDebugStyle *style);

enum PonkoStatus ponko_ctx_debug_frustum3d(struct PonkoCtx *ctx,
                                           const struct PonkoCamera3D *camera,
                                           struct PonkoColor color,
                                           const struct PonkoDebugStyle *style);

enum PonkoStatus ponko_ctx_debug_text3d(struct PonkoCtx *ctx,
                                        struct PonkoVec3 at,
                                        const char *label,
                                        struct PonkoColor color,
                                        const struct PonkoDebugStyle *style);

enum PonkoStatus ponko_ctx_debug_text3d_depth_tested(struct PonkoCtx *ctx,
                                                     struct PonkoVec3 at,
                                                     const char *label,
                                                     struct PonkoColor color,
                                                     const struct PonkoDebugStyle *style);

enum PonkoStatus ponko_ctx_debug_axes3d(struct PonkoCtx *ctx,
                                        struct PonkoVec3 at,
                                        struct PonkoQuat rotation,
                                        float length,
                                        const struct PonkoDebugStyle *style);

enum PonkoStatus ponko_collider_ex_from(struct PonkoCollider collider, struct PonkoColliderEx *out);

enum PonkoStatus ponko_set_collider_ex(struct PonkoWorld *world,
                                       uint64_t entity,
                                       const struct PonkoColliderEx *collider);

enum PonkoStatus ponko_get_collider_ex(struct PonkoWorld *world,
                                       uint64_t entity,
                                       struct PonkoColliderEx *out);

enum PonkoStatus ponko_query_filter_ex_defaults(struct PonkoQueryFilterEx *filter);

enum PonkoStatus ponko_ctx_cast_ray2d_ex(struct PonkoCtx *ctx,
                                         struct PonkoVec2 origin,
                                         struct PonkoVec2 direction,
                                         float max_distance,
                                         const struct PonkoQueryFilterEx *filter,
                                         bool *out_found,
                                         struct PonkoRayHit *out_hit);

enum PonkoStatus ponko_ctx_cast_ray2d_all_ex(struct PonkoCtx *ctx,
                                             struct PonkoVec2 origin,
                                             struct PonkoVec2 direction,
                                             float max_distance,
                                             const struct PonkoQueryFilterEx *filter,
                                             struct PonkoRayHit *out_hits,
                                             size_t capacity,
                                             size_t *out_count);

enum PonkoStatus ponko_ctx_cast_circle2d_ex(struct PonkoCtx *ctx,
                                            struct PonkoVec2 origin,
                                            float radius,
                                            struct PonkoVec2 direction,
                                            float max_distance,
                                            const struct PonkoQueryFilterEx *filter,
                                            bool *out_found,
                                            struct PonkoRayHit *out_hit);

enum PonkoStatus ponko_ctx_cast_box2d_ex(struct PonkoCtx *ctx,
                                         struct PonkoVec2 origin,
                                         struct PonkoVec2 size,
                                         float rotation,
                                         struct PonkoVec2 direction,
                                         float max_distance,
                                         const struct PonkoQueryFilterEx *filter,
                                         bool *out_found,
                                         struct PonkoRayHit *out_hit);

enum PonkoStatus ponko_ctx_cast_capsule2d_ex(struct PonkoCtx *ctx,
                                             struct PonkoVec2 origin,
                                             float height,
                                             float radius,
                                             struct PonkoVec2 direction,
                                             float max_distance,
                                             const struct PonkoQueryFilterEx *filter,
                                             bool *out_found,
                                             struct PonkoRayHit *out_hit);

enum PonkoStatus ponko_ctx_overlap_point2d_ex(struct PonkoCtx *ctx,
                                              struct PonkoVec2 point,
                                              const struct PonkoQueryFilterEx *filter,
                                              struct PonkoEntity *out_entities,
                                              size_t capacity,
                                              size_t *out_count);

enum PonkoStatus ponko_ctx_overlap_circle2d_ex(struct PonkoCtx *ctx,
                                               struct PonkoVec2 center,
                                               float radius,
                                               const struct PonkoQueryFilterEx *filter,
                                               struct PonkoEntity *out_entities,
                                               size_t capacity,
                                               size_t *out_count);

enum PonkoStatus ponko_ctx_overlap_box2d_ex(struct PonkoCtx *ctx,
                                            struct PonkoVec2 center,
                                            struct PonkoVec2 size,
                                            float rotation,
                                            const struct PonkoQueryFilterEx *filter,
                                            struct PonkoEntity *out_entities,
                                            size_t capacity,
                                            size_t *out_count);

enum PonkoStatus ponko_ctx_overlap_capsule2d_ex(struct PonkoCtx *ctx,
                                                struct PonkoVec2 center,
                                                float height,
                                                float radius,
                                                const struct PonkoQueryFilterEx *filter,
                                                struct PonkoEntity *out_entities,
                                                size_t capacity,
                                                size_t *out_count);

enum PonkoStatus ponko_ctx_closest_point2d_ex(struct PonkoCtx *ctx,
                                              struct PonkoVec2 point,
                                              float max_distance,
                                              const struct PonkoQueryFilterEx *filter,
                                              bool *out_found,
                                              struct PonkoPointHit2D *out_hit);

enum PonkoStatus ponko_ctx_penetrations2d_ex(struct PonkoCtx *ctx,
                                             struct PonkoCollider shape,
                                             struct PonkoVec2 position,
                                             float rotation,
                                             const struct PonkoQueryFilterEx *filter,
                                             struct PonkoPenetration2D *out_penetrations,
                                             size_t capacity,
                                             size_t *out_count);

enum PonkoStatus ponko_ctx_cast_ray3d_ex(struct PonkoCtx *ctx,
                                         struct PonkoVec3 origin,
                                         struct PonkoVec3 direction,
                                         float max_distance,
                                         const struct PonkoQueryFilterEx *filter,
                                         bool *out_found,
                                         struct PonkoHit3D *out_hit);

enum PonkoStatus ponko_ctx_cast_ray3d_all_ex(struct PonkoCtx *ctx,
                                             struct PonkoVec3 origin,
                                             struct PonkoVec3 direction,
                                             float max_distance,
                                             const struct PonkoQueryFilterEx *filter,
                                             struct PonkoHit3D *out_hits,
                                             size_t capacity,
                                             size_t *out_count);

enum PonkoStatus ponko_ctx_cast_sphere3d_ex(struct PonkoCtx *ctx,
                                            struct PonkoVec3 origin,
                                            float radius,
                                            struct PonkoVec3 direction,
                                            float max_distance,
                                            const struct PonkoQueryFilterEx *filter,
                                            bool *out_found,
                                            struct PonkoHit3D *out_hit);

enum PonkoStatus ponko_ctx_cast_capsule3d_ex(struct PonkoCtx *ctx,
                                             struct PonkoVec3 origin,
                                             float height,
                                             float radius,
                                             struct PonkoVec3 direction,
                                             float max_distance,
                                             const struct PonkoQueryFilterEx *filter,
                                             bool *out_found,
                                             struct PonkoHit3D *out_hit);

enum PonkoStatus ponko_ctx_cast_box3d_ex(struct PonkoCtx *ctx,
                                         struct PonkoVec3 origin,
                                         struct PonkoVec3 size,
                                         struct PonkoQuat rotation,
                                         struct PonkoVec3 direction,
                                         float max_distance,
                                         const struct PonkoQueryFilterEx *filter,
                                         bool *out_found,
                                         struct PonkoHit3D *out_hit);

enum PonkoStatus ponko_ctx_overlap_point3d_ex(struct PonkoCtx *ctx,
                                              struct PonkoVec3 point,
                                              const struct PonkoQueryFilterEx *filter,
                                              struct PonkoEntity *out_entities,
                                              size_t capacity,
                                              size_t *out_count);

enum PonkoStatus ponko_ctx_overlap_sphere3d_ex(struct PonkoCtx *ctx,
                                               struct PonkoVec3 center,
                                               float radius,
                                               const struct PonkoQueryFilterEx *filter,
                                               struct PonkoEntity *out_entities,
                                               size_t capacity,
                                               size_t *out_count);

enum PonkoStatus ponko_ctx_overlap_box3d_ex(struct PonkoCtx *ctx,
                                            struct PonkoVec3 center,
                                            struct PonkoVec3 size,
                                            struct PonkoQuat rotation,
                                            const struct PonkoQueryFilterEx *filter,
                                            struct PonkoEntity *out_entities,
                                            size_t capacity,
                                            size_t *out_count);

enum PonkoStatus ponko_ctx_overlap_capsule3d_ex(struct PonkoCtx *ctx,
                                                struct PonkoVec3 center,
                                                float height,
                                                float radius,
                                                const struct PonkoQueryFilterEx *filter,
                                                struct PonkoEntity *out_entities,
                                                size_t capacity,
                                                size_t *out_count);

enum PonkoStatus ponko_ctx_closest_point3d_ex(struct PonkoCtx *ctx,
                                              struct PonkoVec3 point,
                                              float max_distance,
                                              const struct PonkoQueryFilterEx *filter,
                                              bool *out_found,
                                              struct PonkoPointHit3D *out_hit);

enum PonkoStatus ponko_ctx_penetrations3d_ex(struct PonkoCtx *ctx,
                                             struct PonkoCollider3D shape,
                                             struct PonkoVec3 position,
                                             struct PonkoQuat rotation,
                                             const struct PonkoQueryFilterEx *filter,
                                             struct PonkoPenetration3D *out_penetrations,
                                             size_t capacity,
                                             size_t *out_count);

enum PonkoStatus ponko_material_desc_ex_defaults(struct PonkoMaterialDescEx *desc);

enum PonkoStatus ponko_ctx_create_material_ex(struct PonkoCtx *ctx,
                                              const struct PonkoMaterialDescEx *desc,
                                              struct PonkoMaterial *out_material);

enum PonkoStatus ponko_ctx_update_material_ex(struct PonkoCtx *ctx,
                                              struct PonkoMaterial material,
                                              const struct PonkoMaterialDescEx *desc);

enum PonkoStatus ponko_nav_bake_settings_ex_defaults(struct PonkoNavBakeSettingsEx *settings);

enum PonkoStatus ponko_ctx_bake_navmesh_ex(struct PonkoCtx *ctx,
                                           const struct PonkoNavSources *sources,
                                           const struct PonkoNavBakeSettingsEx *settings);

enum PonkoStatus ponko_lighting_ex_defaults(struct PonkoLightingEx *lighting);

enum PonkoStatus ponko_ctx_lighting_ex(struct PonkoCtx *ctx, struct PonkoLightingEx *out_lighting);

enum PonkoStatus ponko_ctx_set_lighting_ex(struct PonkoCtx *ctx,
                                           const struct PonkoLightingEx *lighting);

enum PonkoStatus ponko_post_ex_defaults(struct PonkoPostEx *post);

enum PonkoStatus ponko_ctx_post_ex(struct PonkoCtx *ctx, struct PonkoPostEx *out_post);

enum PonkoStatus ponko_ctx_set_post_ex(struct PonkoCtx *ctx, const struct PonkoPostEx *post);

enum PonkoStatus ponko_post_ex_lerp(const struct PonkoPostEx *a,
                                    const struct PonkoPostEx *b,
                                    float t,
                                    struct PonkoPostEx *out_post);

enum PonkoStatus ponko_post_blend_volumes(const struct PonkoPostEx *base,
                                          const struct PonkoPostVolume *volumes,
                                          size_t count,
                                          struct PonkoVec3 at,
                                          struct PonkoPostEx *out_post);

enum PonkoStatus ponko_particle_settings3d_ex_defaults(struct PonkoParticleSettings3DEx *settings);

enum PonkoStatus ponko_ctx_set_particle_emitter3d_ex(struct PonkoCtx *ctx,
                                                     uint64_t entity_bits,
                                                     const struct PonkoParticleSettings3DEx *settings);

enum PonkoStatus ponko_camera3d_ex_from(const struct PonkoCamera3D *camera,
                                        struct PonkoCamera3DEx *out_camera);

enum PonkoStatus ponko_ctx_camera3d_ex(struct PonkoCtx *ctx, struct PonkoCamera3DEx *out_camera);

enum PonkoStatus ponko_ctx_set_camera3d_ex(struct PonkoCtx *ctx,
                                           const struct PonkoCamera3DEx *camera);

enum PonkoStatus ponko_target_camera_ex_from(const struct PonkoTargetCamera *camera,
                                             struct PonkoTargetCameraEx *out_camera);

enum PonkoStatus ponko_ctx_set_target_cameras_ex(struct PonkoCtx *ctx,
                                                 const struct PonkoTargetCameraEx *const *cameras,
                                                 size_t count);

enum PonkoStatus ponko_ctx_target_camera_ex(struct PonkoCtx *ctx,
                                            size_t index,
                                            struct PonkoTargetCameraEx *out_camera);

enum PonkoStatus ponko_prediction_new(const void *state,
                                      size_t state_size,
                                      size_t input_size,
                                      size_t capacity,
                                      struct PonkoPrediction **out_prediction);

void ponko_prediction_free(struct PonkoPrediction *prediction);

enum PonkoStatus ponko_prediction_predict(struct PonkoPrediction *prediction,
                                          const void *input,
                                          PonkoPredictStepFn step,
                                          void *user,
                                          uint16_t *out_seq);

enum PonkoStatus ponko_prediction_reconcile(struct PonkoPrediction *prediction,
                                            uint16_t ack,
                                            const void *state,
                                            PonkoPredictStepFn step,
                                            void *user,
                                            struct PonkoReconciled *out_result,
                                            void *out_before);

enum PonkoStatus ponko_prediction_state(const struct PonkoPrediction *prediction, void *out_state);

enum PonkoStatus ponko_prediction_previous(const struct PonkoPrediction *prediction,
                                           void *out_state);

enum PonkoStatus ponko_prediction_reset(struct PonkoPrediction *prediction, const void *state);

enum PonkoStatus ponko_prediction_unacked(const struct PonkoPrediction *prediction,
                                          uint16_t *out_seqs,
                                          void *out_inputs,
                                          size_t capacity,
                                          size_t *out_count);

enum PonkoStatus ponko_prediction_acked(const struct PonkoPrediction *prediction,
                                        bool *out_has,
                                        uint16_t *out_ack);

enum PonkoStatus ponko_prediction_next_seq(const struct PonkoPrediction *prediction,
                                           uint16_t *out_seq);

enum PonkoStatus ponko_prediction_stats(const struct PonkoPrediction *prediction,
                                        struct PonkoPredictionStats *out_stats);

enum PonkoStatus ponko_input_queue_new(size_t input_size,
                                       size_t capacity,
                                       struct PonkoInputQueue **out_queue);

void ponko_input_queue_free(struct PonkoInputQueue *queue);

enum PonkoStatus ponko_input_queue_push(struct PonkoInputQueue *queue,
                                        uint16_t seq,
                                        const void *input,
                                        uint32_t *out_push);

enum PonkoStatus ponko_input_queue_pop(struct PonkoInputQueue *queue,
                                       bool *out_found,
                                       uint16_t *out_seq,
                                       void *out_input);

enum PonkoStatus ponko_input_queue_ack(const struct PonkoInputQueue *queue,
                                       bool *out_has,
                                       uint16_t *out_ack);

const char *ponko_human_bone_name(uint32_t bone);

bool ponko_human_bone_is_required(uint32_t bone);

enum PonkoStatus ponko_humanoid_map_new(struct PonkoHumanoidMap **out_map);

enum PonkoStatus ponko_ctx_detect_humanoid_map(struct PonkoCtx *ctx,
                                               uint64_t skeleton_entity,
                                               struct PonkoHumanoidMap **out_map);

void ponko_humanoid_map_free(struct PonkoHumanoidMap *map);

enum PonkoStatus ponko_humanoid_map_get(const struct PonkoHumanoidMap *map,
                                        uint32_t bone,
                                        int32_t *out_joint);

enum PonkoStatus ponko_humanoid_map_set(struct PonkoHumanoidMap *map, uint32_t bone, int32_t joint);

enum PonkoStatus ponko_humanoid_map_missing(const struct PonkoHumanoidMap *map,
                                            uint32_t *out_bones,
                                            size_t capacity,
                                            size_t *out_count);

enum PonkoStatus ponko_ctx_create_retargeter(struct PonkoCtx *ctx,
                                             uint64_t source_entity,
                                             const struct PonkoHumanoidMap *source_map,
                                             uint64_t target_entity,
                                             const struct PonkoHumanoidMap *target_map,
                                             struct PonkoRetargeter **out_retargeter);

void ponko_retargeter_free(struct PonkoRetargeter *retargeter);

enum PonkoStatus ponko_retargeter_scale(const struct PonkoRetargeter *retargeter, float *out_scale);

enum PonkoStatus ponko_retargeter_set_foot_ik(struct PonkoRetargeter *retargeter, bool on);

enum PonkoStatus ponko_retargeter_set_sample_rate(struct PonkoRetargeter *retargeter, float rate);

enum PonkoStatus ponko_retarget_clip(struct PonkoRetargeter *retargeter,
                                     const struct PonkoClip *clip,
                                     struct PonkoClip **out_clip);

enum PonkoStatus ponko_ctx_retarget_pose(struct PonkoCtx *ctx,
                                         struct PonkoRetargeter *retargeter,
                                         uint64_t source_entity,
                                         uint64_t target_entity);

enum PonkoStatus ponko_ctx_aseprite_frame(struct PonkoCtx *ctx,
                                          const char *path,
                                          bool split_layers,
                                          uint32_t frame,
                                          const char *layer,
                                          struct PonkoSprite *out_sprite);

enum PonkoStatus ponko_ctx_play_aseprite(struct PonkoCtx *ctx,
                                         uint64_t entity,
                                         const char *path,
                                         bool split_layers,
                                         const char *tag,
                                         const char *layer);

float ponko_smooth_damp(float current, float target, float *velocity, float smooth_time, float dt);

struct PonkoVec2 ponko_smooth_damp_vec2(struct PonkoVec2 current,
                                        struct PonkoVec2 target,
                                        struct PonkoVec2 *velocity,
                                        float smooth_time,
                                        float dt);

struct PonkoVec3 ponko_smooth_damp_vec3(struct PonkoVec3 current,
                                        struct PonkoVec3 target,
                                        struct PonkoVec3 *velocity,
                                        float smooth_time,
                                        float dt);

float ponko_damp(float current, float target, float half_life, float dt);

struct PonkoVec2 ponko_damp_vec2(struct PonkoVec2 current,
                                 struct PonkoVec2 target,
                                 float half_life,
                                 float dt);

struct PonkoVec3 ponko_damp_vec3(struct PonkoVec3 current,
                                 struct PonkoVec3 target,
                                 float half_life,
                                 float dt);

float ponko_damp_angle(float current, float target, float half_life, float dt);

enum PonkoStatus ponko_camera_shake_defaults(struct PonkoCameraShake *shake);

enum PonkoStatus ponko_camera_shake_add_trauma(struct PonkoCameraShake *shake, float amount);

enum PonkoStatus ponko_camera_shake_update(struct PonkoCameraShake *shake, float dt);

enum PonkoStatus ponko_camera_shake_apply2d(const struct PonkoCameraShake *shake,
                                            struct PonkoCtx *ctx);

enum PonkoStatus ponko_camera_shake_apply3d(const struct PonkoCameraShake *shake,
                                            struct PonkoCtx *ctx);

enum PonkoStatus ponko_follow_camera2d_defaults(struct PonkoFollowCamera2D *rig);

enum PonkoStatus ponko_follow_camera2d_update(struct PonkoFollowCamera2D *rig,
                                              struct PonkoCtx *ctx,
                                              struct PonkoVec2 target,
                                              float dt);

enum PonkoStatus ponko_follow_camera2d_snap(struct PonkoFollowCamera2D *rig,
                                            struct PonkoCtx *ctx,
                                            struct PonkoVec2 target);

enum PonkoStatus ponko_pixel_perfect_defaults(struct PonkoPixelPerfect *pixels);

enum PonkoStatus ponko_pixel_perfect_apply(const struct PonkoPixelPerfect *pixels,
                                           struct PonkoCtx *ctx);

enum PonkoStatus ponko_pixel_perfect_screen_rect(const struct PonkoPixelPerfect *pixels,
                                                 struct PonkoCtx *ctx,
                                                 struct PonkoRect *out_rect);

enum PonkoStatus ponko_orbit_camera_defaults(struct PonkoOrbitCamera *rig);

enum PonkoStatus ponko_orbit_camera_mouse_controls(struct PonkoOrbitCamera *rig,
                                                   struct PonkoCtx *ctx);

enum PonkoStatus ponko_orbit_camera_update(struct PonkoOrbitCamera *rig,
                                           struct PonkoCtx *ctx,
                                           float dt);

enum PonkoStatus ponko_first_person_camera_defaults(struct PonkoFirstPersonCamera *rig);

enum PonkoStatus ponko_first_person_camera_look(struct PonkoFirstPersonCamera *rig,
                                                struct PonkoVec2 delta);

enum PonkoStatus ponko_first_person_camera_flat_forward(const struct PonkoFirstPersonCamera *rig,
                                                        struct PonkoVec3 *out_forward);

enum PonkoStatus ponko_first_person_camera_flat_right(const struct PonkoFirstPersonCamera *rig,
                                                      struct PonkoVec3 *out_right);

enum PonkoStatus ponko_first_person_camera_update(struct PonkoFirstPersonCamera *rig,
                                                  struct PonkoCtx *ctx,
                                                  struct PonkoVec3 eye,
                                                  float dt);

enum PonkoStatus ponko_third_person_camera_defaults(struct PonkoThirdPersonCamera *rig);

enum PonkoStatus ponko_third_person_camera_look(struct PonkoThirdPersonCamera *rig,
                                                struct PonkoVec2 delta);

enum PonkoStatus ponko_third_person_camera_flat_forward(const struct PonkoThirdPersonCamera *rig,
                                                        struct PonkoVec3 *out_forward);

enum PonkoStatus ponko_third_person_camera_flat_right(const struct PonkoThirdPersonCamera *rig,
                                                      struct PonkoVec3 *out_right);

enum PonkoStatus ponko_third_person_camera_update(struct PonkoThirdPersonCamera *rig,
                                                  struct PonkoCtx *ctx,
                                                  struct PonkoVec3 target,
                                                  float dt);

enum PonkoStatus ponko_third_person_camera_update_physics(struct PonkoThirdPersonCamera *rig,
                                                          struct PonkoCtx *ctx,
                                                          struct PonkoVec3 target,
                                                          const struct PonkoQueryFilterEx *filter,
                                                          float dt);

enum PonkoStatus ponko_ctx_world_to_screen(struct PonkoCtx *ctx,
                                           struct PonkoVec2 world,
                                           struct PonkoVec2 *out_screen);

enum PonkoStatus ponko_ctx_screen_to_world(struct PonkoCtx *ctx,
                                           struct PonkoVec2 screen,
                                           struct PonkoVec2 *out_world);

enum PonkoStatus ponko_ctx_world_to_screen3d(struct PonkoCtx *ctx,
                                             struct PonkoVec3 world,
                                             bool *out_visible,
                                             struct PonkoVec2 *out_screen);

enum PonkoStatus ponko_ctx_screen_to_world3d(struct PonkoCtx *ctx,
                                             struct PonkoVec2 screen,
                                             float depth,
                                             struct PonkoVec3 *out_world);

enum PonkoStatus ponko_ctx_screen_to_ground(struct PonkoCtx *ctx,
                                            struct PonkoVec2 screen,
                                            float height,
                                            bool *out_found,
                                            struct PonkoVec3 *out_point);

struct PonkoViewport ponko_viewport_split(uint32_t players, uint32_t index);

enum PonkoStatus ponko_view2d_defaults(struct PonkoView2D *view);

enum PonkoStatus ponko_ctx_camera_view(struct PonkoCtx *ctx, struct PonkoView2D *out_view);

enum PonkoStatus ponko_ctx_set_camera_view(struct PonkoCtx *ctx, const struct PonkoView2D *view);

enum PonkoStatus ponko_ctx_view2d_count(struct PonkoCtx *ctx, size_t *out_count);

enum PonkoStatus ponko_ctx_view2d(struct PonkoCtx *ctx, size_t index, struct PonkoView2D *out_view);

enum PonkoStatus ponko_ctx_set_view2d(struct PonkoCtx *ctx,
                                      size_t index,
                                      const struct PonkoView2D *view);

enum PonkoStatus ponko_ctx_remove_view2d(struct PonkoCtx *ctx, size_t index);

enum PonkoStatus ponko_ctx_camera_at(struct PonkoCtx *ctx,
                                     struct PonkoVec2 screen,
                                     int32_t *out_view);

enum PonkoStatus ponko_ctx_screen_to_world_at(struct PonkoCtx *ctx,
                                              struct PonkoVec2 screen,
                                              struct PonkoVec2 *out_world);

enum PonkoStatus ponko_ctx_set_draw_layers(struct PonkoCtx *ctx, uint32_t layers);

enum PonkoStatus ponko_world_set_render_layers(struct PonkoWorld *world,
                                               uint64_t entity,
                                               uint32_t layers);

enum PonkoStatus ponko_world_render_layers(struct PonkoWorld *world,
                                           uint64_t entity,
                                           uint32_t *out_layers);

enum PonkoStatus ponko_view3d_defaults(struct PonkoView3D *view);

enum PonkoStatus ponko_ctx_camera3d_view(struct PonkoCtx *ctx, struct PonkoView3D *out_view);

enum PonkoStatus ponko_ctx_set_camera3d_view(struct PonkoCtx *ctx, const struct PonkoView3D *view);

enum PonkoStatus ponko_ctx_view3d_count(struct PonkoCtx *ctx, size_t *out_count);

enum PonkoStatus ponko_ctx_view3d(struct PonkoCtx *ctx, size_t index, struct PonkoView3D *out_view);

enum PonkoStatus ponko_ctx_set_view3d(struct PonkoCtx *ctx,
                                      size_t index,
                                      const struct PonkoView3D *view);

enum PonkoStatus ponko_ctx_remove_view3d(struct PonkoCtx *ctx, size_t index);

enum PonkoStatus ponko_ctx_camera3d_at(struct PonkoCtx *ctx,
                                       struct PonkoVec2 screen,
                                       int32_t *out_view);

enum PonkoStatus ponko_ctx_screen_to_ground_at(struct PonkoCtx *ctx,
                                               struct PonkoVec2 screen,
                                               float height,
                                               bool *out_found,
                                               struct PonkoVec3 *out_point);

enum PonkoStatus ponko_ctx_feed_touch(struct PonkoCtx *ctx,
                                      uint64_t id,
                                      uint32_t phase,
                                      struct PonkoVec2 position);

enum PonkoStatus ponko_ctx_touches(struct PonkoCtx *ctx,
                                   struct PonkoTouch *out_touches,
                                   size_t capacity,
                                   size_t *out_count);

enum PonkoStatus ponko_ctx_gesture_events(struct PonkoCtx *ctx,
                                          struct PonkoGestureEvent *out_events,
                                          size_t capacity,
                                          size_t *out_count);

enum PonkoStatus ponko_ctx_gesture_deltas(struct PonkoCtx *ctx,
                                          struct PonkoVec2 *out_drag,
                                          float *out_pinch_scale,
                                          float *out_rotation);

enum PonkoStatus ponko_ctx_touch_device(struct PonkoCtx *ctx, bool *out_touch);

enum PonkoStatus ponko_ctx_set_touch_device(struct PonkoCtx *ctx, bool touch);

enum PonkoStatus ponko_ctx_gesture_settings(struct PonkoCtx *ctx,
                                            struct PonkoGestureSettings *out_settings);

enum PonkoStatus ponko_ctx_set_gesture_settings(struct PonkoCtx *ctx,
                                                const struct PonkoGestureSettings *settings);

enum PonkoStatus ponko_ctx_set_touch_controls(struct PonkoCtx *ctx,
                                              const struct PonkoTouchControl *controls,
                                              size_t count,
                                              uint32_t show,
                                              bool over_screens);

enum PonkoStatus ponko_ctx_remove_touch_controls(struct PonkoCtx *ctx);

enum PonkoStatus ponko_ctx_set_touch_controls_show(struct PonkoCtx *ctx, uint32_t show);

enum PonkoStatus ponko_ctx_set_touch_control_shown(struct PonkoCtx *ctx,
                                                   const char *action,
                                                   bool shown);

enum PonkoStatus ponko_ctx_touch_control_center(struct PonkoCtx *ctx,
                                                const char *action,
                                                struct PonkoVec2 *out_center);

enum PonkoStatus ponko_input_scenario_touch(struct PonkoInputScenario *scenario,
                                            uint64_t tick,
                                            uint64_t id,
                                            uint32_t phase,
                                            struct PonkoVec2 position);

enum PonkoStatus ponko_water_desc_lake(struct PonkoWaterDesc *desc,
                                       struct PonkoVec2 center,
                                       struct PonkoVec2 size,
                                       float level);

enum PonkoStatus ponko_water_desc_river(struct PonkoWaterDesc *desc,
                                        struct PonkoVec2 center,
                                        struct PonkoVec2 size,
                                        float level,
                                        struct PonkoVec2 flow);

enum PonkoStatus ponko_water_desc_sea(struct PonkoWaterDesc *desc,
                                      float level,
                                      struct PonkoVec2 wind,
                                      float height);

enum PonkoStatus ponko_game_water(struct PonkoGame *game);

enum PonkoStatus ponko_ctx_spawn_water(struct PonkoCtx *ctx,
                                       const struct PonkoWaterDesc *desc,
                                       uint64_t *out_entity);

enum PonkoStatus ponko_ctx_despawn_water(struct PonkoCtx *ctx, uint64_t entity);

enum PonkoStatus ponko_ctx_water_sample(const struct PonkoCtx *ctx,
                                        uint64_t entity,
                                        struct PonkoVec2 at,
                                        float t,
                                        struct PonkoWaterSample *out_sample);

enum PonkoStatus ponko_ctx_water_touch(struct PonkoCtx *ctx,
                                       uint64_t entity,
                                       struct PonkoVec2 at,
                                       float radius,
                                       float strength);

enum PonkoStatus ponko_ctx_set_buoyancy_box(struct PonkoCtx *ctx,
                                            uint64_t entity,
                                            struct PonkoVec3 size,
                                            float drag);

enum PonkoStatus ponko_ctx_set_buoyancy_sphere(struct PonkoCtx *ctx,
                                               uint64_t entity,
                                               float radius,
                                               float drag);

enum PonkoStatus ponko_ctx_remove_buoyancy(struct PonkoCtx *ctx, uint64_t entity);

enum PonkoStatus ponko_ctx_buoyancy_submerged(const struct PonkoCtx *ctx,
                                              uint64_t entity,
                                              float *out_share);

enum PonkoStatus ponko_component_on_add(struct PonkoWorld *world,
                                        uint32_t component,
                                        PonkoHookFn hook,
                                        void *user,
                                        PonkoFreeUserFn free_user);

enum PonkoStatus ponko_component_on_replace(struct PonkoWorld *world,
                                            uint32_t component,
                                            PonkoHookFn hook,
                                            void *user,
                                            PonkoFreeUserFn free_user);

enum PonkoStatus ponko_component_on_remove(struct PonkoWorld *world,
                                           uint32_t component,
                                           PonkoHookFn hook,
                                           void *user,
                                           PonkoFreeUserFn free_user);

enum PonkoStatus ponko_hook_get(struct PonkoHookWorld *hw,
                                uint64_t entity,
                                uint32_t component,
                                void *out_data,
                                size_t size);

enum PonkoStatus ponko_hook_write(struct PonkoHookWorld *hw,
                                  uint64_t entity,
                                  uint32_t component,
                                  const void *data,
                                  size_t size);

enum PonkoStatus ponko_hook_spawn_later(struct PonkoHookWorld *hw,
                                        const struct PonkoValue *values,
                                        size_t count,
                                        uint64_t *out_entity);

enum PonkoStatus ponko_hook_set_later(struct PonkoHookWorld *hw,
                                      uint64_t entity,
                                      const struct PonkoValue *value);

enum PonkoStatus ponko_hook_remove_later(struct PonkoHookWorld *hw,
                                         uint64_t entity,
                                         uint32_t component);

enum PonkoStatus ponko_hook_despawn_later(struct PonkoHookWorld *hw, uint64_t entity);

enum PonkoStatus ponko_hook_is_alive(struct PonkoHookWorld *hw, uint64_t entity, bool *out_alive);

enum PonkoStatus ponko_ctx_create_clip(struct PonkoCtx *ctx,
                                       uint64_t skeleton_entity,
                                       const char *name,
                                       const struct PonkoJointTrack *tracks,
                                       size_t track_count,
                                       struct PonkoClip **out_clip);

enum PonkoStatus ponko_ctx_live_counts_ex(struct PonkoCtx *ctx,
                                          struct PonkoLiveCountsEx *out_counts);

enum PonkoStatus ponko_mesh_data_generate_lightmap_uvs(struct PonkoMeshData *data,
                                                       float texels_per_unit,
                                                       uint32_t *out_side);

enum PonkoStatus ponko_light_bake_settings_defaults(struct PonkoLightBakeSettings *settings);

enum PonkoStatus ponko_bake_model_defaults(struct PonkoBakeModel *model);

enum PonkoStatus ponko_ctx_light_bake_scene_new(struct PonkoCtx *ctx,
                                                struct PonkoLightBakeScene **out_scene);

void ponko_light_bake_scene_free(struct PonkoLightBakeScene *scene);

enum PonkoStatus ponko_light_bake_scene_add(struct PonkoLightBakeScene *scene,
                                            const struct PonkoBakeModel *model,
                                            size_t *out_index);

enum PonkoStatus ponko_light_bake_scene_add_point_light(struct PonkoLightBakeScene *scene,
                                                        struct PonkoVec3 position,
                                                        const struct PonkoPointLight3D *light);

enum PonkoStatus ponko_light_bake_scene_add_spot_light(struct PonkoLightBakeScene *scene,
                                                       struct PonkoVec3 position,
                                                       struct PonkoVec3 forward,
                                                       const struct PonkoSpotLight3D *light);

enum PonkoStatus ponko_ctx_light_bake_scene_add_lights(struct PonkoCtx *ctx,
                                                       struct PonkoLightBakeScene *scene);

enum PonkoStatus ponko_bake_lighting(const struct PonkoLightBakeScene *scene,
                                     const struct PonkoLightBakeSettings *settings,
                                     struct PonkoBakedLighting **out_baked);

void ponko_baked_lighting_free(struct PonkoBakedLighting *baked);

enum PonkoStatus ponko_baked_lighting_to_bytes(const struct PonkoBakedLighting *baked,
                                               uint8_t *buffer,
                                               size_t capacity,
                                               size_t *out_len);

enum PonkoStatus ponko_baked_lighting_from_bytes(const void *data,
                                                 size_t size,
                                                 struct PonkoBakedLighting **out_baked);

enum PonkoStatus ponko_ctx_load_baked_lighting(struct PonkoCtx *ctx,
                                               const char *path,
                                               struct PonkoBakedLighting **out_baked);

enum PonkoStatus ponko_ctx_set_baked_lighting(struct PonkoCtx *ctx,
                                              const struct PonkoBakedLighting *baked);

enum PonkoStatus ponko_baked_lighting_info(const struct PonkoBakedLighting *baked,
                                           struct PonkoBakedLightingInfo *out_info);

enum PonkoStatus ponko_baked_lighting_lightmap(const struct PonkoBakedLighting *baked,
                                               size_t index,
                                               struct PonkoLightmapRect *out_rect,
                                               struct PonkoLightmapped *out_lightmapped);

enum PonkoStatus ponko_baked_lighting_texel(const struct PonkoBakedLighting *baked,
                                            uint32_t page,
                                            uint32_t x,
                                            uint32_t y,
                                            struct PonkoVec3 *out_light,
                                            bool *out_baked);

enum PonkoStatus ponko_baked_lighting_probe_light(const struct PonkoBakedLighting *baked,
                                                  struct PonkoVec3 position,
                                                  struct PonkoVec3 normal,
                                                  struct PonkoVec3 *out_light);

enum PonkoStatus ponko_lightmapped_component(struct PonkoWorld *world, uint32_t *out_id);

enum PonkoStatus ponko_ctx_nav_view(struct PonkoCtx *ctx, struct PonkoNavView *out_view);

enum PonkoStatus ponko_nav_view_sample_position(const struct PonkoNavView *view,
                                                struct PonkoVec3 point,
                                                float max_distance,
                                                struct PonkoVec3 *out_point,
                                                bool *out_found);

enum PonkoStatus ponko_nav_view_sample_positions(const struct PonkoNavView *view,
                                                 const struct PonkoVec3 *points,
                                                 size_t count,
                                                 float max_distance,
                                                 struct PonkoVec3 *out_points,
                                                 bool *out_found);

enum PonkoStatus ponko_nav_view_find_path(const struct PonkoNavView *view,
                                          struct PonkoVec3 from,
                                          struct PonkoVec3 to,
                                          struct PonkoVec3 *out_corners,
                                          size_t capacity,
                                          size_t *out_len);

enum PonkoStatus ponko_ctx_replication_input(struct PonkoCtx *ctx,
                                             const char *name,
                                             size_t size,
                                             const struct PonkoReplicatedField *fields,
                                             size_t field_count);

enum PonkoStatus ponko_ctx_replication_predicted(struct PonkoCtx *ctx,
                                                 const char *component,
                                                 PonkoPredictFn step,
                                                 void *user,
                                                 PonkoFreeUserFn free_user);

enum PonkoStatus ponko_ctx_replication_interpolated(struct PonkoCtx *ctx,
                                                    const char *component,
                                                    const struct PonkoSnapshotConfig *config);

enum PonkoStatus ponko_ctx_replication_event(struct PonkoCtx *ctx,
                                             const char *name,
                                             uint16_t *out_kind);

enum PonkoStatus ponko_ctx_replication_position(struct PonkoCtx *ctx,
                                                uint32_t component,
                                                size_t offset,
                                                uint32_t dims);

enum PonkoStatus ponko_ctx_set_replication_input(struct PonkoCtx *ctx,
                                                 const void *data,
                                                 size_t size);

enum PonkoStatus ponko_ctx_replication_input_of(struct PonkoCtx *ctx,
                                                uint64_t peer,
                                                void *out_data,
                                                size_t size);

enum PonkoStatus ponko_ctx_replication_send(struct PonkoCtx *ctx,
                                            uint32_t target,
                                            uint64_t peer,
                                            uint16_t kind,
                                            const void *data,
                                            size_t size);

enum PonkoStatus ponko_ctx_net_events(struct PonkoCtx *ctx,
                                      const struct PonkoNetEvent **out_events,
                                      size_t *out_count);

enum PonkoStatus ponko_input_scenario_tap(struct PonkoInputScenario *scenario,
                                          uint64_t tick,
                                          struct PonkoVec2 at);

enum PonkoStatus ponko_input_scenario_touch_hold(struct PonkoInputScenario *scenario,
                                                 uint64_t from,
                                                 uint64_t to,
                                                 struct PonkoVec2 at);

enum PonkoStatus ponko_input_scenario_touch_drag(struct PonkoInputScenario *scenario,
                                                 uint64_t from,
                                                 uint64_t to,
                                                 struct PonkoVec2 start,
                                                 struct PonkoVec2 end);

enum PonkoStatus ponko_sky_kit_defaults(struct PonkoSkyKit *kit);

enum PonkoStatus ponko_ctx_sky_kit(struct PonkoCtx *ctx, struct PonkoSkyKit *out_kit);

enum PonkoStatus ponko_ctx_set_sky_kit(struct PonkoCtx *ctx, const struct PonkoSkyKit *kit);

float ponko_moon_illumination(float phase);

enum PonkoStatus ponko_day_night_new(float hour, struct PonkoDayNight *out_day);

enum PonkoStatus ponko_day_night_advance(struct PonkoDayNight *day, float seconds);

enum PonkoStatus ponko_day_night_moon_phase(const struct PonkoDayNight *day, float *out_phase);

enum PonkoStatus ponko_ctx_apply_day_night(struct PonkoCtx *ctx, const struct PonkoDayNight *day);

enum PonkoStatus ponko_ctx_update_day_night(struct PonkoCtx *ctx, struct PonkoDayNight *day);

enum PonkoStatus ponko_ctx_load_video(struct PonkoCtx *ctx,
                                      const char *path,
                                      struct PonkoVideo **out_video);

enum PonkoStatus ponko_ctx_video_from_bytes(struct PonkoCtx *ctx,
                                            const void *data,
                                            size_t size,
                                            struct PonkoVideo **out_video);

enum PonkoStatus ponko_ctx_free_video(struct PonkoCtx *ctx, struct PonkoVideo *video);

enum PonkoStatus ponko_video_state(const struct PonkoVideo *video,
                                   struct PonkoVideoState *out_state);

enum PonkoStatus ponko_video_frame_rgba(const struct PonkoVideo *video,
                                        const uint8_t **out_pixels,
                                        size_t *out_size);

enum PonkoStatus ponko_video_set_looping(struct PonkoVideo *video, bool looping);

enum PonkoStatus ponko_ctx_video_play(struct PonkoCtx *ctx, struct PonkoVideo *video);

enum PonkoStatus ponko_ctx_video_pause(struct PonkoCtx *ctx, struct PonkoVideo *video);

enum PonkoStatus ponko_ctx_video_seek(struct PonkoCtx *ctx,
                                      struct PonkoVideo *video,
                                      float seconds);

enum PonkoStatus ponko_ctx_video_update(struct PonkoCtx *ctx, struct PonkoVideo *video);

enum PonkoStatus ponko_ctx_video_advance(struct PonkoCtx *ctx,
                                         struct PonkoVideo *video,
                                         float seconds);

enum PonkoStatus ponko_ctx_video_set_volume(struct PonkoCtx *ctx,
                                            struct PonkoVideo *video,
                                            float volume);

enum PonkoStatus ponko_ctx_video_set_bus(struct PonkoCtx *ctx,
                                         struct PonkoVideo *video,
                                         struct PonkoAudioBus bus);

enum PonkoStatus ponko_window_video_mode_ex_defaults(struct PonkoWindowVideoModeEx *desc);

enum PonkoStatus ponko_window_mode_ex_defaults(struct PonkoWindowModeEx *desc);

enum PonkoStatus ponko_window_properties_ex_defaults(struct PonkoWindowPropertiesEx *desc);

enum PonkoStatus ponko_window_image_ex_defaults(struct PonkoWindowImageEx *desc);

enum PonkoStatus ponko_window_monitor_ex_defaults(struct PonkoWindowMonitorEx *desc);

enum PonkoStatus ponko_window_state_ex_defaults(struct PonkoWindowStateEx *desc);

enum PonkoStatus ponko_window_event_ex_defaults(struct PonkoWindowEventEx *desc);

enum PonkoStatus ponko_ctx_window_mode_ex(struct PonkoCtx *ctx, struct PonkoWindowModeEx *desc);

enum PonkoStatus ponko_ctx_window_set_mode_ex(struct PonkoCtx *ctx,
                                              const struct PonkoWindowModeEx *desc);

enum PonkoStatus ponko_ctx_window_properties_ex(struct PonkoCtx *ctx,
                                                struct PonkoWindowPropertiesEx *desc);

enum PonkoStatus ponko_ctx_window_set_title(struct PonkoCtx *ctx, const char *value);

enum PonkoStatus ponko_ctx_window_set_size(struct PonkoCtx *ctx, struct PonkoVec2 size);

enum PonkoStatus ponko_ctx_window_set_position(struct PonkoCtx *ctx, int32_t x, int32_t y);

enum PonkoStatus ponko_ctx_window_set_resize_limits(struct PonkoCtx *ctx,
                                                    const struct PonkoVec2 *min,
                                                    const struct PonkoVec2 *max);

enum PonkoStatus ponko_ctx_window_set_always_on_top(struct PonkoCtx *ctx, bool value);

enum PonkoStatus ponko_ctx_window_set_decorations(struct PonkoCtx *ctx, bool value);

enum PonkoStatus ponko_ctx_window_set_icon_ex(struct PonkoCtx *ctx,
                                              const struct PonkoWindowImageEx *desc);

enum PonkoStatus ponko_ctx_window_set_cursor_icon(struct PonkoCtx *ctx, uint32_t icon);

enum PonkoStatus ponko_ctx_window_set_cursor_image_ex(struct PonkoCtx *ctx,
                                                      const struct PonkoWindowImageEx *desc);

enum PonkoStatus ponko_ctx_window_set_cursor_visible(struct PonkoCtx *ctx, bool value);

enum PonkoStatus ponko_ctx_window_set_cursor_grab(struct PonkoCtx *ctx, uint32_t grab);

enum PonkoStatus ponko_ctx_window_close_requested(struct PonkoCtx *ctx, bool *value);

enum PonkoStatus ponko_ctx_window_cancel_close(struct PonkoCtx *ctx);

enum PonkoStatus ponko_ctx_window_save_settings(struct PonkoCtx *ctx);

enum PonkoStatus ponko_ctx_window_restore_settings(struct PonkoCtx *ctx);

enum PonkoStatus ponko_ctx_window_monitor_count(struct PonkoCtx *ctx, size_t *count);

enum PonkoStatus ponko_ctx_window_video_mode_count(struct PonkoCtx *ctx,
                                                   uint32_t monitor,
                                                   size_t *count);

enum PonkoStatus ponko_ctx_window_event_count(struct PonkoCtx *ctx, size_t *count);

enum PonkoStatus ponko_ctx_window_title(struct PonkoCtx *ctx,
                                        char *buffer,
                                        size_t capacity,
                                        size_t *length);

enum PonkoStatus ponko_ctx_window_monitor_ex(struct PonkoCtx *ctx,
                                             uint32_t index,
                                             struct PonkoWindowMonitorEx *desc);

enum PonkoStatus ponko_ctx_window_monitor_name(struct PonkoCtx *ctx,
                                               uint32_t index,
                                               char *buffer,
                                               size_t capacity,
                                               size_t *length);

enum PonkoStatus ponko_ctx_window_video_mode_ex(struct PonkoCtx *ctx,
                                                uint32_t monitor,
                                                uint32_t index,
                                                struct PonkoWindowVideoModeEx *desc);

enum PonkoStatus ponko_ctx_window_state_ex(struct PonkoCtx *ctx, struct PonkoWindowStateEx *desc);

enum PonkoStatus ponko_ctx_window_event_ex(struct PonkoCtx *ctx,
                                           size_t index,
                                           struct PonkoWindowEventEx *desc,
                                           char *path,
                                           size_t capacity,
                                           size_t *length);

enum PonkoStatus ponko_ctx_clipboard_write(struct PonkoCtx *ctx, const char *value);

enum PonkoStatus ponko_ctx_clipboard_read(struct PonkoCtx *ctx,
                                          char *buffer,
                                          size_t capacity,
                                          size_t *length,
                                          bool *available);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* PONKO_H */
