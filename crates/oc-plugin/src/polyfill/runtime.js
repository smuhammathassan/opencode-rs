// oc-plugin in-process JS runtime.
//
// This file is evaluated once per QuickJS context. It provides:
//  - the module registry + __oc_require/__oc_define for transpiled ESM
//  - the @opencode-ai/plugin API polyfill (v1.18.13 surface)
//  - import shims for opencode/plugin, opencode/plugin/tool, opencode/plugin/shell,
//    opencode/plugin/tui, node:* modules, zod and fetch
//  - the bridge protocol between JS and the Rust host (__oc_bridge_sync)
//  - the plugin load / hook trigger / tool execution entrypoints the host calls
//
// All cross-boundary data is JSON strings.

"use strict";

// ---------------------------------------------------------------------------
// Bridge to the Rust host
// ---------------------------------------------------------------------------

function __oc_bridge_sync(method, payload) {
  // __oc_host_bridge is installed by Rust as a callback.
  const result = __oc_host_bridge(method, JSON.stringify(payload === undefined ? null : payload));
  const parsed = JSON.parse(result);
  if (parsed && typeof parsed === "object" && parsed.__error) {
    throw new Error(parsed.__error);
  }
  return parsed;
}

// Synchronous entrypoint used by the host: call a global JS function with a
// JSON payload and return JSON.stringify(result).
function __oc_call_json(name, payload) {
  const fn = globalThis[name];
  if (typeof fn !== "function") throw new Error("__oc_call_json: no function " + name);
  const result = fn(JSON.parse(payload));
  return JSON.stringify(result === undefined ? null : result);
}

function __oc_read_global(name) {
  const value = globalThis[name];
  return value === undefined ? "null" : JSON.stringify(value);
}

function __oc_set_global(name, valueJson) {
  globalThis[name] = JSON.parse(valueJson);
}

// Promise entrypoint: run a JS function with a JSON payload; the host pumps
// the job queue until this global holds a result. Keep this dispatcher as an
// ordinary function. QuickJS's embedded async-function implementation is
// fragile when resumed across the Rust FFI boundary, while ordinary Promise
// reactions provide the same completion semantics without retaining an
// async-function activation record in the host call frame.
function __oc_async_call(name, payload) {
  const fn = globalThis[name];
  try {
    const result = fn(JSON.parse(payload));
    if (result && typeof result.then === "function") {
      result.then(function (value) {
        __oc_pending = JSON.stringify({ ok: true, value: value === undefined ? null : value });
      }, function (err) {
        __oc_pending = JSON.stringify({ ok: false, error: { message: errorMessage(err) } });
      });
    } else {
      __oc_pending = JSON.stringify({ ok: true, value: result === undefined ? null : result });
    }
  } catch (err) {
    __oc_pending = JSON.stringify({ ok: false, error: { message: errorMessage(err) } });
  }
  return true;
}

function errorMessage(err) {
  if (err && typeof err === "object" && typeof err.message === "string") return err.message;
  return String(err);
}

// Streams keep their callbacks inside QuickJS. Rust only sends serialized
// events back to the owner thread through `__oc_stream_emit`, which avoids
// ever moving a JS function across the FFI boundary.
//
// Backpressure: each stream owns a bounded pending-event queue. The Rust host
// also bounds its cross-thread stream channel (see PluginManager), so an
// overloaded plugin cannot make the server event fan-out grow without bound.
const __oc_streams = Object.create(null);
let __oc_stream_sequence = 0;
const __OC_STREAM_MAX_PENDING = 1024;

function __oc_stream_matches(stream, event) {
  if (!stream.path || stream.path === "/global/event") return true;
  if (!event || typeof event !== "object") return false;
  return event.type === stream.path || event.event === stream.path;
}

function __oc_stream_emit(event) {
  const pending = [];
  Object.keys(__oc_streams).forEach(function (id) {
    const stream = __oc_streams[id];
    if (!stream || stream.closed || !__oc_stream_matches(stream, event)) return;
    // Apply the bounded pending queue: an event beyond the cap evicts the
    // oldest undelivered event and is counted on the stream's stats.
    stream.pending.push(event);
    if (stream.pending.length > stream.maxPending) {
      stream.pending.shift();
      stream.dropped++;
    }
    stream.handlers.slice().forEach(function (handler) {
      pending.push(Promise.resolve().then(function () { return handler(event); }).catch(function () {
        // A subscriber must not stop delivery to other plugin streams.
        return undefined;
      }));
    });
    stream.pending = [];
  });
  return Promise.all(pending).then(function () { return true; });
}

function __oc_make_sse_stream(path) {
  const id = "sse_" + (++__oc_stream_sequence);
  const stream = {
    id: id,
    path: typeof path === "string" ? path : "/global/event",
    handlers: [],
    pending: [],
    maxPending: __OC_STREAM_MAX_PENDING,
    dropped: 0,
    closed: false,
    on: function (eventOrHandler, maybeHandler) {
      const handler = typeof eventOrHandler === "function" ? eventOrHandler : maybeHandler;
      if (typeof handler !== "function") throw new TypeError("stream handler must be a function");
      if (!stream.closed) stream.handlers.push(handler);
      return stream;
    },
    off: function (eventOrHandler, maybeHandler) {
      const handler = typeof eventOrHandler === "function" ? eventOrHandler : maybeHandler;
      stream.handlers = stream.handlers.filter(function (candidate) { return candidate !== handler; });
      return stream;
    },
    done: function () {
      stream.closed = true;
      stream.handlers = [];
      stream.pending = [];
      delete __oc_streams[id];
      return Promise.resolve(true);
    },
    // Backpressure introspection for hosts and tests.
    backpressure: function () {
      return { pending: stream.pending.length, dropped: stream.dropped, max: stream.maxPending };
    },
  };
  __oc_streams[id] = stream;
  return stream;
}

// ---------------------------------------------------------------------------
// Module registry for transpiled ESM
// ---------------------------------------------------------------------------

const __oc_modules = Object.create(null);
let __oc_current_exports = null;
let __oc_main_exports = null;

function __oc_define(name, value) {
  const target = __oc_current_exports || __oc_modules;
  target[name] = value;
}

function __oc_export_all(mod) {
  const target = __oc_current_exports || __oc_modules;
  for (const key of Object.keys(mod)) {
    if (key === "default") continue;
    target[key] = mod[key];
  }
}

function __oc_resolve_spec(spec) {
  if (Object.prototype.hasOwnProperty.call(__oc_modules, spec)) return __oc_modules[spec];
  return undefined;
}

const __oc_file_cache = new Map();

function __oc_require(spec) {
  if (spec === "opencode/plugin" || spec === "@opencode-ai/plugin") return __oc_modules["opencode/plugin"];
  if (spec === "opencode/plugin/tool" || spec === "@opencode-ai/plugin/tool") return __oc_modules["opencode/plugin/tool"];
  if (spec === "opencode/plugin/shell" || spec === "@opencode-ai/plugin/shell") return __oc_modules["opencode/plugin/shell"];
  if (spec === "opencode/plugin/tui" || spec === "@opencode-ai/plugin/tui") return __oc_modules["opencode/plugin/tui"];
  if (spec === "opencode/plugin/v2/effect" || spec === "@opencode-ai/plugin/v2/effect") return __oc_modules["opencode/plugin/v2/effect"];
  if (spec === "opencode/plugin/v2/effect/integration" || spec === "@opencode-ai/plugin/v2/effect/integration")
    return __oc_modules["opencode/plugin/v2/effect/integration"];
  if (spec === "opencode/plugin/v2/effect/plugin" || spec === "@opencode-ai/plugin/v2/effect/plugin")
    return __oc_modules["opencode/plugin/v2/effect/plugin"];
  if (spec === "opencode/plugin/v2/promise" || spec === "@opencode-ai/plugin/v2/promise")
    return __oc_modules["opencode/plugin/v2/promise"];
  if (spec === "zod") return __oc_modules["zod"];
  if (spec.startsWith("node:")) return __oc_modules[spec];
  const hit = __oc_resolve_spec(spec);
  if (hit !== undefined) return hit;
  if (__oc_file_cache.has(spec)) return __oc_file_cache.get(spec);

  // Relative / absolute local file: ask the host for the transpiled source.
  const resolved = __oc_bridge_sync("resolve", { spec: spec });
  if (resolved && resolved.kind === "inline") {
    const exports = __oc_eval_module(resolved.code, resolved.path);
    __oc_file_cache.set(spec, exports);
    return exports;
  }
  if (resolved && resolved.kind === "path") {
    const exports = __oc_eval_module_path(resolved.path);
    __oc_file_cache.set(spec, exports);
    return exports;
  }
  throw new Error("Cannot find module '" + spec + "'");
}

function __oc_import(spec) {
  return Promise.resolve().then(function () {
    return __oc_require(spec);
  });
}

function __oc_eval_module(code, filename) {
  const exports = Object.create(null);
  const previous = __oc_current_exports;
  __oc_current_exports = exports;
  try {
    const fn = new Function(code);
    fn();
  } finally {
    __oc_current_exports = previous;
  }
  return exports;
}

// Evaluate the plugin's main entry module. Its exports are captured into
// `__oc_main_exports` and consumed by __oc_load_plugin.
function __oc_eval_main(code, filename) {
  const exports = Object.create(null);
  const previous = __oc_current_exports;
  __oc_current_exports = exports;
  __oc_main_exports = exports;
  try {
    const fn = new Function(code);
    fn();
  } finally {
    __oc_current_exports = previous;
  }
  return exports;
}

function __oc_eval_module_path(path) {
  const resolved = __oc_bridge_sync("read", { path: path });
  if (!resolved || resolved.kind !== "inline") {
    throw new Error("Cannot read module '" + path + "'");
  }
  return __oc_eval_module(resolved.code, path);
}

// ---------------------------------------------------------------------------
// console, timers, microtasks
// ---------------------------------------------------------------------------

if (typeof console === "undefined") {
  globalThis.console = {
    log: function () { __oc_bridge_sync("log", { level: "info", args: Array.prototype.map.call(arguments, formatArg) }); },
    info: function () { __oc_bridge_sync("log", { level: "info", args: Array.prototype.map.call(arguments, formatArg) }); },
    warn: function () { __oc_bridge_sync("log", { level: "warn", args: Array.prototype.map.call(arguments, formatArg) }); },
    error: function () { __oc_bridge_sync("log", { level: "error", args: Array.prototype.map.call(arguments, formatArg) }); },
    debug: function () { __oc_bridge_sync("log", { level: "debug", args: Array.prototype.map.call(arguments, formatArg) }); },
  };
}

function formatArg(arg) {
  if (typeof arg === "string") return arg;
  try {
    return JSON.stringify(arg);
  } catch (e) {
    return String(arg);
  }
}

if (typeof queueMicrotask === "undefined") {
  globalThis.queueMicrotask = function (fn) {
    Promise.resolve().then(fn);
  };
}

// Timers are not backed by a real event loop. setTimeout/setInterval callbacks
// are scheduled as microtasks with the delay ignored; this keeps plugins that
// use timers for debouncing working while documenting that wall-clock delays
// are not honored. Flags: quick-js limitation.
if (typeof setTimeout === "undefined") {
  globalThis.setTimeout = function (fn) {
    Promise.resolve().then(function () { fn(); });
    return 0;
  };
  globalThis.clearTimeout = function () {};
  globalThis.setInterval = function (fn) {
    Promise.resolve().then(function () { fn(); });
    return 0;
  };
  globalThis.clearInterval = function () {};
}

// ---------------------------------------------------------------------------
// fetch polyfill (blocking via the host bridge)
// ---------------------------------------------------------------------------

function __oc_fetch(url, options) {
  options = options || {};
  return Promise.resolve().then(function () {
    const result = __oc_bridge_sync("fetch", {
      url: String(url),
      method: options.method || "GET",
      headers: options.headers || {},
      body: options.body !== undefined && options.body !== null ? String(options.body) : null,
    });
    return __oc_make_response(result);
  });
}

function __oc_make_response(result) {
  if (!result) throw new Error("fetch failed");
  const status = result.status || 200;
  const headers = result.headers || {};
  const body = result.body === undefined ? "" : result.body;
  const response = {
    ok: status >= 200 && status < 300,
    status: status,
    statusText: status === 200 ? "OK" : "Error",
    url: result.url || "",
    headers: new __oc_Headers(headers),
    text: function () { return Promise.resolve(body); },
    json: function () { return Promise.resolve(JSON.parse(body)); },
    arrayBuffer: function () { return Promise.resolve(new Uint8Array(0).buffer); },
  };
  return response;
}

function __oc_Headers(init) {
  const map = Object.create(null);
  if (init) {
    for (const key of Object.keys(init)) map[key.toLowerCase()] = String(init[key]);
  }
  this.get = function (name) { return map[name.toLowerCase()] || null; };
  this.has = function (name) { return map[name.toLowerCase()] !== undefined; };
  this.entries = function () { return Object.entries(map); };
  this.forEach = function (cb) { for (const [k, v] of Object.entries(map)) cb(v, k, this); };
}

globalThis.fetch = __oc_fetch;

// ---------------------------------------------------------------------------
// @opencode-ai/plugin polyfill
// ---------------------------------------------------------------------------

const __oc_plugin = {
  hooks: [],
  tools: Object.create(null),
  toolKeys: Object.create(null),
  workspaceAdapters: Object.create(null),
  // Auth hook objects and returned OAuth callbacks stay in this QuickJS
  // context. Only __oc_auth_summary() crosses the Rust boundary.
  authHooks: [],
  authPending: Object.create(null),
  pluginId: null,
};

// Minimal zod subset producing JSON schema via __oc_schema_to_json_schema.
function __oc_make_zod() {
  const makeSchema = (kind, params) => new __oc_ZSchema(kind, params);
  class __oc_ZSchema {
    constructor(kind, params) {
      this._oc_kind = kind;
      this._oc_params = params || {};
      this._oc_required = !(kind === "optional" || kind === "nullable" || kind === "default");
    }
    describe(description) {
      const next = new __oc_ZSchema(this._oc_kind, this._oc_params);
      next._oc_required = this._oc_required;
      next._oc_description = description;
      return next;
    }
    optional() {
      const next = new __oc_ZSchema("optional", { inner: this });
      next._oc_required = false;
      return next;
    }
    nullable() {
      return new __oc_ZSchema("nullable", { inner: this });
    }
    default(value) {
      return new __oc_ZSchema("default", { inner: this, value });
    }
    min(value) {
      const next = new __oc_ZSchema(this._oc_kind, Object.assign({}, this._oc_params, { min: value }));
      next._oc_required = this._oc_required;
      return next;
    }
    max(value) {
      const next = new __oc_ZSchema(this._oc_kind, Object.assign({}, this._oc_params, { max: value }));
      next._oc_required = this._oc_required;
      return next;
    }
    length(value) {
      return this.min(value).max(value);
    }
    email() {
      const next = new __oc_ZSchema("string", Object.assign({}, this._oc_params, { format: "email" }));
      next._oc_required = this._oc_required;
      return next;
    }
    url() {
      const next = new __oc_ZSchema("string", Object.assign({}, this._oc_params, { format: "uri" }));
      next._oc_required = this._oc_required;
      return next;
    }
    regex(pattern) {
      const next = new __oc_ZSchema("string", Object.assign({}, this._oc_params, { pattern: String(pattern) }));
      next._oc_required = this._oc_required;
      return next;
    }
    enum(values) {
      return new __oc_ZSchema("enum", { values: Array.isArray(values) ? values : Array.from(arguments) });
    }
    array() {
      return new __oc_ZSchema("array", { item: makeSchema("any", {}) });
    }
    element(item) {
      return new __oc_ZSchema("array", { item });
    }
    int() {
      return new __oc_ZSchema("integer", {});
    }
    step() {
      return this;
    }
  }

  const string = (params) => makeSchema("string", params || {});
  string.min = () => string();
  string.email = () => makeSchema("string", { format: "email" });
  string.url = () => makeSchema("string", { format: "uri" });
  string.regex = () => string();
  string.datetime = () => string();

  return {
    string,
    number: (params) => makeSchema("number", params || {}),
    boolean: () => makeSchema("boolean", {}),
    bigint: () => makeSchema("string", {}),
    integer: (params) => makeSchema("integer", params || {}),
    any: () => makeSchema("any", {}),
    unknown: () => makeSchema("any", {}),
    never: () => makeSchema("never", {}),
    void: () => makeSchema("null", {}),
    null: () => makeSchema("null", {}),
    undefined: () => makeSchema("undefined", {}),
    literal: (value) => makeSchema("literal", { value }),
    object: (shape) => makeSchema("object", { shape: shape || {} }),
    array: (item) => makeSchema("array", { item: item || makeSchema("any", {}) }),
    enum: (values) => new __oc_ZSchema("enum", { values: Array.isArray(values) ? values : Array.from(arguments) }),
    record: (key, value) => makeSchema("record", { key: key || makeSchema("string", {}), value: value || makeSchema("any", {}) }),
    union: (items) => makeSchema("union", { items }),
    discriminatedUnion: (discriminator, options) => makeSchema("union", { options, discriminator }),
    tuple: (items) => makeSchema("tuple", { items }),
    date: () => makeSchema("date", {}),
    instanceOf: () => makeSchema("any", {}),
    promise: (inner) => makeSchema("promise", { inner }),
    ZodError: class ZodError extends Error {},
    default: {},
  };
}

const z = __oc_make_zod();
__oc_modules["zod"] = { default: z, z: z };

function zschema_to_json(schema) {
  if (!schema || typeof schema._oc_kind !== "string") {
    // Plain value (not a zod schema): accept anything.
    return { type: "string" };
  }
  const kind = schema._oc_kind;
  const params = schema._oc_params || {};
  let out;
  switch (kind) {
    case "string": out = { type: "string" }; break;
    case "number": out = { type: "number" }; break;
    case "integer": out = { type: "integer" }; break;
    case "boolean": out = { type: "boolean" }; break;
    case "null": out = { type: "null" }; break;
    case "undefined": out = {}; break;
    case "any": out = {}; break;
    case "never": out = { not: {} }; break;
    case "date": out = { type: "string", format: "date-time" }; break;
    case "literal": out = { const: params.value }; break;
    case "enum": out = { enum: params.values || [] }; break;
    case "array": out = { type: "array", items: zschema_to_json(params.item || makeSchema("any", {})) }; break;
    case "object": out = __oc_schema_to_json_schema(params.shape || {}); break;
    case "record": out = { type: "object", additionalProperties: zschema_to_json(params.value || makeSchema("any", {})) }; break;
    case "tuple": out = { type: "array", items: (params.items || []).map(zschema_to_json) }; break;
    case "union": {
      const items = params.items || (params.options ? Object.values(params.options) : []);
      if (params.discriminator) {
        out = { oneOf: (params.options || []).map((item) => zschema_to_json(item)) };
      } else {
        out = { anyOf: items.map(zschema_to_json) };
      }
      break;
    }
    case "optional": out = zschema_to_json(params.inner || makeSchema("any", {})); break;
    case "nullable": {
      const inner = zschema_to_json(params.inner || makeSchema("any", {}));
      out = { anyOf: [inner, { type: "null" }] };
      break;
    }
    case "default": {
      out = zschema_to_json(params.inner || makeSchema("any", {}));
      if (params.value !== undefined) out.default = params.value;
      break;
    }
    case "promise": out = zschema_to_json(params.inner || makeSchema("any", {})); break;
    default: out = {};
  }
  if (params.min !== undefined && params.min !== null) {
    if (out.type === "string") out.minLength = params.min;
    else if (out.type === "array") out.minItems = params.min;
    else out.minimum = params.min;
  }
  if (params.max !== undefined && params.max !== null) {
    if (out.type === "string") out.maxLength = params.max;
    else if (out.type === "array") out.maxItems = params.max;
    else out.maximum = params.max;
  }
  if (params.format) out.format = params.format;
  if (params.pattern) out.pattern = params.pattern;
  if (schema._oc_description) out.description = schema._oc_description;
  return out;
}

function __oc_schema_to_json_schema(shape) {
  if (!shape || typeof shape !== "object") return { type: "object" };
  const properties = {};
  const required = [];
  for (const key of Object.keys(shape)) {
    const schema = shape[key];
    properties[key] = zschema_to_json(schema);
    if (schema && schema._oc_required !== false) required.push(key);
  }
  const out = { type: "object", properties };
  if (required.length) out.required = required;
  return out;
}

// The v1.18.13 tool() API: returns input unchanged; tool.schema = z.
function __oc_tool(input) {
  return input;
}
__oc_tool.schema = z;

__oc_modules["opencode/plugin/tool"] = {
  tool: __oc_tool,
  z: z,
  default: __oc_tool,
};

// Shell shim (`$`). No real streaming; commands run synchronously via the
// host bridge and resolve with a BunShellOutput-like object.
function __oc_make_shell() {
  let env = null;
  let cwd = null;
  let nothrow = false;
  let quiet = false;

  const shell = function (strings, ...expressions) {
    let command = "";
    for (let i = 0; i < strings.length; i++) {
      command += strings[i];
      if (i < expressions.length) {
        const value = expressions[i];
        command += value && typeof value.toString === "function" ? value.toString() : String(value);
      }
    }
    return __oc_run_shell(command);
  };

  shell.braces = function (pattern) {
    const result = __oc_bridge_sync("shell.braces", { pattern: String(pattern) });
    return result && Array.isArray(result) ? result : [String(pattern)];
  };
  shell.escape = function (input) {
    return String(input).replace(/[^A-Za-z0-9_.,-]/g, "\\$&");
  };
  shell.env = function (newEnv) {
    if (newEnv === undefined) return env;
    env = Object.assign({}, env, newEnv);
    return shell;
  };
  shell.cwd = function (newCwd) {
    if (newCwd === undefined) return cwd;
    cwd = newCwd;
    return shell;
  };
  shell.nothrow = function () {
    nothrow = true;
    return shell;
  };
  shell.throws = function (shouldThrow) {
    nothrow = !shouldThrow;
    return shell;
  };

  function __oc_run_shell(command) {
    const result = __oc_bridge_sync("shell.exec", {
      command: command,
      cwd: cwd,
      env: env,
      nothrow: nothrow,
      quiet: quiet,
    }) || { stdout: "", stderr: "", exitCode: 0 };
    const output = __oc_make_shell_output(result);
    const promise = Promise.resolve().then(function () { return output; });
    promise.cwd = function () { return promise; };
    promise.env = function () { return promise; };
    promise.quiet = function () { quiet = true; return promise; };
    promise.nothrow = function () { return promise; };
    promise.throws = function () { return promise; };
    promise.text = function () { quiet = true; return Promise.resolve(result.stdout); };
    promise.json = function () { quiet = true; try { return Promise.resolve(JSON.parse(result.stdout)); } catch (e) { return Promise.reject(e); } };
    promise.arrayBuffer = function () { quiet = true; return Promise.resolve(new Uint8Array(0).buffer); };
    promise.blob = function () { quiet = true; return Promise.resolve(new Blob()); };
    promise.stdin = { getWriter: function () { return { write: function () {}, close: function () {}, releaseLock: function () {} }; } };
    return promise;
  }

  return shell;
}

function __oc_make_shell_output(result) {
  const stdout = String(result.stdout === undefined ? "" : result.stdout);
  const stderr = String(result.stderr === undefined ? "" : result.stderr);
  const exitCode = result.exitCode === undefined ? 0 : result.exitCode;
  const buf = { stdout, stderr, exitCode };
  buf.text = function () { return stdout; };
  buf.json = function () { return JSON.parse(stdout); };
  buf.arrayBuffer = function () { return new Uint8Array(0).buffer; };
  buf.bytes = function () { return new Uint8Array(0); };
  buf.blob = function () { return new Blob(); };
  return buf;
}

__oc_modules["opencode/plugin/shell"] = {
  default: __oc_make_shell(),
  $: __oc_make_shell(),
};

// TUI shim: no TUI runtime in-process. Exports a no-op createBindingLookup.
function __oc_create_binding_lookup(config, options) {
  return {
    get: function () { return []; },
    has: function () { return false; },
    gather: function () { return []; },
    pick: function () { return []; },
    omit: function () { return []; },
    bindings: [],
  };
}

__oc_modules["opencode/plugin/tui"] = {
  createBindingLookup: __oc_create_binding_lookup,
  default: { createBindingLookup: __oc_create_binding_lookup },
};

// ---------------------------------------------------------------------------
// Plugin input construction
// ---------------------------------------------------------------------------

function __oc_make_client() {
  const client = {};
  const methods = [
    "pty.list", "pty.create", "pty.remove", "pty.get", "pty.update", "pty.connect",
    "session.list", "session.create", "session.get", "session.remove", "session.update",
    "session.prompt", "session.messages", "session.count", "session.event",
    "session.status",
    "session.delete", "session.children", "session.todo", "session.init",
    "session.fork", "session.abort", "session.unshare", "session.share",
    "session.diff", "session.summarize", "session.message", "session.promptAsync",
    "session.command", "session.shell", "session.revert", "session.unrevert",
    "session.permission",
    "message.create", "message.get", "message.update", "message.remove",
    "message.parts", "message.reasoning",
    "part.create", "part.get", "part.update", "part.remove", "part.meta",
    "session.data",
    "config.get", "config.set", "config.update", "config.providers",
    "project.get", "project.update", "project.list", "project.current",
    "model.get", "model.list", "provider.list", "provider.auth",
    "provider.oauth.authorize", "provider.oauth.callback",
    "tool.list", "tool.get", "tool.ids",
    "instance.dispose", "path.get", "vcs.get",
    "command.list",
    "find.text", "find.files", "find.symbols",
    "file.get", "file.list", "file.read", "file.status",
    "app.version", "app.log", "app.agents", "app.skills",
    "skill.list",
    "mcp.status", "mcp.add", "mcp.connect", "mcp.disconnect",
    "mcp.auth.remove", "mcp.auth.start", "mcp.auth.callback", "mcp.auth.authenticate",
    "lsp.status", "formatter.status",
    "global.event", "event.subscribe",
    "tui.appendPrompt", "tui.openHelp", "tui.openSessions", "tui.openThemes",
    "tui.openModels", "tui.submitPrompt", "tui.clearPrompt", "tui.executeCommand",
    "tui.showToast", "tui.publish", "tui.control.next", "tui.control.response",
    "auth.set",
    "user.get", "user.list",
    "help",
  ];
  for (const path of methods) {
    const parts = path.split(".");
    const method = parts.pop();
    const obj = parts.reduce((acc, p) => {
      if (!acc[p]) acc[p] = {};
      return acc[p];
    }, client);
    obj[method] = function (args) {
      // The official SDK methods are Promise-based. Keep the host callback
      // synchronous internally, but expose the same rejection boundary to
      // plugin code so `await client.session.get(...)` and `.catch(...)`
      // behave like the reference client.
      return __oc_client_call(path, args);
    };
  }
  // The reference SDK exposes SSE through generated `get.sse` methods. Keep
  // callbacks in QuickJS and let the owner-thread manager feed serialized
  // events through `__oc_stream_emit`.
  client.sse = { stream: __oc_make_sse_stream };
  client.global.event = function (path) { return __oc_make_sse_stream(path || "/global/event"); };
  client.session.event = function (path) { return __oc_make_sse_stream(path || "/global/event"); };
  client.event.subscribe = function (path) { return __oc_make_sse_stream(path || "/global/event"); };
  // Cancel an in-flight request by id (see `__oc_client_cancel`).
  client.cancel = __oc_client_cancel;
  return client;
}

// ---------------------------------------------------------------------------
// Client request lifecycle
// ---------------------------------------------------------------------------

// Every client call gets a monotonically increasing request id so the host can
// correlate responses when several calls are in flight and plugin code can
// cancel an individual request. The reference SDK performs the same
// correlation implicitly over HTTP; the in-process bridge needs the explicit
// id because a host may multiplex requests on its own transport.
let __oc_client_sequence = 0;
const __oc_client_inflight = Object.create(null);

function __oc_client_call(path, args) {
  const requestID = "req_" + (++__oc_client_sequence);
  // Register the request synchronously so it can be cancelled before the
  // bridge round-trip settles.
  __oc_client_inflight[requestID] = true;
  const promise = Promise.resolve().then(function () {
    if (__oc_client_inflight[requestID] === "cancelled") {
      delete __oc_client_inflight[requestID];
      const err = new Error("client " + path + " was cancelled");
      err.name = "AbortError";
      throw err;
    }
    const result = __oc_bridge_sync("client", {
      method: path,
      args: args === undefined ? null : args,
      requestID: requestID,
    });
    if (result && result.cancelled === true) {
      delete __oc_client_inflight[requestID];
      const err = new Error("client " + path + " was cancelled");
      err.name = "AbortError";
      throw err;
    }
    delete __oc_client_inflight[requestID];
    if (result && result.ok === false) {
      const err = new Error(result.error && result.error.message ? result.error.message : "client " + path + " failed");
      err.status = result.error && result.error.status;
      throw err;
    }
    return result && result.data !== undefined ? result.data : null;
  });
  // Expose the request id on the promise so plugin code can cancel the call.
  promise.requestID = requestID;
  return promise;
}

// Cancel an in-flight client request by id. The host's `client_cancel` is
// invoked for the request id and the request's promise rejects with an
// AbortError. Without an id the most recent in-flight request is cancelled.
function __oc_client_cancel(requestID) {
  const target = requestID || Object.keys(__oc_client_inflight).pop();
  if (!target || !__oc_client_inflight[target]) return false;
  __oc_client_inflight[target] = "cancelled";
  __oc_bridge_sync("client.cancel", { requestID: String(target) });
  return true;
}

// The v1 PluginInput passed to plugin functions.
function __oc_make_input(input) {
  return {
    client: __oc_make_client(),
    project: input.project || {},
    directory: input.directory || "",
    worktree: input.worktree || "",
    experimental_workspace: {
      register: function (type, adapter) {
        __oc_plugin.workspaceAdapters[type] = adapter;
      },
    },
    serverUrl: input.serverUrl || "http://localhost:4096",
    $: __oc_modules["opencode/plugin/shell"].default,
  };
}

// ---------------------------------------------------------------------------
// Hooks registry
// ---------------------------------------------------------------------------

function __oc_pick_server(exports) {
  const value = exports["default"];
  if (value !== undefined && value !== null && typeof value === "object") {
    if ("id" in value || "server" in value || "tui" in value) {
      if (typeof value.server === "function") return { fn: value.server, id: value.id };
      if (typeof value.tui === "function") return { fn: value.tui, id: value.id };
      // v2 promise/effect plugin shape: `{ id, setup(ctx) }`.
      if (typeof value.setup === "function") return { fn: value.setup, id: value.id, v2: true };
    }
  }
  if (typeof value === "function") return { fn: value };
  // legacy: first exported function or { server }
  for (const key of Object.keys(exports)) {
    const entry = exports[key];
    if (typeof entry === "function") return { fn: entry };
    if (entry && typeof entry === "object" && typeof entry.server === "function") return { fn: entry.server, id: entry.id };
  }
  return undefined;
}

function __oc_make_v2_context() {
  return {
    options: {},
    agent: __oc_v2_domain("agent"),
    command: __oc_v2_domain("command"),
    skill: __oc_v2_domain("skill"),
    catalog: __oc_v2_domain("catalog"),
    reference: __oc_v2_domain("reference"),
    integration: {
      transform: __oc_v2_domain("integration").transform,
      reload: __oc_v2_domain("integration").reload,
      connection: {
        active: function () { return Promise.resolve(null); },
        resolve: function (connection) { return Promise.resolve(connection); },
      },
    },
    aisdk: {
      sdk: function () { return { dispose: function () {} }; },
      language: function () { return { dispose: function () {} }; },
    },
    plugin: {
      add: function (input) {
        __oc_bridge_sync("v1.register", { kind: "plugin", input: input, pluginId: __oc_plugin.pluginId });
        return Promise.resolve();
      },
      remove: function (id) {
        __oc_bridge_sync("v1.register", { kind: "plugin.remove", input: { id: id }, pluginId: __oc_plugin.pluginId });
        return Promise.resolve();
      },
    },
  };
}

function __oc_register_hooks(hooks) {
  if (!hooks || typeof hooks !== "object") {
    throw new Error("Plugin must return an object of hooks");
  }
  __oc_plugin.hooks.push(hooks);
  const tools = hooks.tool;
  if (tools && typeof tools === "object") {
    for (const name of Object.keys(tools)) {
      const def = tools[name];
      if (!def) continue;
      __oc_plugin.tools[name] = def;
      __oc_plugin.toolKeys[name] = {
        description: def.description || "",
        schema: def.args ? __oc_schema_to_json_schema(def.args) : { type: "object", properties: {} },
      };
    }
  }
  if (hooks.auth && typeof hooks.auth === "object") {
    __oc_plugin.authHooks.push(hooks.auth);
  }
}

function __oc_auth_when(value) {
  if (!value || typeof value !== "object") return null;
  if (typeof value.key !== "string" || typeof value.op !== "string" || typeof value.value !== "string") return null;
  return { key: value.key, op: value.op, value: value.value };
}

function __oc_auth_prompt_summary(prompt) {
  if (!prompt || typeof prompt !== "object" || typeof prompt.type !== "string") return null;
  const when = __oc_auth_when(prompt.when);
  if (prompt.type === "text") {
    const result = {
      type: "text",
      key: String(prompt.key || ""),
      message: String(prompt.message || ""),
    };
    if (prompt.placeholder !== undefined) result.placeholder = String(prompt.placeholder);
    if (when) result.when = when;
    return result;
  }
  if (prompt.type === "select") {
    const options = Array.isArray(prompt.options) ? prompt.options.map(function (option) {
      const result = {
        label: String(option && option.label !== undefined ? option.label : ""),
        value: String(option && option.value !== undefined ? option.value : ""),
      };
      if (option && option.hint !== undefined) result.hint = String(option.hint);
      return result;
    }) : [];
    const result = {
      type: "select",
      key: String(prompt.key || ""),
      message: String(prompt.message || ""),
      options: options,
    };
    if (when) result.when = when;
    return result;
  }
  return null;
}

function __oc_auth_method_summary(method) {
  if (!method || typeof method !== "object") return null;
  if (method.type !== "oauth" && method.type !== "api") return null;
  const result = {
    type: method.type,
    label: String(method.label || ""),
  };
  if (Array.isArray(method.prompts)) {
    result.prompts = method.prompts.map(__oc_auth_prompt_summary).filter(function (prompt) { return prompt !== null; });
  }
  return result;
}

function __oc_auth_summary(auth) {
  if (!auth || typeof auth !== "object" || typeof auth.provider !== "string" || !Array.isArray(auth.methods)) return null;
  return {
    provider: auth.provider,
    methods: auth.methods.map(__oc_auth_method_summary).filter(function (method) { return method !== null; }),
  };
}

function __oc_auth_find(provider) {
  for (const auth of __oc_plugin.authHooks) {
    if (auth && auth.provider === provider) return auth;
  }
  return null;
}

function __oc_auth_key(provider, method) {
  // Keep the key free of embedded NUL bytes. Older QuickJS builds can crash
  // while hashing a NUL-containing property name even though modern engines
  // accept it as a normal JavaScript string.
  return String(provider) + ":" + String(method);
}

function __oc_hooks_summary() {
  const names = new Set();
  for (const hooks of __oc_plugin.hooks) {
    for (const key of Object.keys(hooks)) {
      if (typeof hooks[key] === "function") names.add(key);
      else if (key === "tool") names.add("tool");
    }
  }
  return {
    hookNames: Array.from(names),
    tools: Object.keys(__oc_plugin.toolKeys).map((name) => ({
      name,
      description: __oc_plugin.toolKeys[name].description,
      schema: __oc_plugin.toolKeys[name].schema,
    })),
    auth: __oc_plugin.authHooks.map(__oc_auth_summary).filter(function (auth) { return auth !== null; }),
  };
}

// Promise entrypoint: load the current module as a plugin.
function __oc_load_plugin(payload) {
  const exports = __oc_main_exports || __oc_modules;
  const input = __oc_make_input(payload.input || {});
  const options = payload.options;
  const picked = __oc_pick_server(exports);
  if (!picked) {
    throw new TypeError("Plugin must default export an object with server() or a function");
  }
  __oc_plugin.pluginId = picked.id !== undefined ? picked.id : null;
  const hooks = picked.v2 ? picked.fn(__oc_make_v2_context()) : picked.fn(input, options);
  return Promise.resolve(hooks).then(function (resolvedHooks) {
    if (resolvedHooks !== undefined && resolvedHooks !== null) {
      __oc_register_hooks(resolvedHooks);
    }
    const summary = __oc_hooks_summary();
    summary.pluginId = picked.id !== undefined ? picked.id : null;
    return summary;
  });
}

// Promise entrypoint: call a hook with (input, output), returning the output.
function __oc_trigger(payload) {
  const name = payload.name;
  let input = payload.input === undefined || payload.input === null ? {} : payload.input;
  let output = payload.output === undefined || payload.output === null ? {} : payload.output;
  let chain = Promise.resolve();
  for (const hooks of __oc_plugin.hooks) {
    const fn = hooks[name];
    if (typeof fn !== "function") continue;
    chain = chain.then(function () {
      return Promise.resolve(fn(input, output));
    });
  }
  return chain.then(function () { return output; });
}

// Promise auth entrypoints. The request contains only serializable identifiers
// and inputs. Function-valued validators/authorize/callbacks never leave this
// QuickJS context.
function __oc_auth_validate(payload) {
  return Promise.resolve().then(function () {
    const auth = __oc_auth_find(payload.provider);
    if (!auth || !Array.isArray(auth.methods)) throw new Error("No auth hook for provider '" + payload.provider + "'");
    const method = auth.methods[payload.method];
    if (!method) throw new Error("No auth method " + payload.method + " for provider '" + payload.provider + "'");
    const prompts = Array.isArray(method.prompts) ? method.prompts : [];
    const prompt = prompts.find(function (entry) { return entry && entry.key === payload.key; });
    if (!prompt || typeof prompt.validate !== "function") return null;
    return prompt.validate(String(payload.value === undefined || payload.value === null ? "" : payload.value));
  }).then(function (resolved) {
    return resolved === undefined || resolved === null ? null : String(resolved);
  });
}

function __oc_auth_authorize(payload) {
  return Promise.resolve().then(function () {
    const auth = __oc_auth_find(payload.provider);
    if (!auth || !Array.isArray(auth.methods)) throw new Error("No auth hook for provider '" + payload.provider + "'");
    const method = auth.methods[payload.method];
    if (!method || typeof method.authorize !== "function") throw new Error("Auth method " + payload.method + " has no authorize function");
    return method.authorize(payload.inputs || {});
  }).then(function (resolved) {
    if (resolved === undefined || resolved === null || typeof resolved !== "object") throw new Error("Auth authorize returned no result");

    const pendingKey = __oc_auth_key(payload.provider, payload.method);
    delete __oc_plugin.authPending[pendingKey];
    if (typeof resolved.callback === "function") {
      __oc_plugin.authPending[pendingKey] = {
        method: resolved.method,
        callback: resolved.callback,
      };
      return {
        url: String(resolved.url || ""),
        method: String(resolved.method || "auto"),
        instructions: String(resolved.instructions || ""),
      };
    }
    // API authorize results are already serializable and have no callback.
    return resolved;
  });
}

function __oc_auth_callback(payload) {
  const pendingKey = __oc_auth_key(payload.provider, payload.method);
  const pending = __oc_plugin.authPending[pendingKey];
  if (!pending || typeof pending.callback !== "function") {
    throw new Error("No pending auth callback for provider '" + payload.provider + "'");
  }
  let result;
  try {
    result = pending.method === "code"
      ? pending.callback(payload.code === undefined || payload.code === null ? "" : String(payload.code))
      : pending.callback();
  } catch (error) {
    delete __oc_plugin.authPending[pendingKey];
    throw error;
  }
  return Promise.resolve(result).then(function (resolved) {
    delete __oc_plugin.authPending[pendingKey];
    return resolved;
  }, function (error) {
    delete __oc_plugin.authPending[pendingKey];
    throw error;
  });
}

// Sync entrypoint: call the config hook.
function __oc_config(payload) {
  for (const hooks of __oc_plugin.hooks) {
    const fn = hooks.config;
    if (typeof fn === "function") fn(payload.config);
  }
  return true;
}

// Promise entrypoint: deliver an event to the event hook and wait for every
// hook's promise before returning to the Rust host. The manager's server
// fan-out relies on this completion boundary so event delivery is not lost
// when a plugin hook is declared async.
function __oc_event(payload) {
  let chain = Promise.resolve();
  for (const hooks of __oc_plugin.hooks) {
    const fn = hooks.event;
    if (typeof fn === "function") {
      chain = chain.then(function () {
        return Promise.resolve(fn({ event: payload.event }));
      });
    }
  }
  return chain.then(function () { return true; });
}

// Sync entrypoint: dispose hooks.
function __oc_dispose() {
  const pending = [];
  for (const hooks of __oc_plugin.hooks) {
    const fn = hooks.dispose;
    if (typeof fn === "function") pending.push(Promise.resolve().then(function () { return fn(); }));
  }
  __oc_plugin.hooks = [];
  return Promise.all(pending).then(function () { return true; });
}

// Promise entrypoint: execute a tool.
function __oc_tool_execute(payload) {
  const name = payload.name;
  const def = __oc_plugin.tools[name];
  if (!def) throw new Error("Tool '" + name + "' is not registered");
  if (typeof def.execute !== "function") throw new Error("Tool '" + name + "' has no execute function");
  const context = __oc_make_tool_context(payload.context || {});
  return def.execute(payload.args, context);
}

function __oc_make_tool_context(ctx) {
  return {
    sessionID: ctx.sessionID || "",
    messageID: ctx.messageID || "",
    agent: ctx.agent || "",
    directory: ctx.directory || "",
    worktree: ctx.worktree || "",
    abort: __oc_abort_signal(),
    metadata: function (input) {
      __oc_bridge_sync("tool.metadata", { callID: ctx.callID, title: input && input.title, metadata: input && input.metadata });
    },
    ask: function (input) {
      return Promise.resolve().then(function () {
        const result = __oc_bridge_sync("tool.ask", { callID: ctx.callID, input });
        if (result && result.ok === false) throw new Error(result.message || "ask failed");
        return result;
      });
    },
  };
}

function __oc_abort_signal() {
  const signal = {
    aborted: false,
    reason: null,
    _abortNotified: false,
    addEventListener: function (type, listener) {
      if (type === "abort") {
        this._listeners = this._listeners || [];
        this._listeners.push(listener);
      }
    },
    removeEventListener: function () {},
    dispatchEvent: function () { return true; },
  };
  Object.defineProperty(signal, "aborted", { get: function () { return __oc_tool_cancelled(); } });
  globalThis.__oc_active_abort_signal = signal;
  return signal;
}

function __oc_tool_abort_notify() {
  const signal = globalThis.__oc_active_abort_signal;
  if (!signal || signal._abortNotified || !__oc_tool_cancelled()) return false;
  signal._abortNotified = true;
  signal.reason = new Error("tool execution aborted");
  const event = { type: "abort", target: signal };
  (signal._listeners || []).slice().forEach(function (listener) {
    try { listener.call(signal, event); } catch (_) {}
  });
  return true;
}

// Promise entrypoint: run a workspace adapter method.
function __oc_workspace_adapter(payload) {
  const adapter = __oc_plugin.workspaceAdapters[payload.type];
  if (!adapter) throw new Error("No workspace adapter registered for '" + payload.type + "'");
  const fn = adapter[payload.method];
  if (typeof fn !== "function") throw new Error("Workspace adapter '" + payload.type + "' has no method '" + payload.method + "'");
  return fn(payload.args);
}

// Promise entrypoint: run a v2 domain transform callback with a mutable draft.
// Each domain wraps the raw draft with the reference's typed surface
// (reference/packages/core/src/plugin/host.ts): read operations like `list` /
// `get` and mutations like `update` / `remove` / `default` operate on the
// shared draft object, so a plugin's transforms shape the same JSON document
// the Rust host handed in.
function __oc_v2_transform(payload) {
  const callbacks = (__oc_plugin.v2Callbacks = __oc_plugin.v2Callbacks || Object.create(null));
  const callback = callbacks[payload.domain];
  if (typeof callback !== "function") {
    throw new Error("No v2 transform callback registered for domain '" + payload.domain + "'");
  }
  const draft = payload.draft === undefined || payload.draft === null ? {} : payload.draft;
  return Promise.resolve(callback(__oc_v2_wrap_draft(payload.domain, draft))).then(function () { return draft; });
}

// The reference host hands each domain callback a scoped API over the draft.
// This mirrors the wrapper construction in reference/packages/core/src/plugin/host.ts
// so `ctx.agent.transform((draft) => draft.update(...))` has real semantics.
function __oc_v2_wrap_draft(domain, draft) {
  if (domain === "command") return draft; // command transforms receive the raw draft
  const data = draft;
  switch (domain) {
    case "agent": {
      return {
        list: function () { return data.agents || []; },
        get: function (id) {
          return (data.agents || []).find(function (agent) { return agent.id === id; });
        },
        default: function (id) {
          if (id === undefined) return data.defaultAgent;
          data.defaultAgent = id;
        },
        update: function (id, update) {
          const agents = (data.agents = data.agents || []);
          const index = agents.findIndex(function (agent) { return agent.id === id; });
          if (index === -1) agents.push(Object.assign({ id: id }, update));
          else agents[index] = Object.assign({}, agents[index], update);
        },
        remove: function (id) {
          data.agents = (data.agents || []).filter(function (agent) { return agent.id !== id; });
        },
      };
    }
    case "skill": {
      return {
        source: function (source) {
          const sources = (data.sources = data.sources || []);
          sources.push(source);
        },
        list: function () { return data.sources || []; },
      };
    }
    case "reference": {
      return {
        add: function (name, source) {
          data.references = data.references || {};
          data.references[name] = source;
        },
        remove: function (name) {
          if (data.references) delete data.references[name];
        },
        list: function () {
          return Object.keys(data.references || {});
        },
      };
    }
    case "catalog": {
      // Catalog items mirror the reference: `{ provider, models }` where
      // `models` is a map of model id -> model. `provider.update` /
      // `model.update` accept either a mutation function or a plain object.
      const itemFor = function (providerID) {
        return (data.providers || []).find(function (item) {
          return item && item.provider && item.provider.id === providerID;
        });
      };
      const apply = function (target, update) {
        if (typeof update === "function") update(target);
        else if (update && typeof update === "object") Object.assign(target, update);
      };
      return {
        provider: {
          list: function () { return data.providers || []; },
          get: function (id) {
            const item = itemFor(id);
            return item ? item.provider : undefined;
          },
          update: function (id, update) {
            const providers = (data.providers = data.providers || []);
            let item = itemFor(id);
            if (!item) {
              item = { provider: { id: id }, models: {} };
              providers.push(item);
            }
            apply(item.provider, update);
          },
          remove: function (id) {
            data.providers = (data.providers || []).filter(function (item) {
              return !item || !item.provider || item.provider.id !== id;
            });
          },
        },
        model: {
          get: function (providerID, modelID) {
            const item = itemFor(providerID);
            return item && item.models ? item.models[modelID] : undefined;
          },
          update: function (providerID, modelID, update) {
            const providers = (data.providers = data.providers || []);
            let item = itemFor(providerID);
            if (!item) {
              item = { provider: { id: providerID }, models: {} };
              providers.push(item);
            }
            item.models = item.models || {};
            if (!item.models[modelID]) item.models[modelID] = { id: modelID };
            apply(item.models[modelID], update);
          },
          remove: function (providerID, modelID) {
            const item = itemFor(providerID);
            if (item && item.models) delete item.models[modelID];
          },
          default: {
            get: function () { return data.defaultModel; },
            set: function (providerID, modelID) {
              data.defaultModel = { providerID: providerID, modelID: modelID };
            },
          },
        },
      };
    }
    case "integration": {
      return {
        list: function () { return data.integrations || []; },
        get: function (id) {
          return (data.integrations || []).find(function (integration) { return integration.id === id; });
        },
        update: function (id, update) {
          const integrations = (data.integrations = data.integrations || []);
          const index = integrations.findIndex(function (integration) { return integration.id === id; });
          if (index === -1) integrations.push(Object.assign({ id: id }, update));
          else integrations[index] = Object.assign({}, integrations[index], update);
        },
        remove: function (id) {
          data.integrations = (data.integrations || []).filter(function (integration) { return integration.id !== id; });
        },
        method: {
          list: function (id) {
            const integration = (data.integrations || []).find(function (candidate) { return candidate.id === id; });
            return (integration && integration.methods) || [];
          },
          update: function (input) {
            const integration = (data.integrations || []).find(function (candidate) { return candidate.id === (input && input.integrationID); });
            if (integration) {
              integration.methods = integration.methods || [];
              const method = input && input.method;
              const existing = integration.methods.find(function (candidate) {
                return candidate.id && method && candidate.id === method.id;
              });
              if (existing && method) Object.assign(existing, method);
              else if (method) integration.methods.push(method);
            }
          },
          remove: function (id, method) {
            const integration = (data.integrations || []).find(function (candidate) { return candidate.id === id; });
            if (integration) {
              integration.methods = (integration.methods || []).filter(function (candidate) {
                return !method || !method.id || candidate.id !== method.id;
              });
            }
          },
        },
      };
    }
    default:
      return data;
  }
}

// ---------------------------------------------------------------------------
// v2 promise / effect API shims
// ---------------------------------------------------------------------------

// The v2 API is hosted on the same registries. Each domain exposes
// transform(callback) / reload() backed by the Rust host so Promise-based v2
// plugins can register agents, commands, skills, providers and references.
function __oc_v2_domain(method) {
  return {
    transform: function (callback) {
      __oc_plugin.v2Callbacks = __oc_plugin.v2Callbacks || Object.create(null);
      __oc_plugin.v2Callbacks[method] = callback;
      __oc_bridge_sync("v2.transform", { domain: method });
      return { dispose: function () {} };
    },
    reload: function () {
      return Promise.resolve().then(function () {
        __oc_bridge_sync("v2.reload", { domain: method });
        return true;
      });
    },
  };
}

__oc_modules["opencode/plugin/v2/effect"] = {
  define: function (input) {
    return input;
  },
  default: {},
};
__oc_modules["opencode/plugin/v2/effect/integration"] = {
  define: function (input) { return input; },
  default: {},
};
__oc_modules["opencode/plugin/v2/effect/plugin"] = {
  define: function (input) { return input; },
  default: {},
};
__oc_modules["opencode/plugin/v2/promise"] = {
  define: function (input) {
    return input;
  },
  PluginContext: {},
  Registration: {},
  default: {},
};

// ---------------------------------------------------------------------------
// node:* shims
// ---------------------------------------------------------------------------

__oc_modules["node:path"] = (function () {
  function normalizeArray(parts) {
    const out = [];
    for (const part of parts) {
      if (part === "" || part === ".") continue;
      if (part === "..") {
        if (out.length && out[out.length - 1] !== "..") out.pop();
        else if (out.length === 0) out.push("..");
      } else {
        out.push(part);
      }
    }
    return out;
  }
  function normalize(p) {
    if (p === "" || p === undefined) return ".";
    const isAbsolutePath = p.startsWith("/");
    const parts = normalizeArray(p.split(/[\\/]+/));
    const prefix = isAbsolutePath ? "/" : "";
    const joined = prefix + parts.join("/");
    return joined === "" ? "." : joined;
  }
  function join() {
    let out = "";
    for (const part of arguments) {
      if (part === "" || part === undefined) continue;
      if (out === "") out = part;
      else out = out.replace(/\/+$/, "") + "/" + String(part).replace(/^\/+/, "");
    }
    return normalize(out);
  }
  function resolve() {
    let out = "/";
    for (const part of arguments) {
      if (typeof part !== "string") continue;
      if (part.startsWith("/")) out = part;
      else out = out.replace(/\/+$/, "") + "/" + part;
    }
    return normalize(out);
  }
  function dirname(p) {
    const parts = String(p).split(/[\\/]+/);
    parts.pop();
    return parts.join("/") || (String(p).startsWith("/") ? "/" : ".");
  }
  function basename(p, ext) {
    let name = String(p).split(/[\\/]+/).pop() || "";
    if (ext && name.endsWith(ext)) name = name.slice(0, -ext.length);
    return name;
  }
  function extname(p) {
    const name = basename(p);
    const index = name.lastIndexOf(".");
    if (index <= 0) return "";
    return name.slice(index);
  }
  return {
    join: join,
    resolve: resolve,
    normalize: normalize,
    dirname: dirname,
    basename: basename,
    extname: extname,
    relative: function (from, to) {
      const a = normalize(from).split("/").filter(Boolean);
      const b = normalize(to).split("/").filter(Boolean);
      while (a.length && b.length && a[0] === b[0]) { a.shift(); b.shift(); }
      return Array(a.length).fill("..").concat(b).join("/") || ".";
    },
    isAbsolute: function (p) { return String(p).startsWith("/"); },
    sep: "/",
    delimiter: ":",
    parse: function (p) {
      return { root: "/", dir: dirname(p), base: basename(p), ext: extname(p), name: basename(p, extname(p)) };
    },
  };
})();

__oc_modules["node:fs/promises"] = (function () {
  const fsMethods = ["mkdir", "rm", "readFile", "writeFile", "readdir", "stat", "access", "readlink", "readJson", "writeJson", "exists", "realpath"];
  const out = {};
  for (const method of fsMethods) {
    out[method] = function (arg1, arg2, arg3) {
      return Promise.resolve().then(function () {
        const result = __oc_bridge_sync("fs", { method: method, args: [arg1, arg2, arg3] });
        if (result && result.ok === false) {
          const err = new Error(result.error || method + " failed");
          err.code = result.code;
          throw err;
        }
        return result && result.data !== undefined ? result.data : undefined;
      });
    };
  }
  out.constants = {};
  return out;
})();

__oc_modules["node:os"] = {
  homedir: function () {
    const result = __oc_bridge_sync("os", { name: "homedir" });
    return result && result.value !== undefined ? result.value : "";
  },
  tmpdir: function () {
    const result = __oc_bridge_sync("os", { name: "tmpdir" });
    return result && result.value !== undefined ? result.value : "/tmp";
  },
  platform: function () {
    const result = __oc_bridge_sync("os", { name: "platform" });
    return result && result.value !== undefined ? result.value : "linux";
  },
  arch: function () {
    return "x64";
  },
  EOL: "\n",
};

__oc_modules["node:url"] = {
  pathToFileURL: function (p) { return { href: String(p).replace(/^\/+/, "") ? "file://" + String(p) : "file:///" + String(p) }; },
  fileURLToPath: function (url) {
    const s = typeof url === "string" ? url : (url && url.href) || String(url);
    if (s.startsWith("file://")) return s.slice("file://".length);
    return s;
  },
};

__oc_modules["node:process"] = {
  env: {},
  cwd: function () {
    const result = __oc_bridge_sync("os", { name: "cwd" });
    return result && result.value !== undefined ? result.value : "/";
  },
  platform: "linux",
  arch: "x64",
  argv: [],
};

__oc_modules["node:util"] = {
  promisify: function (fn) {
    return function () {
      const args = Array.prototype.slice.call(arguments);
      return new Promise(function (resolve, reject) {
        args.push(function (err, value) { return err ? reject(err) : resolve(value); });
        fn.apply(null, args);
      });
    };
  },
};

// Blob polyfill (QuickJS may lack it).
if (typeof Blob === "undefined") {
  globalThis.Blob = class Blob {
    constructor(parts, options) {
      this._parts = parts || [];
      this._options = options || {};
      this.size = 0;
    }
    text() {
      const all = this._parts.map((p) => (p instanceof Uint8Array ? String.fromCharCode.apply(null, p) : String(p))).join("");
      return Promise.resolve(all);
    }
    arrayBuffer() {
      return Promise.resolve(new Uint8Array(0).buffer);
    }
    slice() {
      return new Blob();
    }
    get type() { return this._options.type || ""; }
  };
}

if (typeof Uint8Array !== "undefined" && typeof TextEncoder === "undefined") {
  globalThis.TextEncoder = class TextEncoder {
    encode(input) {
      const s = String(input);
      const out = new Uint8Array(s.length * 4);
      let n = 0;
      for (let i = 0; i < s.length; i++) {
        const code = s.codePointAt(i);
        if (code < 0x80) out[n++] = code;
        else if (code < 0x800) { out[n++] = 0xc0 | (code >> 6); out[n++] = 0x80 | (code & 0x3f); }
        else if (code < 0x10000) { out[n++] = 0xe0 | (code >> 12); out[n++] = 0x80 | ((code >> 6) & 0x3f); out[n++] = 0x80 | (code & 0x3f); }
        else { out[n++] = 0xf0 | (code >> 18); out[n++] = 0x80 | ((code >> 12) & 0x3f); out[n++] = 0x80 | ((code >> 6) & 0x3f); out[n++] = 0x80 | (code & 0x3f); }
      }
      return out.subarray(0, n);
    }
  };
}

// ---------------------------------------------------------------------------
// The @opencode-ai/plugin module
// ---------------------------------------------------------------------------

const __oc_legacy_default = {
  config: function (hook) {
    // Legacy hook registration API (pre-1.18): route into the hooks list.
    return __oc_register_v1_hook("config", hook);
  },
  provider: function (input) {
    return __oc_register_v1("provider", input);
  },
  agent: function (input) {
    return __oc_register_v1("agent", input);
  },
  command: function (input) {
    return __oc_register_v1("command", input);
  },
  skill: function (input) {
    return __oc_register_v1("skill", input);
  },
  variant: function (input) {
    return __oc_register_v1("variant", input);
  },
  tool: __oc_tool,
  hook: function (name, hook) {
    return __oc_register_v1_hook(name, hook);
  },
  chat: {
    message: function (hook) { return __oc_register_v1_hook("chat.message", hook); },
    params: function (hook) { return __oc_register_v1_hook("chat.params", hook); },
    headers: function (hook) { return __oc_register_v1_hook("chat.headers", hook); },
  },
  event: function (hook) { return __oc_register_v1_hook("event", hook); },
  shell: {
    env: function (hook) { return __oc_register_v1_hook("shell.env", hook); },
  },
  permission: {
    ask: function (hook) { return __oc_register_v1_hook("permission.ask", hook); },
  },
  command_execute_before: function (hook) { return __oc_register_v1_hook("command.execute.before", hook); },
  tool_execute_before: function (hook) { return __oc_register_v1_hook("tool.execute.before", hook); },
  tool_execute_after: function (hook) { return __oc_register_v1_hook("tool.execute.after", hook); },
};

function __oc_register_v1_hook(name, hook) {
  if (typeof hook !== "function") throw new TypeError("hook " + name + " must be a function");
  if (__oc_plugin.hooks.length === 0) __oc_plugin.hooks.push({});
  __oc_plugin.hooks[0][name] = hook;
  return hook;
}

function __oc_register_v1(kind, input) {
  // The legacy declarative registrations (agent/command/skill/...) are routed
  // to the host so the integrating application can consume them.
  __oc_bridge_sync("v1.register", { kind: kind, input: input, pluginId: __oc_plugin.pluginId });
  return input;
}

__oc_modules["opencode/plugin"] = {
  tool: __oc_tool,
  z: z,
  default: __oc_legacy_default,
};
