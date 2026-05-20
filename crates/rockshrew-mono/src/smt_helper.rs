//! This module previously re-exported `metashrew_runtime::smt::SMTHelper` for
//! backward compatibility. The SMT subsystem has been deleted from
//! `metashrew-runtime` (v10 cleanup: the SMT was a no-op in the hot path and
//! the only callers stored state-root metadata that nothing consumed). Future
//! per-table root calculation happens inside the WASM indexer, not here.
//!
//! This file is kept as an empty module so external `mod smt_helper;`
//! declarations don't break; remove the file and the `pub mod smt_helper;`
//! line in `lib.rs` once no callers reference it.
