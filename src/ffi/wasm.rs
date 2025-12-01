//! C-style FFI interface for WASM targets.
//!
//! This module provides a C-compatible interface for the JsonLogic library,
//! allowing it to be used from multiple languages through WASM.
//!
//! # Memory Model
//!
//! The module uses a single static buffer for output storage (1MB).
//! All responses are valid JSON - either the evaluation result or an error object.
//!
//! # Usage Pattern
//!
//! ```text
//! 1. Write logic JSON string to WASM memory
//! 2. Write data JSON string to WASM memory  
//! 3. Call apply_json_logic(logic_ptr, logic_len, data_ptr, data_len)
//! 4. The return value is a packed pointer: high 32 bits = pointer, low 32 bits = length
//! 5. Read `length` bytes from `pointer`
//! 6. Parse the JSON - it's either the result or {"error": "message"}
//! ```

use std::sync::Mutex;

/// Buffer size: 1MB for output
const BUFFER_SIZE: usize = 1024 * 1024;

/// Static buffer for storing output (results or errors).
/// Protected by a Mutex for thread safety (even though WASM is typically single-threaded).
static OUTPUT_BUFFER: Mutex<[u8; BUFFER_SIZE]> = Mutex::new([0u8; BUFFER_SIZE]);

/// Returns the maximum buffer size (1MB).
///
/// This function can be used by callers to determine the maximum size
/// of output that can be returned.
#[no_mangle]
pub extern "C" fn get_buffer_size() -> i32 {
    BUFFER_SIZE as i32
}

/// Writes output to the buffer and returns a packed pointer.
///
/// Returns a 64-bit value where:
/// - High 32 bits: pointer to the output buffer (truncated to 32 bits for WASM32)
/// - Low 32 bits: length of the output
fn write_output(output: &str) -> i64 {
    let bytes = output.as_bytes();

    match OUTPUT_BUFFER.lock() {
        Ok(mut buffer) => {
            let len = bytes.len().min(BUFFER_SIZE);
            buffer[..len].copy_from_slice(&bytes[..len]);
            
            // Pack pointer and length into a single i64
            // High 32 bits = pointer (as u32), Low 32 bits = length
            // In WASM32, pointers are 32-bit, so this truncation is safe
            let ptr = buffer.as_ptr() as usize as u32;
            ((ptr as i64) << 32) | (len as i64)
        }
        Err(_) => {
            // If we can't acquire the lock, return 0 (null pointer, zero length)
            0
        }
    }
}

/// Writes an error as JSON to the output buffer.
///
/// Returns a packed pointer (same format as write_output).
fn write_error(message: &str) -> i64 {
    let error_json = format!(r#"{{"error":"{}"}}"#, escape_json_string(message));
    write_output(&error_json)
}

/// Escapes special characters in a string for JSON.
fn escape_json_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c if c.is_control() => {
                result.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => result.push(c),
        }
    }
    result
}

/// Apply JSONLogic rules to data.
///
/// Takes two JSON strings via pointer + length and returns a packed pointer.
/// The output is always valid JSON, stored in the output buffer.
///
/// On success, the output is the JSON result of the evaluation.
/// On error, the output is a JSON object: `{"error": "error message"}`
///
/// # Arguments
///
/// * `logic_ptr` - Pointer to the JSON logic string
/// * `logic_len` - Length of the JSON logic string in bytes
/// * `data_ptr` - Pointer to the JSON data string
/// * `data_len` - Length of the JSON data string in bytes
///
/// # Returns
///
/// A packed 64-bit value where:
/// - High 32 bits: pointer to the output buffer
/// - Low 32 bits: length of the output in bytes
///
/// To unpack in the caller:
/// ```text
/// pointer = (result >> 32) & 0xFFFFFFFF
/// length = result & 0xFFFFFFFF
/// ```
///
/// # Safety
///
/// The caller must ensure that:
/// - `logic_ptr` points to valid memory of at least `logic_len` bytes
/// - `data_ptr` points to valid memory of at least `data_len` bytes
/// - Both memory regions contain valid UTF-8 encoded strings
/// - The pointers remain valid for the duration of the function call
#[no_mangle]
pub extern "C" fn apply_json_logic(
    logic_ptr: *const u8,
    logic_len: i32,
    data_ptr: *const u8,
    data_len: i32,
) -> i64 {
    // Validate input pointers
    if logic_ptr.is_null() {
        return write_error("logic pointer is null");
    }
    if data_ptr.is_null() {
        return write_error("data pointer is null");
    }

    // Validate lengths
    if logic_len < 0 {
        return write_error("logic length is negative");
    }
    if data_len < 0 {
        return write_error("data length is negative");
    }

    // Convert pointers to slices
    // SAFETY: We have validated that the pointers are not null and lengths are non-negative.
    // The caller must ensure the memory regions are valid and contain valid UTF-8.
    let logic_slice = unsafe {
        std::slice::from_raw_parts(logic_ptr, logic_len as usize)
    };
    let data_slice = unsafe {
        std::slice::from_raw_parts(data_ptr, data_len as usize)
    };

    // Convert slices to UTF-8 strings
    let logic_str = match std::str::from_utf8(logic_slice) {
        Ok(s) => s,
        Err(e) => return write_error(&format!("invalid UTF-8 in logic: {}", e)),
    };
    let data_str = match std::str::from_utf8(data_slice) {
        Ok(s) => s,
        Err(e) => return write_error(&format!("invalid UTF-8 in data: {}", e)),
    };

    // Parse JSON strings
    let logic_value: serde_json::Value = match serde_json::from_str(logic_str) {
        Ok(v) => v,
        Err(e) => return write_error(&format!("failed to parse logic JSON: {}", e)),
    };
    let data_value: serde_json::Value = match serde_json::from_str(data_str) {
        Ok(v) => v,
        Err(e) => return write_error(&format!("failed to parse data JSON: {}", e)),
    };

    // Apply JSONLogic
    match crate::apply(&logic_value, &data_value) {
        Ok(result) => {
            // Serialize the result to JSON
            match serde_json::to_string(&result) {
                Ok(json) => write_output(&json),
                Err(e) => write_error(&format!("failed to serialize result: {}", e)),
            }
        }
        Err(e) => write_error(&format!("evaluation error: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to get the length from packed result
    fn get_length(packed: i64) -> usize {
        (packed & 0xFFFFFFFF) as usize
    }

    /// Helper to read output from the buffer using the packed result length
    fn read_output(packed: i64) -> String {
        let len = get_length(packed);
        if len == 0 {
            return String::new();
        }
        let buffer = OUTPUT_BUFFER.lock().unwrap();
        std::str::from_utf8(&buffer[..len]).unwrap().to_string()
    }

    #[test]
    fn test_get_buffer_size() {
        assert_eq!(get_buffer_size(), 1024 * 1024);
    }

    #[test]
    fn test_apply_json_logic_valid() {
        let logic = r#"{"==": [1, 1]}"#;
        let data = r#"{}"#;

        let result = apply_json_logic(
            logic.as_ptr(),
            logic.len() as i32,
            data.as_ptr(),
            data.len() as i32,
        );

        let len = get_length(result);
        assert!(len > 0);

        let output = read_output(result);
        assert_eq!(output, "true");
    }

    #[test]
    fn test_apply_json_logic_with_data() {
        let logic = r#"{"var": "foo"}"#;
        let data = r#"{"foo": "bar"}"#;

        let result = apply_json_logic(
            logic.as_ptr(),
            logic.len() as i32,
            data.as_ptr(),
            data.len() as i32,
        );

        let output = read_output(result);
        assert_eq!(output, r#""bar""#);
    }

    #[test]
    fn test_apply_json_logic_invalid_logic_json() {
        let logic = r#"{"invalid json"#;
        let data = r#"{}"#;

        let result = apply_json_logic(
            logic.as_ptr(),
            logic.len() as i32,
            data.as_ptr(),
            data.len() as i32,
        );

        let output = read_output(result);
        assert!(output.contains("error"));
        assert!(output.contains("parse logic JSON"));
    }

    #[test]
    fn test_apply_json_logic_invalid_data_json() {
        let logic = r#"{"==": [1, 1]}"#;
        let data = r#"{"invalid json"#;

        let result = apply_json_logic(
            logic.as_ptr(),
            logic.len() as i32,
            data.as_ptr(),
            data.len() as i32,
        );

        let output = read_output(result);
        assert!(output.contains("error"));
        assert!(output.contains("parse data JSON"));
    }

    #[test]
    fn test_apply_json_logic_null_logic_ptr() {
        let data = r#"{}"#;

        let result = apply_json_logic(
            std::ptr::null(),
            0,
            data.as_ptr(),
            data.len() as i32,
        );

        let output = read_output(result);
        assert!(output.contains("error"));
        assert!(output.contains("logic pointer is null"));
    }

    #[test]
    fn test_apply_json_logic_null_data_ptr() {
        let logic = r#"{"==": [1, 1]}"#;

        let result = apply_json_logic(
            logic.as_ptr(),
            logic.len() as i32,
            std::ptr::null(),
            0,
        );

        let output = read_output(result);
        assert!(output.contains("error"));
        assert!(output.contains("data pointer is null"));
    }

    #[test]
    fn test_apply_json_logic_negative_length() {
        let logic = r#"{"==": [1, 1]}"#;
        let data = r#"{}"#;

        let result = apply_json_logic(
            logic.as_ptr(),
            -1,
            data.as_ptr(),
            data.len() as i32,
        );

        let output = read_output(result);
        assert!(output.contains("error"));
        assert!(output.contains("logic length is negative"));
    }

    #[test]
    fn test_apply_json_logic_evaluation_error() {
        // This logic causes an evaluation error - wrong number of arguments
        let logic = r#"{"==": [1]}"#;
        let data = r#"{}"#;

        let result = apply_json_logic(
            logic.as_ptr(),
            logic.len() as i32,
            data.as_ptr(),
            data.len() as i32,
        );

        let output = read_output(result);
        assert!(output.contains("error"));
    }

    #[test]
    fn test_escape_json_string() {
        assert_eq!(escape_json_string("hello"), "hello");
        assert_eq!(escape_json_string("hello\"world"), "hello\\\"world");
        assert_eq!(escape_json_string("hello\\world"), "hello\\\\world");
        assert_eq!(escape_json_string("hello\nworld"), "hello\\nworld");
        assert_eq!(escape_json_string("hello\rworld"), "hello\\rworld");
        assert_eq!(escape_json_string("hello\tworld"), "hello\\tworld");
    }

    #[test]
    fn test_packed_pointer_format() {
        let logic = r#"{"==": [1, 1]}"#;
        let data = r#"{}"#;

        let result = apply_json_logic(
            logic.as_ptr(),
            logic.len() as i32,
            data.as_ptr(),
            data.len() as i32,
        );

        // Verify the packed format
        let ptr = ((result >> 32) & 0xFFFFFFFF) as u32;
        let len = (result & 0xFFFFFFFF) as u32;
        
        assert!(ptr != 0, "Pointer should not be null");
        assert_eq!(len, 4, "Length should be 4 for 'true'");
    }
}
