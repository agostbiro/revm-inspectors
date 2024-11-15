//! Opcount tracing inspector that simply counts all opcodes.
//!
//! See also <https://geth.ethereum.org/docs/developers/evm-tracing/built-in-tracers>

use revm::{interpreter::Interpreter, EvmWiring, EvmContext};
use revm_inspector::Inspector;

/// An inspector that counts all opcodes.
#[derive(Clone, Copy, Debug, Default)]
pub struct OpcodeCountInspector {
    /// opcode counter
    count: usize,
}

impl OpcodeCountInspector {
    /// Returns the opcode counter
    #[inline]
    pub const fn count(&self) -> usize {
        self.count
    }
}

impl<EvmWiringT> Inspector<EvmWiringT> for OpcodeCountInspector
where
    EvmWiringT: EvmWiring,
{
    fn step(&mut self, _interp: &mut Interpreter, _context: &mut EvmContext<EvmWiringT>) {
        self.count += 1;
    }
}
