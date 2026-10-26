#![deny(unsafe_code)]
//! The playground's lazily loaded WAT printer; independent of the compiler.

/// Prints folded instructions, matching the former compiler WAT output.
pub fn print(wasm: &[u8]) -> Result<String, String> {
    let mut text = String::new();
    wasmprinter::Config::new()
        .fold_instructions(true)
        .print(wasm, &mut wasmprinter::PrintFmtWrite(&mut text))
        .map_err(|error| error.to_string())?;
    Ok(text)
}

// Like hd_web's exports: no unsafe blocks or pointer dereferences. JS
// reads and writes owned buffers through the exported linear memory.
#[cfg(target_arch = "wasm32")]
#[allow(unsafe_code)]
mod exports {
    use std::cell::RefCell;
    #[derive(Default)]
    struct State {
        input: Vec<u8>,
        output: String,
    }
    thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }
    #[unsafe(no_mangle)]
    pub extern "C" fn hd_input(len: usize) -> *mut u8 {
        STATE.with_borrow_mut(|state| {
            state.input.clear();
            state.input.resize(len, 0);
            state.input.as_mut_ptr()
        })
    }
    #[unsafe(no_mangle)]
    pub extern "C" fn hd_print() -> u32 {
        STATE.with_borrow_mut(|state| match super::print(&state.input) {
            Ok(text) => {
                state.output = text;
                0
            }
            Err(error) => {
                state.output = error;
                1
            }
        })
    }
    #[unsafe(no_mangle)]
    pub extern "C" fn hd_output() -> *const u8 {
        STATE.with_borrow(|state| state.output.as_ptr())
    }
    #[unsafe(no_mangle)]
    pub extern "C" fn hd_output_len() -> usize {
        STATE.with_borrow(|state| state.output.len())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn prints_an_empty_module() {
        assert_eq!(super::print(b"\0asm\x01\0\0\0").unwrap(), "(module)\n");
    }
    #[test]
    fn invalid_bytes_are_an_error() {
        assert!(super::print(b"invalid").is_err());
    }
}
