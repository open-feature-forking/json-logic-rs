//! C-style FFI interface for WASM targets.
//!
//! This module provides a C-compatible interface for the JsonLogic library,
//! allowing it to be used from multiple languages through WASM.
//!
//! # Memory Model
//!
//! The module uses static mutable buffers for result and error storage:
//! - Result buffer: 1MB for storing successful evaluation results
//! - Error buffer: 1MB for storing error messages
//!
//! # Usage Pattern
//!
//! ```text
//! 1. Allocate memory in WASM for logic JSON string
//! 2. Write logic JSON to that memory
//! 3. Allocate memory for data JSON string
//! 4. Write data JSON to that memory
//! 5. Call apply_json_logic(logic_ptr, logic_len, data_ptr, data_len)
//! 6. If result >= 0: call get_result_ptr() and read result_len bytes
//! 7. If result < 0: call get_error_ptr() and read abs(result) bytes
//! ```
//!
//! # Testing Recommendations
//!
//! The following test cases should be covered:
//! - Test with valid JSON logic and data
//! - Test with invalid JSON in logic parameter
//! - Test with invalid JSON in data parameter
//! - Test with logic that causes evaluation errors
//! - Test with very large results that might exceed buffer
//! - Test with null/empty inputs
//! - Test buffer clearing functionality

use std::sync::Mutex;

/// Buffer size: 1MB for both result and error buffers
const BUFFER_SIZE: usize = 1024 * 1024;

/// Static buffer for storing successful results.
/// Protected by a Mutex for thread safety (even though WASM is typically single-threaded).
static RESULT_BUFFER: Mutex<[u8; BUFFER_SIZE]> = Mutex::new([0u8; BUFFER_SIZE]);

/// Static buffer for storing error messages.
/// Protected by a Mutex for thread safety.
static ERROR_BUFFER: Mutex<[u8; BUFFER_SIZE]> = Mutex::new([0u8; BUFFER_SIZE]);

/// Returns the maximum buffer size (1MB).
///
/// This function can be used by callers to determine the maximum size
/// of results or errors that can be returned.
#[no_mangle]
pub extern "C" fn get_buffer_size() -> i32 {
    BUFFER_SIZE as i32
}

/// Clears both result and error buffers (sets all bytes to 0).
///
/// This function should be called before making a new `apply_json_logic` call
/// if you want to ensure clean buffers.
#[no_mangle]
pub extern "C" fn clear_buffers() {
    // SAFETY: We acquire the mutex lock before modifying the buffer.
    // The mutex ensures exclusive access to the buffer.
    if let Ok(mut result) = RESULT_BUFFER.lock() {
        result.fill(0);
    }
    if let Ok(mut error) = ERROR_BUFFER.lock() {
        error.fill(0);
    }
}

/// Returns pointer to the result buffer.
///
/// After a successful call to `apply_json_logic` (return value >= 0),
/// use this pointer to read the result. The number of bytes to read
/// is the return value of `apply_json_logic`.
#[no_mangle]
pub extern "C" fn get_result_ptr() -> *const u8 {
    // SAFETY: We return a pointer to the static buffer.
    // The buffer has a fixed address and lifetime.
    // The caller must ensure they don't read more bytes than returned by apply_json_logic.
    match RESULT_BUFFER.lock() {
        Ok(guard) => guard.as_ptr(),
        Err(_) => std::ptr::null(),
    }
}

/// Returns pointer to the error buffer.
///
/// After a failed call to `apply_json_logic` (return value < 0),
/// use this pointer to read the error message. The number of bytes to read
/// is the absolute value of the return value of `apply_json_logic`.
#[no_mangle]
pub extern "C" fn get_error_ptr() -> *const u8 {
    // SAFETY: We return a pointer to the static buffer.
    // The buffer has a fixed address and lifetime.
    // The caller must ensure they don't read more bytes than abs(return value) of apply_json_logic.
    match ERROR_BUFFER.lock() {
        Ok(guard) => guard.as_ptr(),
        Err(_) => std::ptr::null(),
    }
}

/// Writes an error message to the error buffer.
///
/// Returns the negative length of the error message (for use as return value).
fn write_error(message: &str) -> i32 {
    let error_json = format!(r#"{{"error": "{}"}}"#, escape_json_string(message));
    let bytes = error_json.as_bytes();

    match ERROR_BUFFER.lock() {
        Ok(mut error_buffer) => {
            let len = bytes.len().min(BUFFER_SIZE);
            error_buffer[..len].copy_from_slice(&bytes[..len]);

            // If the message was truncated, indicate that
            if bytes.len() > BUFFER_SIZE {
                // Negative length indicates error
                -(BUFFER_SIZE as i32)
            } else {
                -(len as i32)
            }
        }
        Err(_) => {
            // If we can't acquire the lock, return a minimal error
            -1
        }
    }
}

/// Writes a result to the result buffer.
///
/// Returns the positive length of the result.
fn write_result(result: &str) -> i32 {
    let bytes = result.as_bytes();

    match RESULT_BUFFER.lock() {
        Ok(mut result_buffer) => {
            let len = bytes.len().min(BUFFER_SIZE);
            result_buffer[..len].copy_from_slice(&bytes[..len]);

            // If the result was truncated, we still return the truncated length
            // The caller should check against get_buffer_size() to detect truncation
            len as i32
        }
        Err(_) => write_error("Failed to acquire result buffer lock"),
    }
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
/// Takes two JSON strings via pointer + length and returns:
/// - Positive i32 for success: the length of the result stored in the result buffer
/// - Negative i32 for error: the negative length of the error stored in the error buffer
///
/// # Arguments
///
/// * `logic_ptr` - Pointer to the JSON logic string
/// * `logic_len` - Length of the JSON logic string in bytes
/// * `data_ptr` - Pointer to the JSON data string
/// * `data_len` - Length of the JSON data string in bytes
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
) -> i32 {
    // Validate input pointers
    if logic_ptr.is_null() {
        return write_error("Logic pointer is null");
    }
    if data_ptr.is_null() {
        return write_error("Data pointer is null");
    }

    // Validate lengths
    if logic_len < 0 {
        return write_error("Logic length is negative");
    }
    if data_len < 0 {
        return write_error("Data length is negative");
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
        Err(e) => return write_error(&format!("Invalid UTF-8 in logic: {}", e)),
    };
    let data_str = match std::str::from_utf8(data_slice) {
        Ok(s) => s,
        Err(e) => return write_error(&format!("Invalid UTF-8 in data: {}", e)),
    };

    // Parse JSON strings
    let logic_value: serde_json::Value = match serde_json::from_str(logic_str) {
        Ok(v) => v,
        Err(e) => return write_error(&format!("Failed to parse logic JSON: {}", e)),
    };
    let data_value: serde_json::Value = match serde_json::from_str(data_str) {
        Ok(v) => v,
        Err(e) => return write_error(&format!("Failed to parse data JSON: {}", e)),
    };

    // Apply JSONLogic
    match crate::apply(&logic_value, &data_value) {
        Ok(result) => {
            // Serialize the result to JSON
            match serde_json::to_string(&result) {
                Ok(json) => write_result(&json),
                Err(e) => write_error(&format!("Failed to serialize result: {}", e)),
            }
        }
        Err(e) => write_error(&format!("JSONLogic evaluation error: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_buffer_size() {
        assert_eq!(get_buffer_size(), 1024 * 1024);
    }

    #[test]
    fn test_clear_buffers() {
        // Write something to buffers
        write_result("test");
        write_error("test error");

        // Clear buffers
        clear_buffers();

        // Verify buffers are cleared
        let result = RESULT_BUFFER.lock().unwrap();
        assert!(result.iter().all(|&b| b == 0));
        let error = ERROR_BUFFER.lock().unwrap();
        assert!(error.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_get_result_ptr() {
        let ptr = get_result_ptr();
        assert!(!ptr.is_null());
    }

    #[test]
    fn test_get_error_ptr() {
        let ptr = get_error_ptr();
        assert!(!ptr.is_null());
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

        assert!(result > 0);

        // Read the result
        let result_buffer = RESULT_BUFFER.lock().unwrap();
        let result_str = std::str::from_utf8(&result_buffer[..result as usize]).unwrap();
        assert_eq!(result_str, "true");
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

        assert!(result > 0);

        let result_buffer = RESULT_BUFFER.lock().unwrap();
        let result_str = std::str::from_utf8(&result_buffer[..result as usize]).unwrap();
        assert_eq!(result_str, r#""bar""#);
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

        assert!(result < 0);

        // Read the error
        let error_buffer = ERROR_BUFFER.lock().unwrap();
        let error_str = std::str::from_utf8(&error_buffer[..(-result) as usize]).unwrap();
        assert!(error_str.contains("error"));
        assert!(error_str.contains("parse logic JSON"));
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

        assert!(result < 0);

        let error_buffer = ERROR_BUFFER.lock().unwrap();
        let error_str = std::str::from_utf8(&error_buffer[..(-result) as usize]).unwrap();
        assert!(error_str.contains("error"));
        assert!(error_str.contains("parse data JSON"));
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

        assert!(result < 0);

        let error_buffer = ERROR_BUFFER.lock().unwrap();
        let error_str = std::str::from_utf8(&error_buffer[..(-result) as usize]).unwrap();
        assert!(error_str.contains("Logic pointer is null"));
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

        assert!(result < 0);

        let error_buffer = ERROR_BUFFER.lock().unwrap();
        let error_str = std::str::from_utf8(&error_buffer[..(-result) as usize]).unwrap();
        assert!(error_str.contains("Data pointer is null"));
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

        assert!(result < 0);

        let error_buffer = ERROR_BUFFER.lock().unwrap();
        let error_str = std::str::from_utf8(&error_buffer[..(-result) as usize]).unwrap();
        assert!(error_str.contains("Logic length is negative"));
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

        assert!(result < 0);

        let error_buffer = ERROR_BUFFER.lock().unwrap();
        let error_str = std::str::from_utf8(&error_buffer[..(-result) as usize]).unwrap();
        assert!(error_str.contains("error"));
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
}
