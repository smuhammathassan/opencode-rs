use std::{
    convert::TryFrom,
    ffi::CString,
    marker::PhantomData,
    os::raw::{c_int, c_void},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
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
        let ptr = value.u.ptr as *mut q::JSRefCountHeader;
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

type WrappedCallback =
    dyn Fn(*mut q::JSContext, c_int, *mut q::JSValue) -> q::JSValue + Send + Sync;
type CallbackRegistry = Vec<Arc<WrappedCallback>>;
type CallbackRegistryHandle = Arc<Mutex<CallbackRegistry>>;

/// The only payload stored in a QuickJS C-function value is a canonical
/// integer callback index. The closure itself is kept in the context-owned
/// Rust registry below; no Rust pointer is fabricated as a `JSValue`.
unsafe extern "C" fn callback_trampoline(
    ctx: *mut q::JSContext,
    _this: q::JSValue,
    argc: c_int,
    argv: *mut q::JSValue,
    _magic: c_int,
    data: *mut q::JSValue,
) -> q::JSValue {
    if ctx.is_null() || data.is_null() || (*data).tag != TAG_INT {
        return callback_exception(ctx, "invalid plugin callback handle");
    }
    let index = (*data).u.int32;
    if index < 0 {
        return callback_exception(ctx, "invalid plugin callback index");
    }
    let opaque = q::JS_GetContextOpaque(ctx);
    if opaque.is_null() {
        return callback_exception(ctx, "plugin callback registry is unavailable");
    }
    let registry = &*(opaque as *const CallbackRegistryHandle);
    let callback = match registry.lock() {
        Ok(registry) => registry.get(index as usize).cloned(),
        Err(_) => None,
    };
    match callback {
        Some(callback) => callback(ctx, argc, argv),
        None => callback_exception(ctx, "plugin callback handle is no longer registered"),
    }
}

// ---------------------------------------------------------------------------
// Execution limits (interrupt handler + memory guard)
// ---------------------------------------------------------------------------

/// Resource limits for one QuickJS runtime. The memory limit is a hard cap
/// (`JS_SetMemoryLimit`); the instruction and time budgets are enforced
/// cooperatively by QuickJS's interrupt handler while JS is executing.
#[derive(Debug, Clone, Copy, Default)]
pub struct RuntimeLimits {
    /// Maximum number of interpreter ticks before execution is aborted.
    pub instruction_budget: Option<u64>,
    /// Wall-clock budget for one execution.
    pub time_budget: Option<Duration>,
    /// Hard memory cap in bytes.
    pub memory_limit: Option<usize>,
}

/// The interrupt handler's shared state. The handler is a C callback, so all
/// communication happens through atomics on a boxed value kept at a stable
/// address for the runtime's lifetime.
#[repr(C)]
struct InterruptState {
    active: AtomicBool,
    deadline_ms: AtomicU64,
    budget: AtomicU64,
    ticks: AtomicU64,
    interrupted: AtomicBool,
}

impl InterruptState {
    fn new() -> Self {
        Self {
            active: AtomicBool::new(false),
            deadline_ms: AtomicU64::new(0),
            budget: AtomicU64::new(0),
            ticks: AtomicU64::new(0),
            interrupted: AtomicBool::new(false),
        }
    }
}

/// Check the budget every N ticks; QuickJS calls the handler frequently
/// enough that a coarser cadence keeps the overhead negligible.
const CHECK_INTERVAL: u64 = 1024;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// # Safety
/// `opaque` must point to an [`InterruptState`] that outlives the handler.
unsafe extern "C" fn interrupt_handler(_rt: *mut q::JSRuntime, opaque: *mut c_void) -> c_int {
    if opaque.is_null() {
        return 0;
    }
    let state = &*(opaque as *const InterruptState);
    if !state.active.load(Ordering::Acquire) {
        return 0;
    }
    let ticks = state.ticks.fetch_add(1, Ordering::Relaxed) + 1;
    if ticks % CHECK_INTERVAL != 0 {
        return 0;
    }
    let budget = state.budget.load(Ordering::Relaxed);
    if budget != 0 && ticks >= budget {
        state.interrupted.store(true, Ordering::Release);
        return 1;
    }
    let deadline = state.deadline_ms.load(Ordering::Relaxed);
    if deadline != 0 && now_ms() >= deadline {
        state.interrupted.store(true, Ordering::Release);
        return 1;
    }
    0
}

unsafe fn callback_exception(ctx: *mut q::JSContext, message: &str) -> q::JSValue {
    if ctx.is_null() {
        return q::JSValue {
            u: q::JSValueUnion { int32: 0 },
            tag: TAG_EXCEPTION,
        };
    }
    let Ok(message) = CString::new(message) else {
        return q::JSValue {
            u: q::JSValueUnion { int32: 0 },
            tag: TAG_EXCEPTION,
        };
    };
    let value = q::JS_NewString(ctx, message.as_ptr());
    if value.tag == TAG_EXCEPTION {
        return value;
    }
    q::JS_Throw(ctx, value)
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
    callbacks: CallbackRegistryHandle,
    interrupt: *mut InterruptState,
}

impl Runtime {
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
        // The bundled QuickJS default is intentionally small and is easily
        // exhausted by promise reactions plus the plugin bridge call frame.
        // The runtime is confined to a dedicated host thread with matching
        // native stack headroom (see PluginManager), so raise QuickJS's own
        // guard as well.
        unsafe { q::JS_SetMaxStackSize(ctx, 8 * 1024 * 1024) };
        // JS_NewContext already registers the standard intrinsics (including
        // Promise, Map/Set and TypedArrays) via js_standard_init.
        let callbacks: CallbackRegistryHandle = Arc::new(Mutex::new(Vec::new()));
        // QuickJS owns the context lifetime; keep one Arc handle reachable
        // through its opaque slot so static C trampolines can safely resolve
        // callback indices without embedding Rust pointers in JSValue data.
        let callback_opaque = Box::into_raw(Box::new(Arc::clone(&callbacks))) as *mut c_void;
        unsafe { q::JS_SetContextOpaque(ctx, callback_opaque) };
        // Install the execution-limits interrupt handler. The state is boxed
        // so its address is stable for the runtime's lifetime.
        let interrupt = Box::into_raw(Box::new(InterruptState::new()));
        unsafe { q::JS_SetInterruptHandler(rt, Some(interrupt_handler), interrupt as *mut c_void) };
        let runtime = Self {
            rt,
            ctx,
            callbacks,
            interrupt,
        };
        // This bundled QuickJS predates the `globalThis` standard; polyfill it
        // so the polyfill runtime and plugin code can rely on it.
        runtime.eval(
            "if (typeof globalThis === \"undefined\") { globalThis = new Function(\"return this\")(); }",
            "opencode-globalthis.js",
        )?;
        Ok(runtime)
    }

    /// Apply persistent resource limits: a hard memory cap and a default
    /// instruction budget. The time budget, when present, is treated as a
    /// per-execution budget armed by the next [`Runtime::arm_time_budget`]
    /// call.
    pub fn set_limits(&self, limits: RuntimeLimits) {
        if let Some(memory) = limits.memory_limit {
            unsafe { q::JS_SetMemoryLimit(self.rt, memory) };
        }
        if let Some(budget) = limits.instruction_budget {
            self.interrupt_state()
                .budget
                .store(budget, Ordering::Release);
        }
        if let Some(time) = limits.time_budget {
            let deadline = now_ms().saturating_add(time.as_millis() as u64);
            self.interrupt_state()
                .deadline_ms
                .store(deadline, Ordering::Release);
            self.interrupt_state().active.store(true, Ordering::Release);
        }
    }

    /// Arm the wall-clock budget for the next execution. Call
    /// [`Runtime::disarm_interrupt`] once the call completes.
    pub fn arm_time_budget(&self, budget: Duration) {
        self.reset_interrupt();
        let deadline = now_ms().saturating_add(budget.as_millis() as u64);
        self.interrupt_state()
            .deadline_ms
            .store(deadline, Ordering::Release);
        self.interrupt_state().active.store(true, Ordering::Release);
    }

    /// Arm an instruction budget for the next execution.
    pub fn arm_instruction_budget(&self, budget: u64) {
        self.reset_interrupt();
        self.interrupt_state()
            .budget
            .store(budget, Ordering::Release);
        self.interrupt_state().active.store(true, Ordering::Release);
    }

    /// Disarm the interrupt handler after an execution.
    pub fn disarm_interrupt(&self) {
        let state = self.interrupt_state();
        state.active.store(false, Ordering::Release);
    }

    /// Whether the interrupt handler fired (execution was aborted by limits).
    pub fn was_interrupted(&self) -> bool {
        self.interrupt_state().interrupted.load(Ordering::Acquire)
    }

    /// Clear the interrupted flag and any armed budgets.
    pub fn reset_interrupt(&self) {
        let state = self.interrupt_state();
        state.interrupted.store(false, Ordering::Release);
        state.ticks.store(0, Ordering::Relaxed);
        state.budget.store(0, Ordering::Release);
        state.deadline_ms.store(0, Ordering::Release);
        state.active.store(false, Ordering::Release);
    }

    /// Read QuickJS's own view of the runtime's memory usage.
    pub fn memory_usage(&self) -> q::JSMemoryUsage {
        let mut usage: q::JSMemoryUsage = unsafe { std::mem::zeroed() };
        unsafe { q::JS_ComputeMemoryUsage(self.rt, &mut usage) };
        usage
    }

    fn interrupt_state(&self) -> &InterruptState {
        // The pointer is installed at construction and only freed in Drop.
        unsafe { &*self.interrupt }
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
        let _ = self.pump_jobs_with(|| Ok(()));
    }

    fn pump_jobs_with(
        &self,
        mut before_job: impl FnMut() -> Result<(), JsError>,
    ) -> Result<(), JsError> {
        loop {
            let pending = unsafe { q::JS_IsJobPending(self.rt) };
            if pending == 0 {
                return Ok(());
            }
            before_job()?;
            let mut ctx = self.ctx;
            let result = unsafe { q::JS_ExecutePendingJob(self.rt, &mut ctx) };
            // An execution-limit abort surfaces as an uncatchable job error:
            // `JS_ExecutePendingJob` still returns success, so the interrupt
            // flag is the reliable signal. Clear the pending exception so the
            // context stays usable for later calls.
            if self.was_interrupted() {
                let exception = unsafe { q::JS_GetException(self.ctx) };
                let _ = Owned {
                    ctx: self.ctx,
                    value: exception,
                };
                return Err(JsError::Limit(
                    "plugin execution exceeded its resource budget".into(),
                ));
            }
            if result < 0 {
                return Ok(());
            }
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

    /// Call a promise-producing global function and keep its returned promise
    /// alive while QuickJS drains the job queue. Dropping that value before
    /// the jobs settle can leave QuickJS with a dangling promise reference.
    pub fn call_function_and_pump(
        &self,
        name: &str,
        args: impl IntoIterator<Item = impl Into<JsValue>>,
    ) -> Result<(), JsError> {
        self.call_function_and_pump_with_probe(name, args, || Ok(()))
    }

    /// Call a promise-producing global function and run a probe before each
    /// pending job. The probe runs on the QuickJS owner thread and may enqueue
    /// additional promise work, which is useful for cooperative signals that
    /// must notify JS listeners while an async tool is being pumped.
    pub fn call_function_and_pump_with_probe(
        &self,
        name: &str,
        args: impl IntoIterator<Item = impl Into<JsValue>>,
        mut before_job: impl FnMut() -> Result<(), JsError>,
    ) -> Result<(), JsError> {
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
        self.pump_jobs_with(&mut before_job)?;
        // Keep the JS call frame and all argument values rooted until every
        // promise reaction has run. Drop them explicitly only after the pump;
        // otherwise their last use can be earlier than the async resumption.
        drop(owned);
        drop(qargs);
        drop(func);
        Ok(())
    }

    /// Like [`Runtime::call_function_and_pump_with_probe`], but arms the
    /// execution-limits interrupt handler for the duration of the job pump.
    ///
    /// The budget is deliberately *not* armed around the initial `JS_Call`:
    /// this QuickJS build cannot safely recover from an interrupt that fires
    /// inside the synchronous prefix of an async function (it corrupts the
    /// pending promise state), so runaway *synchronous* prefixes are out of
    /// scope. Loops that run inside promise jobs are aborted cleanly.
    pub fn call_function_and_pump_with_probe_limited(
        &self,
        name: &str,
        args: impl IntoIterator<Item = impl Into<JsValue>>,
        before_job: impl FnMut() -> Result<(), JsError>,
        budget: Option<Duration>,
    ) -> Result<(), JsError> {
        // Always clear a stale interrupt flag from a previous aborted call so
        // an unarmed execution cannot spuriously report a limit abort.
        self.reset_interrupt();
        if let Some(budget) = budget {
            self.arm_time_budget(budget);
        }
        let result = self.call_function_and_pump_with_probe(name, args, before_job);
        if budget.is_some() {
            self.disarm_interrupt();
        }
        result
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
        let wrapper =
            move |ctx: *mut q::JSContext, argc: c_int, argv: *mut q::JSValue| -> q::JSValue {
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
        let index = {
            let mut callbacks = self.callbacks.lock().unwrap();
            let index = i32::try_from(callbacks.len())
                .map_err(|_| JsError::Internal("too many plugin callbacks".into()))?;
            callbacks.push(Arc::new(wrapper));
            index
        };
        let mut data = q::JSValue {
            u: q::JSValueUnion { int32: index },
            tag: TAG_INT,
        };
        let cfunc = unsafe {
            q::JS_NewCFunctionData(
                self.ctx,
                Some(callback_trampoline),
                argcount,
                0,
                1,
                &mut data,
            )
        };
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
            let callback_opaque = q::JS_GetContextOpaque(self.ctx);
            // Detach the interrupt handler before freeing the context so it
            // never runs against a half-destroyed runtime.
            q::JS_SetInterruptHandler(rt, None, std::ptr::null_mut());
            q::JS_SetContextOpaque(self.ctx, std::ptr::null_mut());
            q::JS_FreeContext(self.ctx);
            if !callback_opaque.is_null() {
                drop(Box::from_raw(
                    callback_opaque as *mut CallbackRegistryHandle,
                ));
            }
            q::JS_FreeRuntime(rt);
            if !self.interrupt.is_null() {
                drop(Box::from_raw(self.interrupt));
            }
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

    #[test]
    fn time_budget_aborts_runaway_loop() {
        let runtime = Runtime::new().unwrap();
        runtime
            .eval("function spin() { while (true) { } }", "t.js")
            .unwrap();
        runtime.arm_time_budget(std::time::Duration::from_millis(150));
        let result = runtime.call_function("spin", Vec::<JsValue>::new());
        assert!(result.is_err(), "runaway loop should be aborted");
        assert!(runtime.was_interrupted());
        runtime.disarm_interrupt();
        runtime.reset_interrupt();
        // The runtime remains usable afterwards.
        assert_eq!(runtime.eval("1 + 1", "t.js").unwrap(), JsValue::Int(2));
    }

    #[test]
    fn time_budget_aborts_pumped_async_call() {
        let runtime = Runtime::new().unwrap();
        // The loop runs inside a promise job (after an await), not in the
        // synchronous prefix of the async function.
        runtime
            .eval(
                "async function spin() { await Promise.resolve(); while (true) { } }",
                "t.js",
            )
            .unwrap();
        runtime.arm_time_budget(std::time::Duration::from_millis(150));
        let result = runtime.call_function_and_pump("spin", Vec::<JsValue>::new());
        assert!(matches!(result, Err(JsError::Limit(_))));
        assert!(
            runtime.was_interrupted(),
            "interrupt flag should be set after the abort"
        );
        runtime.disarm_interrupt();
        runtime.reset_interrupt();
    }

    #[test]
    fn memory_limit_is_enforced() {
        let runtime = Runtime::new().unwrap();
        runtime.set_limits(RuntimeLimits {
            memory_limit: Some(2 * 1024 * 1024),
            ..RuntimeLimits::default()
        });
        let result = runtime.eval(
            "const leak = []; while (true) { leak.push(new Array(1024).fill('x')); }",
            "t.js",
        );
        assert!(
            result.is_err(),
            "allocation over the memory limit should fail"
        );
    }
}
