use std::{
    convert::TryFrom,
    ffi::CString,
    marker::PhantomData,
    os::raw::{c_int, c_void},
    sync::{Arc, Mutex},
};

use libquickjs_sys as q;

use crate::js::value::JsValue;

use super::value::JsError;

// JS_TAG_* constants from QuickJS. bindgen does not pick them up for some reason.
const TAG_STRING: i64 = -7;
const TAG_OBJECT: i64 = -1;
const TAG_INT: i64 = 0;
const TAG_BOOL: i64 = 1;
const TAG_NULL: i64 = 2;
const TAG_UNDEFINED: i64 = 3;
const TAG_EXCEPTION: i64 = 6;
const TAG_FLOAT64: i64 = 7;

/// Free a JSValue, mirroring `JS_FreeValue` (which is `static inline` in
/// quickjs.h and therefore not exported as a symbol).
///
/// # Safety
/// `value` must be a live QuickJS value owned by `context`.
unsafe fn free_value(context: *mut q::JSContext, value: q::JSValue) {
    if value.tag < 0 {
        let ptr =
            std::mem::transmute::<*mut std::ffi::c_void, *mut q::JSRefCountHeader>(value.u.ptr);
        let pref: &mut q::JSRefCountHeader = &mut *ptr;
        pref.ref_count -= 1;
        if pref.ref_count <= 0 {
            q::__JS_FreeValue(context, value);
        }
    }
}

fn make_cstring(value: &str) -> Result<CString, JsError> {
    CString::new(value).map_err(|_| JsError::StringWithZeroBytes)
}

/// A Rust closure used as a global JS function. Mirrors the `Callback` trait
/// from quick-js' bindings.rs so closures with up to 3 arguments of simple
/// types can be registered with [`Runtime::add_callback`].
pub trait Callback<F>: Send + Sync + 'static {
    fn argument_count(&self) -> usize;
    fn call(&self, args: Vec<JsValue>) -> Result<Result<JsValue, String>, JsError>;
}

macro_rules! count {
    () => { 0 };
    ($first:ident $($rest:ident)*) => { 1 + count!($($rest)*) };
}

impl<R, F> Callback<PhantomData<(&R, &F)>> for F
where
    F: Fn() -> R + Send + Sync + 'static,
    R: Into<JsValue>,
{
    fn argument_count(&self) -> usize {
        0
    }

    fn call(&self, args: Vec<JsValue>) -> Result<Result<JsValue, String>, JsError> {
        if !args.is_empty() {
            return Ok(Err("invalid argument count".into()));
        }
        Ok(Ok(self().into()))
    }
}

#[allow(non_snake_case)]
macro_rules! impl_callback {
    ($first:ident $($rest:ident)*) => {
        #[allow(non_snake_case)]
        impl<$first, $($rest,)* R, F> Callback<PhantomData<($first, $($rest,)* &R, &F)>> for F
        where
            F: Fn($first, $($rest),*) -> R + Send + Sync + 'static,
            $first: TryFrom<JsValue, Error = JsError>,
            $($rest: TryFrom<JsValue, Error = JsError>,)*
            R: Into<JsValue>,
        {
            fn argument_count(&self) -> usize {
                count!($first $($rest)*)
            }

            fn call(&self, args: Vec<JsValue>) -> Result<Result<JsValue, String>, JsError> {
                let mut iter = args.into_iter();
                let $first = $first::try_from(iter.next().ok_or_else(|| JsError::UnexpectedType)?)?;
                $(
                    let $rest = $rest::try_from(iter.next().ok_or_else(|| JsError::UnexpectedType)?)?;
                )*
                Ok(Ok(self($first, $($rest),*).into()))
            }
        }
    };
}

impl_callback! { A1 }
impl_callback! { A1 A2 }
impl_callback! { A1 A2 A3 }

type WrappedCallback = dyn Fn(c_int, *mut q::JSValue) -> q::JSValue;
type CallbackRegistry = Vec<(Box<WrappedCallback>, Box<q::JSValue>)>;

/// Build a C trampoline for a Rust closure so QuickJS can call it.
///
/// The returned boxed closure and data must be kept alive by the caller (the
/// callback registry on the [`Runtime`]).
///
/// # Safety
/// Mirrors the approach in quick-js' bindings.rs; the closure pointer is
/// stored in a JSValue and recovered inside the C trampoline.
unsafe fn build_closure_trampoline<F>(
    closure: F,
) -> ((Box<WrappedCallback>, Box<q::JSValue>), q::JSCFunctionData)
where
    F: Fn(c_int, *mut q::JSValue) -> q::JSValue + 'static,
{
    unsafe extern "C" fn trampoline<F>(
        _ctx: *mut q::JSContext,
        _this: q::JSValue,
        argc: c_int,
        argv: *mut q::JSValue,
        _magic: c_int,
        data: *mut q::JSValue,
    ) -> q::JSValue
    where
        F: Fn(c_int, *mut q::JSValue) -> q::JSValue,
    {
        let closure_ptr = (*data).u.ptr;
        let closure: &mut F = &mut *(closure_ptr as *mut F);
        (*closure)(argc, argv)
    }

    let boxed_f = Box::new(closure);
    let data = Box::new(q::JSValue {
        u: q::JSValueUnion {
            ptr: (&*boxed_f) as *const F as *mut c_void,
        },
        tag: TAG_NULL,
    });
    ((boxed_f, data), Some(trampoline::<F>))
}

/// An owned handle to a JS value. Frees the value on drop.
struct Owned {
    ctx: *mut q::JSContext,
    value: q::JSValue,
}

impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { free_value(self.ctx, self.value) };
    }
}

impl Owned {
    /// # Safety
    /// The caller takes ownership of the raw value.
    unsafe fn into_inner(mut self) -> q::JSValue {
        let value = self.value;
        self.value = q::JSValue {
            u: q::JSValueUnion { int32: 0 },
            tag: TAG_UNDEFINED,
        };
        value
    }
}

/// An owned handle to a JS object, with convenience property accessors.
struct OwnedObject {
    value: Owned,
}

impl OwnedObject {
    fn new(value: Owned) -> Result<Self, JsError> {
        if value.value.tag != TAG_OBJECT {
            Err(JsError::Internal("expected an object".into()))
        } else {
            Ok(Self { value })
        }
    }

    fn property(&self, name: &str) -> Result<Owned, JsError> {
        let cname = make_cstring(name)?;
        let raw = unsafe { q::JS_GetPropertyStr(self.value.ctx, self.value.value, cname.as_ptr()) };
        if raw.tag == TAG_EXCEPTION {
            Err(JsError::Internal(format!(
                "exception while getting property '{name}'"
            )))
        } else if raw.tag == TAG_UNDEFINED {
            Err(JsError::Internal(format!("property '{name}' not found")))
        } else {
            Ok(Owned {
                ctx: self.value.ctx,
                value: raw,
            })
        }
    }
}

/// A minimal in-process JavaScript runtime wrapping QuickJS via
/// `libquickjs-sys`.
///
/// This is the documented divergence from the reference, which spawns a Bun
/// subprocess. All `unsafe` FFI in this crate lives in this module.
pub struct Runtime {
    rt: *mut q::JSRuntime,
    ctx: *mut q::JSContext,
    callbacks: Arc<Mutex<CallbackRegistry>>,
}

impl Runtime {
    // QuickJS runtimes are not thread-safe: `Runtime` is deliberately `!Send`
    // (raw `rt`/`ctx` pointers), so the callback registry may hold `!Send`
    // closures too. `#[allow(clippy::arc_with_non_send_sync)]`: wrapping in a
    // `Mutex` is required for interior mutability on the single owning thread
    // (RUST-005); do not fake `Send`.
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn new() -> Result<Self, JsError> {
        let rt = unsafe { q::JS_NewRuntime() };
        if rt.is_null() {
            return Err(JsError::Internal("could not create JS runtime".into()));
        }
        let ctx = unsafe { q::JS_NewContext(rt) };
        if ctx.is_null() {
            unsafe { q::JS_FreeRuntime(rt) };
            return Err(JsError::Internal("could not create JS context".into()));
        }
        // JS_NewContext already registers the standard intrinsics (including
        // Promise, Map/Set and TypedArrays) via js_standard_init.
        let runtime = Self {
            rt,
            ctx,
            callbacks: Arc::new(Mutex::new(Vec::new())),
        };
        // This bundled QuickJS predates the `globalThis` standard; polyfill it
        // so the polyfill runtime and plugin code can rely on it.
        runtime.eval(
            "if (typeof globalThis === \"undefined\") { globalThis = new Function(\"return this\")(); }",
            "opencode-globalthis.js",
        )?;
        Ok(runtime)
    }

    /// Evaluate `code` as a global script and return the value of the final
    /// expression. The job queue is not pumped; call [`Runtime::pump_jobs`]
    /// when the evaluated code schedules promise callbacks.
    pub fn eval(&self, code: &str, filename: &str) -> Result<JsValue, JsError> {
        let code_c = make_cstring(code)?;
        let filename_c = make_cstring(filename)?;
        let raw = unsafe {
            q::JS_Eval(
                self.ctx,
                code_c.as_ptr(),
                code.len(),
                filename_c.as_ptr(),
                q::JS_EVAL_TYPE_GLOBAL as c_int,
            )
        };
        let owned = Owned {
            ctx: self.ctx,
            value: raw,
        };
        if owned.value.tag == TAG_EXCEPTION {
            return Err(exception(self.ctx));
        }
        to_value(self.ctx, &owned.value)
    }

    /// Evaluate `code` and convert the resulting value to JSON. Only works for
    /// JSON-compatible results (no functions, classes, ...).
    pub fn eval_json(&self, code: &str, filename: &str) -> Result<serde_json::Value, JsError> {
        let value = self.eval(code, filename)?;
        Ok(serde_json::Value::from(value))
    }

    /// Run all pending microtasks / promise jobs to completion. This makes
    /// `await` chains over already-resolved thenables progress even though the
    /// engine itself is synchronous.
    pub fn pump_jobs(&self) {
        loop {
            let pending = unsafe { q::JS_IsJobPending(self.rt) };
            if pending == 0 {
                return;
            }
            let mut ctx = self.ctx;
            let _ = unsafe { q::JS_ExecutePendingJob(self.rt, &mut ctx) };
        }
    }

    /// Read the current value of a global by name. Returns `null` for absent
    /// globals.
    pub fn global(&self, name: &str) -> Result<serde_json::Value, JsError> {
        let global = global_object(self.ctx)?;
        let cname = make_cstring(name)?;
        let raw = unsafe { q::JS_GetPropertyStr(self.ctx, global.value.value, cname.as_ptr()) };
        if raw.tag == TAG_EXCEPTION {
            return Err(exception(self.ctx));
        }
        let value = to_value(self.ctx, &raw)?;
        unsafe { free_value(self.ctx, raw) };
        Ok(serde_json::Value::from(value))
    }

    /// Set a global from a JSON value.
    pub fn set_global_json(&self, name: &str, value: serde_json::Value) -> Result<(), JsError> {
        let js_value = JsValue::from(&value);
        let owned = serialize_value(self.ctx, js_value)?;
        let global = global_object(self.ctx)?;
        let cname = make_cstring(name)?;
        // JS_SetPropertyStr takes ownership of the value; consume it so it is
        // not freed twice.
        let raw = unsafe { owned.into_inner() };
        let ret =
            unsafe { q::JS_SetPropertyStr(self.ctx, global.value.value, cname.as_ptr(), raw) };
        if ret < 0 {
            return Err(JsError::Internal(format!("could not set global '{name}'")));
        }
        Ok(())
    }

    /// Set a global to JS `null`.
    pub fn set_global_null(&self, name: &str) -> Result<(), JsError> {
        self.set_global_json(name, serde_json::Value::Null)
    }

    /// Install the JS <-> Rust bridge callback (`__oc_host_bridge`) used by the
    /// polyfill runtime.
    pub fn install_bridge(
        &self,
        host: std::sync::Arc<dyn crate::host::PluginHost>,
        resolver: std::sync::Arc<crate::loader::ModuleResolver>,
    ) -> Result<(), JsError> {
        let callback = crate::bridge::make_callback(host, resolver);
        self.add_callback("__oc_host_bridge", callback)
    }

    /// Call a global JS function by name. `args` must be simple values; use
    /// [`Runtime::call_json`] for JSON payloads.
    pub fn call_function(
        &self,
        name: &str,
        args: impl IntoIterator<Item = impl Into<JsValue>>,
    ) -> Result<JsValue, JsError> {
        let global = global_object(self.ctx)?;
        let func = global.property(name)?;
        if func.value.tag != TAG_OBJECT {
            return Err(JsError::Internal(format!(
                "could not find function '{name}' in global scope"
            )));
        }
        let qargs = args
            .into_iter()
            .map(|arg| serialize_value(self.ctx, arg.into()))
            .collect::<Result<Vec<_>, _>>()?;
        let qargs = qargs.iter().map(|arg| arg.value).collect::<Vec<_>>();
        let raw = unsafe {
            q::JS_Call(
                self.ctx,
                func.value,
                q::JSValue {
                    u: q::JSValueUnion { int32: 0 },
                    tag: TAG_UNDEFINED,
                },
                qargs.len() as c_int,
                qargs.as_ptr() as *mut q::JSValue,
            )
        };
        let owned = Owned {
            ctx: self.ctx,
            value: raw,
        };
        if owned.value.tag == TAG_EXCEPTION {
            return Err(exception(self.ctx));
        }
        to_value(self.ctx, &owned.value)
    }

    /// Call a global JS function with a single JSON-string argument and read
    /// the result back. The JS helper is expected to return a string produced
    /// by `JSON.stringify`; the polyfill's `__oc_call_json` does this.
    pub fn call_json(
        &self,
        name: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, JsError> {
        let payload =
            serde_json::to_string(&payload).map_err(|e| JsError::Internal(e.to_string()))?;
        let value = self.call_function(name, vec![JsValue::String(payload)])?;
        let s = value
            .into_string()
            .ok_or_else(|| JsError::Internal("call_json returned non-string".into()))?;
        if s.is_empty() {
            return Ok(serde_json::Value::Null);
        }
        serde_json::from_str(&s).map_err(|e| JsError::Internal(e.to_string()))
    }

    /// Register a Rust closure as a global JS function.
    ///
    /// The trampoline captures stable pointers only (`ctx`, `rt` and an `Arc`
    /// to the callback registry), so the `Runtime` may be moved after this
    /// call without invalidating the callback.
    pub fn add_callback<F>(&self, name: &str, callback: impl Callback<F>) -> Result<(), JsError> {
        let argcount = callback.argument_count() as c_int;
        let ctx = self.ctx;
        let _rt = self.rt;
        let callbacks = self.callbacks.clone();
        let _ = (_rt, &callbacks);
        let wrapper = move |argc: c_int, argv: *mut q::JSValue| -> q::JSValue {
            match exec_callback(ctx, argc, argv, &callback) {
                Ok(value) => unsafe { value.into_inner() },
                Err(err) => {
                    let message = err.to_string();
                    let js_exception = serialize_value(ctx, JsValue::String(message)).unwrap();
                    unsafe {
                        q::JS_Throw(ctx, js_exception.into_inner());
                    }
                    q::JSValue {
                        u: q::JSValueUnion { int32: 0 },
                        tag: TAG_EXCEPTION,
                    }
                }
            }
        };
        let (pair, trampoline) = unsafe { build_closure_trampoline(wrapper) };
        let data = (&*pair.1) as *const q::JSValue as *mut q::JSValue;
        self.callbacks.lock().unwrap().push(pair);
        let cfunc = unsafe { q::JS_NewCFunctionData(self.ctx, trampoline, argcount, 0, 1, data) };
        if cfunc.tag != TAG_OBJECT {
            return Err(JsError::Internal("could not create callback".into()));
        }
        let global = global_object(self.ctx)?;
        let cname = make_cstring(name)?;
        let ret =
            unsafe { q::JS_SetPropertyStr(self.ctx, global.value.value, cname.as_ptr(), cfunc) };
        if ret < 0 {
            return Err(JsError::Internal(format!(
                "could not set global function '{name}'"
            )));
        }
        Ok(())
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe {
            let rt = q::JS_GetRuntime(self.ctx);
            q::JS_FreeContext(self.ctx);
            q::JS_FreeRuntime(rt);
        }
    }
}

fn global_object(ctx: *mut q::JSContext) -> Result<OwnedObject, JsError> {
    let raw = unsafe { q::JS_GetGlobalObject(ctx) };
    OwnedObject::new(Owned { ctx, value: raw })
}

fn exception(ctx: *mut q::JSContext) -> JsError {
    let raw = unsafe { q::JS_GetException(ctx) };
    let owned = Owned { ctx, value: raw };
    // Prefer the JS `toString()` representation so Error objects give a
    // readable message even when `message` is non-enumerable.
    let str_raw = unsafe { q::JS_ToString(ctx, owned.value) };
    let str_owned = Owned {
        ctx,
        value: str_raw,
    };
    match to_value(ctx, &str_owned.value) {
        Ok(JsValue::String(message)) => JsError::Exception(message),
        Ok(value) => JsError::Exception(format!("{value:?}")),
        Err(_) => match to_value(ctx, &owned.value) {
            Ok(value) => JsError::Exception(format!("{value:?}")),
            Err(_) => JsError::Exception("unknown exception".into()),
        },
    }
}

/// Execute a registered callback closure with the JS-provided arguments.
fn exec_callback<F>(
    ctx: *mut q::JSContext,
    argc: c_int,
    argv: *mut q::JSValue,
    callback: &impl Callback<F>,
) -> Result<Owned, JsError> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let arg_slice = unsafe { std::slice::from_raw_parts(argv, argc as usize) };
        let args = arg_slice
            .iter()
            .map(|raw| to_value(ctx, raw))
            .collect::<Result<Vec<_>, _>>()?;
        match callback.call(args) {
            Ok(Ok(result)) => serialize_value(ctx, result),
            Ok(Err(message)) => Err(JsError::Internal(message)),
            Err(err) => Err(err),
        }
    }));
    match result {
        Ok(owned) => owned,
        Err(_) => Err(JsError::Internal("callback panicked".into())),
    }
}

/// Serialize a Rust value into a QuickJS value.
fn serialize_value(ctx: *mut q::JSContext, value: JsValue) -> Result<Owned, JsError> {
    let raw = match value {
        JsValue::Undefined | JsValue::Null => q::JSValue {
            u: q::JSValueUnion { int32: 0 },
            tag: TAG_NULL,
        },
        JsValue::Bool(flag) => q::JSValue {
            u: q::JSValueUnion {
                int32: if flag { 1 } else { 0 },
            },
            tag: TAG_BOOL,
        },
        JsValue::Int(val) => q::JSValue {
            u: q::JSValueUnion { int32: val },
            tag: TAG_INT,
        },
        JsValue::Float(val) => q::JSValue {
            u: q::JSValueUnion { float64: val },
            tag: TAG_FLOAT64,
        },
        JsValue::String(val) => {
            let cstr = make_cstring(&val)?;
            let qval = unsafe { q::JS_NewString(ctx, cstr.as_ptr()) };
            if qval.tag == TAG_EXCEPTION {
                return Err(JsError::Internal(
                    "could not create string in runtime".into(),
                ));
            }
            qval
        }
        JsValue::Array(values) => {
            let arr = unsafe { q::JS_NewArray(ctx) };
            if arr.tag == TAG_EXCEPTION {
                return Err(JsError::Internal(
                    "could not create array in runtime".into(),
                ));
            }
            for (index, item) in values.into_iter().enumerate() {
                let qvalue = match serialize_value(ctx, item) {
                    Ok(qvalue) => qvalue,
                    Err(err) => {
                        unsafe { free_value(ctx, arr) };
                        return Err(err);
                    }
                };
                // JS_DefinePropertyValue* consumes the value (set_value stores
                // it without duplicating); transfer ownership.
                let raw = unsafe { qvalue.into_inner() };
                let ret = unsafe {
                    q::JS_DefinePropertyValueUint32(
                        ctx,
                        arr,
                        index as u32,
                        raw,
                        q::JS_PROP_C_W_E as c_int,
                    )
                };
                if ret < 0 {
                    unsafe { free_value(ctx, arr) };
                    return Err(JsError::Internal("could not append array element".into()));
                }
            }
            arr
        }
        JsValue::Object(map) => {
            let obj = unsafe { q::JS_NewObject(ctx) };
            if obj.tag == TAG_EXCEPTION {
                return Err(JsError::Internal("could not create object".into()));
            }
            for (key, value) in map {
                let ckey = make_cstring(&key)?;
                let qvalue = match serialize_value(ctx, value) {
                    Ok(qvalue) => qvalue,
                    Err(err) => {
                        unsafe { free_value(ctx, obj) };
                        return Err(err);
                    }
                };
                let raw = unsafe { qvalue.into_inner() };
                let ret = unsafe { q::JS_SetPropertyStr(ctx, obj, ckey.as_ptr(), raw) };
                if ret < 0 {
                    unsafe { free_value(ctx, obj) };
                    return Err(JsError::Internal(format!("could not set property '{key}'")));
                }
            }
            obj
        }
    };
    Ok(Owned { ctx, value: raw })
}

/// Deserialize a QuickJS value into a Rust value. Borrows the raw value (no
/// ownership transfer); the caller keeps the reference alive and is
/// responsible for freeing it. Cyclic objects are cut at the first repeated
/// reference (read as `null`).
fn to_value(ctx: *mut q::JSContext, raw: &q::JSValue) -> Result<JsValue, JsError> {
    let mut visited = std::collections::HashSet::new();
    to_value_inner(ctx, raw, &mut visited)
}

fn to_value_inner(
    ctx: *mut q::JSContext,
    raw: &q::JSValue,
    visited: &mut std::collections::HashSet<usize>,
) -> Result<JsValue, JsError> {
    match raw.tag {
        TAG_INT => {
            let val = unsafe { raw.u.int32 };
            Ok(JsValue::Int(val))
        }
        TAG_BOOL => {
            let val = unsafe { raw.u.int32 };
            Ok(JsValue::Bool(val > 0))
        }
        TAG_NULL => Ok(JsValue::Null),
        TAG_UNDEFINED => Ok(JsValue::Undefined),
        TAG_FLOAT64 => {
            let val = unsafe { raw.u.float64 };
            Ok(JsValue::Float(val))
        }
        TAG_STRING => {
            let ptr = unsafe { q::JS_ToCStringLen(ctx, std::ptr::null_mut(), *raw, 0) };
            if ptr.is_null() {
                return Err(JsError::Internal("could not convert string".into()));
            }
            let cstr = unsafe { std::ffi::CStr::from_ptr(ptr) };
            let s = cstr.to_str().map_err(JsError::InvalidString)?.to_string();
            unsafe { q::JS_FreeCString(ctx, ptr) };
            Ok(JsValue::String(s))
        }
        TAG_OBJECT => {
            let is_array = unsafe { q::JS_IsArray(ctx, *raw) } > 0;
            if is_array {
                let length_name = make_cstring("length")?;
                let len_value = unsafe {
                    let len = q::JS_GetPropertyStr(ctx, *raw, length_name.as_ptr());
                    Owned { ctx, value: len }
                };
                let len = match to_value_inner(ctx, &len_value.value, visited) {
                    Ok(JsValue::Int(len)) => len,
                    _ => return Err(JsError::Internal("could not determine array length".into())),
                };
                let mut values = Vec::new();
                for index in 0..len as usize {
                    let item_raw = unsafe { q::JS_GetPropertyUint32(ctx, *raw, index as u32) };
                    let item = Owned {
                        ctx,
                        value: item_raw,
                    };
                    if item.value.tag == TAG_EXCEPTION {
                        return Err(JsError::Internal("could not read array element".into()));
                    }
                    values.push(to_value_inner(ctx, &item.value, visited)?);
                }
                Ok(JsValue::Array(values))
            } else {
                let identity = unsafe { raw.u.ptr as usize };
                if !visited.insert(identity) {
                    // Cyclic reference: cut the traversal.
                    return Ok(JsValue::Null);
                }
                let keys = object_keys(ctx, *raw)?;
                let mut map = std::collections::HashMap::new();
                for key in keys {
                    let ckey = make_cstring(&key)?;
                    let prop_raw = unsafe { q::JS_GetPropertyStr(ctx, *raw, ckey.as_ptr()) };
                    let prop = Owned {
                        ctx,
                        value: prop_raw,
                    };
                    if prop.value.tag == TAG_EXCEPTION {
                        return Err(JsError::Internal(format!(
                            "could not read property '{key}'"
                        )));
                    }
                    map.insert(key, to_value_inner(ctx, &prop.value, visited)?);
                }
                visited.remove(&identity);
                Ok(JsValue::Object(map))
            }
        }
        _ => Err(JsError::Internal(format!("unhandled JS tag: {}", raw.tag))),
    }
}

fn object_keys(ctx: *mut q::JSContext, obj: q::JSValue) -> Result<Vec<String>, JsError> {
    let global = global_object(ctx)?;
    let object = OwnedObject::new(global.property("Object")?)?;
    let keys = object.property("keys")?;
    let raw = unsafe { q::JS_Call(ctx, keys.value, object.value.value, 1, [obj].as_mut_ptr()) };
    let owned = Owned { ctx, value: raw };
    if owned.value.tag == TAG_EXCEPTION {
        return Err(exception(ctx));
    }
    match to_value(ctx, &owned.value)? {
        JsValue::Array(items) => {
            let mut out = Vec::new();
            for item in items {
                match item {
                    JsValue::String(s) => out.push(s),
                    _ => return Err(JsError::Internal("Object.keys returned non-string".into())),
                }
            }
            Ok(out)
        }
        _ => Err(JsError::Internal(
            "Object.keys did not return an array".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eval_and_call() {
        let runtime = Runtime::new().unwrap();
        assert_eq!(runtime.eval("1 + 2", "t.js").unwrap(), JsValue::Int(3));
        runtime
            .eval("function add(a, b) { return a + b; }", "t.js")
            .unwrap();
        assert_eq!(
            runtime
                .call_function("add", vec![JsValue::Int(2), JsValue::Int(5)])
                .unwrap(),
            JsValue::Int(7)
        );
    }

    #[test]
    fn promise_jobs_resolve() {
        let runtime = Runtime::new().unwrap();
        runtime
            .eval(
                "globalThis.__result = 0;\nasync function work() { globalThis.__result = await Promise.resolve(42); }",
                "t.js",
            )
            .unwrap();
        runtime
            .call_function("work", Vec::<JsValue>::new())
            .unwrap();
        runtime.pump_jobs();
        assert_eq!(
            runtime.eval("globalThis.__result", "t.js").unwrap(),
            JsValue::Int(42)
        );
    }

    #[test]
    fn json_bridge_callback() {
        let runtime = Runtime::new().unwrap();
        runtime
            .add_callback("echo", |method: String, payload: String| {
                format!("{method}:{payload}")
            })
            .unwrap();
        assert_eq!(
            runtime.eval(r#"echo("m", "p")"#, "t.js").unwrap(),
            JsValue::String("m:p".into())
        );
    }

    #[test]
    fn globals_roundtrip() {
        let runtime = Runtime::new().unwrap();
        runtime
            .set_global_json("__x", serde_json::json!({ "a": [1, 2] }))
            .unwrap();
        assert_eq!(
            runtime.global("__x").unwrap(),
            serde_json::json!({ "a": [1, 2] })
        );
    }

    #[test]
    fn object_readback() {
        let runtime = Runtime::new().unwrap();
        runtime
            .eval(
                "globalThis.__obj = { a: 1, b: \"two\", c: [true, null] };",
                "t.js",
            )
            .unwrap();
        let value = runtime.eval("globalThis.__obj", "t.js").unwrap();
        match value {
            JsValue::Object(map) => {
                assert_eq!(map.get("a"), Some(&JsValue::Int(1)));
                assert_eq!(map.get("b"), Some(&JsValue::String("two".into())));
            }
            other => panic!("expected object, got {other:?}"),
        }
    }

    #[test]
    fn callbacks_survive_runtime_move() {
        // Regression: the trampoline must not capture the Runtime's address.
        let runtime = Runtime::new().unwrap();
        runtime
            .add_callback("echo", |payload: String| payload)
            .unwrap();
        let moved = {
            // Force a move by returning through a wrapper.
            runtime
        };
        assert_eq!(
            moved.eval(r#"echo("hi")"#, "t.js").unwrap(),
            JsValue::String("hi".into())
        );
    }
}
