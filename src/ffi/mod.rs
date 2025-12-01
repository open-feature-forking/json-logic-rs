//! FFI interface for WASM and other targets.
//!
//! This module provides a C-style FFI interface for the JsonLogic library,
//! enabling use from multiple languages (Java/Chicory, JavaScript, Python, Go, .NET)
//! through a single WASM module.

// Compile for WASM targets, and also for tests on any target
#[cfg(any(target_arch = "wasm32", test))]
pub mod wasm;
