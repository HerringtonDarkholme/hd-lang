//! Off-band source origins. They never participate in optimization or encoding.

use std::ops::Range;

/// A module and its optional development-tool source information.
#[derive(Clone, Debug)]
pub struct MappedModule {
    pub wasm: Vec<u8>,
    pub functions: Vec<FunctionOrigins>,
}

/// Source origins of one final Wasm function, including generated functions.
#[derive(Clone, Debug)]
pub struct FunctionOrigins {
    pub index: u32,
    pub name: String,
    /// The source TIR item's stable path; generated runtime helpers have none.
    pub item: Option<String>,
    /// Origins before module optimization/lowering, for pass regression checks.
    pub before_opt: Vec<Option<u32>>,
    /// One entry per encoded instruction, including the final implicit `end`.
    pub instructions: Vec<InstructionOrigin>,
}

/// One encoded instruction's offset and originating TIR instruction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstructionOrigin {
    /// Byte offset within the function body, including its local declarations.
    pub offset: u32,
    /// An instruction of `FunctionOrigins::item`, resolvable through its syntax map.
    pub origin: Option<u32>,
}

/// A rewrite's source table and output table. Copy unchanged ranges; merged
/// or expanded instructions take the first available origin of their inputs.
/// Removed instructions leave no output entry. Disabled maps allocate nothing.
pub(crate) struct Remap {
    source: Option<Vec<Option<u32>>>,
    output: Option<Vec<Option<u32>>>,
}

impl Remap {
    pub fn new(source: Option<&[Option<u32>]>) -> Self {
        Self {
            source: source.map(<[_]>::to_vec),
            output: source.map(|s| Vec::with_capacity(s.len())),
        }
    }

    pub fn first(&self, range: Range<usize>) -> Option<u32> {
        self.source
            .as_ref()?
            .get(range)?
            .iter()
            .copied()
            .flatten()
            .next()
    }

    pub fn copy(&mut self, range: Range<usize>) {
        if let (Some(source), Some(output)) = (&self.source, &mut self.output) {
            output.extend_from_slice(&source[range]);
        }
    }

    pub fn merged(&mut self, range: Range<usize>, count: usize) {
        let origin = self.first(range);
        self.repeat(origin, count);
    }

    pub fn repeat(&mut self, origin: Option<u32>, count: usize) {
        if let Some(output) = &mut self.output {
            output.extend(std::iter::repeat_n(origin, count));
        }
    }

    pub fn pop(&mut self) {
        if let Some(output) = &mut self.output {
            output.pop();
        }
    }

    pub fn finish(self, f: &mut crate::module::Func) {
        if let Some(output) = &self.output {
            assert_eq!(output.len(), f.body.len(), "instruction origin alignment");
        }
        f.origins = self.output;
    }
}
