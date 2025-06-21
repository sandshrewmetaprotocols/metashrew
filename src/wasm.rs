//! WASM module handling for Metashrew

/// Include the metashrew-minimal WASM module
include!(concat!(env!("OUT_DIR"), "/metashrew_minimal_wasm.rs"));

/// Get the metashrew-minimal WASM module as a byte array
pub fn get_metashrew_minimal_wasm() -> &'static [u8] {
    METASHREW_MINIMAL_WASM
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_wasm_module_exists() {
        let wasm = get_metashrew_minimal_wasm();
        // Check that the WASM module starts with the WASM magic bytes
        assert_eq!(&wasm[0..4], b"\0asm", "WASM module should start with the WASM magic bytes");
    }
}