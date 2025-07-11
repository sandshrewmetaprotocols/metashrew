#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {
        let _ = writeln!($crate::stdout(), $($arg)*);
    };
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        let _ = write!($crate::stdout(), $($arg)*);
    };
}

pub mod wasm {
    use std::sync::Arc;

    pub fn to_ptr(v: &mut Vec<u8>) -> i32 {
        return v.as_mut_ptr() as usize as i32;
    }

    pub fn to_passback_ptr(v: &mut Vec<u8>) -> i32 {
        to_ptr(v) + 4
    }

    pub fn to_arraybuffer_layout<T: AsRef<[u8]>>(v: T) -> Vec<u8> {
        let mut buffer = Vec::<u8>::new();
        buffer.extend_from_slice(&(v.as_ref().len() as u32).to_le_bytes());
        buffer.extend_from_slice(v.as_ref());
        return buffer;
    }

    #[allow(unused_unsafe)]
    #[cfg(target_arch = "wasm32")]
    pub fn log(v: Arc<Vec<u8>>) -> () {
        extern "C" {
            fn __log(ptr: i32);
        }
        unsafe {
            __log(to_passback_ptr(&mut to_arraybuffer_layout(v.as_ref())));
        }
    }

    #[allow(unused_unsafe)]
    #[cfg(not(target_arch = "wasm32"))]
    pub fn log(_v: Arc<Vec<u8>>) -> () {
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    pub use std::fmt::{Error, Write};
    use std::sync::Arc;
    use super::wasm;

    pub struct Stdout(());

    impl Write for Stdout {
        fn write_str(&mut self, s: &str) -> Result<(), Error> {
            let data = Arc::new(s.to_string().as_bytes().to_vec());
            wasm::log(data.clone());
            return Ok(());
        }
    }

    pub fn stdout() -> Stdout {
        Stdout(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::io::Write;
    pub struct Stdout(std::io::Stdout);
    impl std::fmt::Write for Stdout {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            self.0.write_all(s.as_bytes()).map_err(|_| std::fmt::Error)
        }
    }
    pub fn stdout() -> Stdout {
        Stdout(std::io::stdout())
    }
}

pub use imp::{stdout, Stdout};
