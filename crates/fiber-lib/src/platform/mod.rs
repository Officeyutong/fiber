cfg_if::cfg_if! {

    if #[cfg(not(target_arch = "wasm32"))]{
        mod native;
        pub use native::*;

    } else {
        mod wasm;
        pub use wasm::*;

    }
}
