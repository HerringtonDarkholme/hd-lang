//! Native trampolines for C provider slots (#2388 step 6,
//! `docs/future-work/FFI.md`, "Binding on Wasmtime"). Each distinct C
//! signature gets one native function, compiled with Cranelift the first
//! time a slot of that signature is bound, and cached for the life of the
//! process. A trampoline takes the slot's address and its arguments' bits,
//! loads each argument at its own C type, and calls the slot with the
//! target's C calling convention. Cranelift places the arguments, in
//! registers and on the stack, as the C compiler does on every target it
//! supports, so there is no limit on the argument count and no
//! per-target bridge.
//!
//! The model is Deno's FFI turbocall (`ext/ffi/turbocall.rs`), which
//! compiles one Cranelift function per symbol and calls the symbol through
//! a constant address. hd differs in two ways:
//! - the slot's address is the trampoline's first argument, so one
//!   trampoline serves every slot of its signature, in every library;
//! - the code lives in one `cranelift-jit` module, which handles the
//!   executable memory and the instruction cache, where Deno maps its own
//!   pages with `memmap2`.
//!
//! Preflight asks for every bound slot's trampoline before any provider
//! is created, so a host Cranelift cannot target, or a failed compile,
//! refuses the run first; linking then finds each one in the cache.
//!
//! Cost. Compiling one signature is one Cranelift compile of a function
//! of `n` loads and one call, linear in `n`. A run compiles at most one
//! trampoline per distinct signature among the C slots it binds, which is
//! at most the number of C-bound methods. The process keeps each one, a
//! few hundred bytes of code packed into shared pages, so memory is linear
//! in the distinct signatures and never grows with calls. A call makes
//! one load per argument and one indirect call: the slot's own arguments,
//! nothing more.

use std::ffi::c_void;
use std::sync::{Arc, Mutex, PoisonError};

use cranelift_codegen::ir::{AbiParam, InstBuilder, MemFlagsData, Signature, types};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::Module;
use hd_base::HashMap;

/// A C parameter's or result's type, as a slot's signature has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CType {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
    /// `bool`: one byte, 0 or 1.
    Bool,
    /// A pointer or `size_t`: the target's pointer width.
    Ptr,
}

/// A slot's C signature: its parameters, `self` first, and its result.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Sig {
    pub params: Vec<CType>,
    pub result: Option<CType>,
}

/// One compiled trampoline:
/// `u64 trampoline(const void *slot, const uint64_t *args)`. Argument `k`
/// is in the low bytes of `args[k]`. The result comes back extended to 64
/// bits by its signedness, a float as its bits, and 0 for `void`.
pub struct Trampoline {
    entry: unsafe extern "C" fn(*const c_void, *const u64) -> u64,
    params: usize,
}

/// The process's trampolines by signature, and the module that holds
/// their code; `None` until the first compile.
struct Cache {
    module: Option<JITModule>,
    by_sig: HashMap<Sig, Arc<Trampoline>>,
    /// How many times each signature was compiled, for the tests.
    #[cfg(feature = "testing")]
    compiles: HashMap<Sig, usize>,
    /// The signatures whose compile the tests make fail.
    #[cfg(feature = "testing")]
    refused: hd_base::HashSet<Sig>,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

impl Cache {
    fn new() -> Self {
        Self {
            module: None,
            by_sig: HashMap::default(),
            #[cfg(feature = "testing")]
            compiles: HashMap::default(),
            #[cfg(feature = "testing")]
            refused: hd_base::HashSet::default(),
        }
    }
}

impl Trampoline {
    /// The trampoline of `sig`, compiled once per process.
    ///
    /// # Errors
    ///
    /// When Cranelift has no back end for this host.
    pub fn of(sig: &Sig) -> Result<Arc<Trampoline>, String> {
        let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
        let cache = cache.get_or_insert_with(Cache::new);
        #[cfg(feature = "testing")]
        if cache.refused.contains(sig) {
            return Err(format!(
                "the C call trampoline of {sig:?}: a compile failure the test asked for"
            ));
        }
        if let Some(t) = cache.by_sig.get(sig) {
            return Ok(Arc::clone(t));
        }
        if cache.module.is_none() {
            cache.module = Some(module()?);
        }
        let Some(module) = cache.module.as_mut() else {
            unreachable!("ruled out: set just above")
        };
        let t = Arc::new(compile(module, sig)?);
        #[cfg(feature = "testing")]
        {
            *cache.compiles.entry(sig.clone()).or_default() += 1;
        }
        cache.by_sig.insert(sig.clone(), Arc::clone(&t));
        Ok(t)
    }

    /// Calls the slot `f` with `args`, one per parameter of the signature.
    ///
    /// # Safety
    ///
    /// `f` is a C function of this trampoline's signature, and each
    /// argument's bits are a value of its parameter's type.
    #[allow(
        unsafe_code,
        clippy::must_use_candidate,
        reason = "a `void` slot's call is made for its effect"
    )]
    pub unsafe fn call(&self, f: *const c_void, args: &[u64]) -> u64 {
        debug_assert_eq!(args.len(), self.params, "a C call's arity");
        // SAFETY: the caller's contract; the trampoline reads exactly
        // `params` words of `args`.
        unsafe { (self.entry)(f, args.as_ptr()) }
    }
}

/// How many times `sig` was compiled in this process.
#[cfg(feature = "testing")]
#[must_use]
pub fn compiles(sig: &Sig) -> usize {
    let cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    cache
        .as_ref()
        .and_then(|c| c.compiles.get(sig).copied())
        .unwrap_or(0)
}

/// Makes every later compile of `sig` fail, as a host Cranelift cannot
/// target or a failed compile would.
#[cfg(feature = "testing")]
pub fn refuse(sig: Sig) {
    let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    cache.get_or_insert_with(Cache::new).refused.insert(sig);
}

/// The host's JIT module.
fn module() -> Result<JITModule, String> {
    let mut flags = settings::builder();
    let set = |flags: &mut settings::Builder, name: &str, value: &str| {
        flags
            .set(name, value)
            .map_err(|e| format!("Cranelift setting {name}: {e}"))
    };
    set(&mut flags, "opt_level", "speed")?;
    set(&mut flags, "is_pic", "false")?;
    let isa = cranelift_native::builder()
        .map_err(|why| format!("no C call trampolines on this host: {why}"))?
        .finish(settings::Flags::new(flags))
        .map_err(|why| format!("no C call trampolines on this host: {why}"))?;
    Ok(JITModule::new(JITBuilder::with_isa(
        isa,
        cranelift_module::default_libcall_names(),
    )))
}

/// The Cranelift parameter of `t`: a narrow integer extended by its
/// signedness, as the C calling conventions ask of a caller.
fn abi(t: CType, pointer: types::Type) -> AbiParam {
    match t {
        CType::I8 => AbiParam::new(types::I8).sext(),
        CType::U8 | CType::Bool => AbiParam::new(types::I8).uext(),
        CType::I16 => AbiParam::new(types::I16).sext(),
        CType::U16 => AbiParam::new(types::I16).uext(),
        CType::I32 | CType::U32 => AbiParam::new(types::I32),
        CType::I64 | CType::U64 => AbiParam::new(types::I64),
        CType::F32 => AbiParam::new(types::F32),
        CType::F64 => AbiParam::new(types::F64),
        CType::Ptr => AbiParam::new(pointer),
    }
}

/// Compiles the trampoline of `sig` into `module`.
#[allow(unsafe_code)]
fn compile(module: &mut JITModule, sig: &Sig) -> Result<Trampoline, String> {
    let fail = |why: String| format!("the C call trampoline of {sig:?}: {why}");
    let isa = module.isa();
    let pointer = isa.pointer_type();
    let call_conv = isa.default_call_conv();
    let frontend = isa.frontend_config();
    let mut target = Signature::new(call_conv);
    target
        .params
        .extend(sig.params.iter().map(|t| abi(*t, pointer)));
    // A result carries no extension: the callee leaves the bits above a
    // narrow result undefined, and the extension below makes them.
    target.returns.extend(
        sig.result
            .map(|t| AbiParam::new(abi(t, pointer).value_type)),
    );
    let mut outer = Signature::new(call_conv);
    outer
        .params
        .extend([AbiParam::new(pointer), AbiParam::new(pointer)]);
    outer.returns.push(AbiParam::new(types::I64));
    let id = module
        .declare_anonymous_function(&outer)
        .map_err(|e| fail(e.to_string()))?;
    let mut ctx = module.make_context();
    ctx.func.signature = outer;
    let mut fctx = FunctionBuilderContext::new();
    {
        let mut b = FunctionBuilder::new(&mut ctx.func, &mut fctx);
        let block = b.create_block();
        b.append_block_params_for_function_params(block);
        b.switch_to_block(block);
        b.seal_block(block);
        let (slot, args) = (b.block_params(block)[0], b.block_params(block)[1]);
        let flags = MemFlagsData::trusted();
        let mut values = Vec::with_capacity(sig.params.len());
        for (k, t) in sig.params.iter().enumerate() {
            let ty = abi(*t, pointer).value_type;
            let at = i32::try_from(k * 8).map_err(|_| fail("too many arguments".into()))?;
            values.push(b.ins().load(ty, flags, args, at));
        }
        let sigref = b.import_signature(target);
        let call = b.ins().call_indirect(sigref, slot, &values);
        let result = match sig.result {
            None => b.ins().iconst(types::I64, 0),
            Some(t) => {
                let r = b.inst_results(call)[0];
                match t {
                    CType::I8 | CType::I16 | CType::I32 => b.ins().sextend(types::I64, r),
                    CType::U8 | CType::Bool | CType::U16 | CType::U32 => {
                        b.ins().uextend(types::I64, r)
                    }
                    CType::I64 | CType::U64 => r,
                    CType::Ptr if pointer == types::I64 => r,
                    CType::Ptr => b.ins().uextend(types::I64, r),
                    CType::F32 => {
                        let bits = b.ins().bitcast(types::I32, MemFlagsData::new(), r);
                        b.ins().uextend(types::I64, bits)
                    }
                    CType::F64 => b.ins().bitcast(types::I64, MemFlagsData::new(), r),
                }
            }
        };
        b.ins().return_(&[result]);
        b.finalize(frontend);
    }
    module
        .define_function(id, &mut ctx)
        .map_err(|e| fail(format!("{e:?}")))?;
    module.clear_context(&mut ctx);
    module
        .finalize_definitions()
        .map_err(|e| fail(e.to_string()))?;
    let code = module.get_finalized_function(id);
    // SAFETY: the function just finalized has the signature `outer`,
    // `(pointer, pointer) -> i64` in the target's C calling convention.
    let entry = unsafe {
        std::mem::transmute::<*const u8, unsafe extern "C" fn(*const c_void, *const u64) -> u64>(
            code,
        )
    };
    Ok(Trampoline {
        entry,
        params: sig.params.len(),
    })
}
