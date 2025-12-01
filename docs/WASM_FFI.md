# WASM FFI Interface

This document describes the C-style FFI (Foreign Function Interface) for the json-logic-rs WASM module. This interface allows the JsonLogic library to be used from multiple languages (Java/Chicory, JavaScript, Python, Go, .NET) through a single WASM module.

## Overview

The FFI interface replaces the previous `wasm-bindgen` based interface with a C-compatible interface that works with any WASM runtime. This enables cross-platform usage without JavaScript-specific bindings.

## Memory Model

The module uses static mutable buffers for result and error storage:

```
┌─────────────────────────────────────────────────────────────────┐
│                         WASM Memory                             │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │              Result Buffer (1MB)                         │   │
│  │  get_result_ptr() → returns pointer to this buffer       │   │
│  │  Stores successful JSONLogic evaluation results          │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │              Error Buffer (1MB)                          │   │
│  │  get_error_ptr() → returns pointer to this buffer        │   │
│  │  Stores error messages as JSON: {"error": "message"}     │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │              Input Memory (caller allocated)             │   │
│  │  Logic JSON string                                       │   │
│  │  Data JSON string                                        │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

## API Reference

### `apply_json_logic`

```c
int32_t apply_json_logic(
    const uint8_t* logic_ptr,
    int32_t logic_len,
    const uint8_t* data_ptr,
    int32_t data_len
);
```

Apply JSONLogic rules to data.

**Parameters:**
- `logic_ptr`: Pointer to the JSON logic string (UTF-8 encoded)
- `logic_len`: Length of the JSON logic string in bytes
- `data_ptr`: Pointer to the JSON data string (UTF-8 encoded)
- `data_len`: Length of the JSON data string in bytes

**Returns:**
- Positive value: Success - the length of the result stored in the result buffer
- Negative value: Error - the absolute value is the length of the error stored in the error buffer

### `get_result_ptr`

```c
const uint8_t* get_result_ptr();
```

Returns a pointer to the result buffer. After a successful call to `apply_json_logic`, use this pointer to read the result.

### `get_error_ptr`

```c
const uint8_t* get_error_ptr();
```

Returns a pointer to the error buffer. After a failed call to `apply_json_logic`, use this pointer to read the error message.

### `get_buffer_size`

```c
int32_t get_buffer_size();
```

Returns the maximum buffer size (1MB = 1,048,576 bytes). Use this to detect if results might be truncated.

### `clear_buffers`

```c
void clear_buffers();
```

Clears both result and error buffers (sets all bytes to 0). Call this before making a new `apply_json_logic` call if you want to ensure clean buffers.

## Usage Pattern

```text
1. Allocate memory in WASM for logic JSON string
2. Write logic JSON to that memory
3. Allocate memory for data JSON string
4. Write data JSON to that memory
5. Call apply_json_logic(logic_ptr, logic_len, data_ptr, data_len)
6. If result >= 0: call get_result_ptr() and read result_len bytes
7. If result < 0: call get_error_ptr() and read abs(result) bytes
```

## Example Usage

### JavaScript (using WebAssembly API)

```javascript
async function applyJsonLogic(wasmInstance, logic, data) {
    const encoder = new TextEncoder();
    const decoder = new TextDecoder();
    
    // Encode inputs as UTF-8
    const logicBytes = encoder.encode(logic);
    const dataBytes = encoder.encode(data);
    
    // Allocate memory in WASM
    const memory = wasmInstance.exports.memory;
    const logicPtr = /* allocate logicBytes.length bytes */;
    const dataPtr = /* allocate dataBytes.length bytes */;
    
    // Copy data to WASM memory
    new Uint8Array(memory.buffer, logicPtr).set(logicBytes);
    new Uint8Array(memory.buffer, dataPtr).set(dataBytes);
    
    // Call the function
    const result = wasmInstance.exports.apply_json_logic(
        logicPtr, logicBytes.length,
        dataPtr, dataBytes.length
    );
    
    if (result >= 0) {
        // Success - read result
        const resultPtr = wasmInstance.exports.get_result_ptr();
        const resultBytes = new Uint8Array(memory.buffer, resultPtr, result);
        return { success: true, data: decoder.decode(resultBytes) };
    } else {
        // Error - read error message
        const errorPtr = wasmInstance.exports.get_error_ptr();
        const errorBytes = new Uint8Array(memory.buffer, errorPtr, -result);
        return { success: false, error: decoder.decode(errorBytes) };
    }
}
```

### Java with Chicory

```java
import com.dylibso.chicory.runtime.*;

public class JsonLogicRunner {
    private final Instance instance;
    
    public String applyJsonLogic(String logic, String data) {
        byte[] logicBytes = logic.getBytes(StandardCharsets.UTF_8);
        byte[] dataBytes = data.getBytes(StandardCharsets.UTF_8);
        
        // Allocate and write to WASM memory
        int logicPtr = allocateAndWrite(logicBytes);
        int dataPtr = allocateAndWrite(dataBytes);
        
        // Call the function
        int result = instance.export("apply_json_logic")
            .apply(logicPtr, logicBytes.length, dataPtr, dataBytes.length);
        
        if (result >= 0) {
            int resultPtr = instance.export("get_result_ptr").apply();
            return readString(resultPtr, result);
        } else {
            int errorPtr = instance.export("get_error_ptr").apply();
            throw new RuntimeException(readString(errorPtr, -result));
        }
    }
}
```

### Go with wazero

```go
import "github.com/tetratelabs/wazero"

func applyJsonLogic(ctx context.Context, mod api.Module, logic, data string) (string, error) {
    logicBytes := []byte(logic)
    dataBytes := []byte(data)
    
    // Allocate and write to WASM memory
    logicPtr := allocateAndWrite(mod, logicBytes)
    dataPtr := allocateAndWrite(mod, dataBytes)
    
    // Call the function
    results, _ := mod.ExportedFunction("apply_json_logic").Call(ctx,
        uint64(logicPtr), uint64(len(logicBytes)),
        uint64(dataPtr), uint64(len(dataBytes)),
    )
    
    result := int32(results[0])
    if result >= 0 {
        resultPtr, _ := mod.ExportedFunction("get_result_ptr").Call(ctx)
        return readString(mod, uint32(resultPtr[0]), uint32(result)), nil
    } else {
        errorPtr, _ := mod.ExportedFunction("get_error_ptr").Call(ctx)
        return "", errors.New(readString(mod, uint32(errorPtr[0]), uint32(-result)))
    }
}
```

### .NET

```csharp
using Wasmtime;

public class JsonLogicRunner
{
    private readonly Instance _instance;
    
    public string ApplyJsonLogic(string logic, string data)
    {
        byte[] logicBytes = Encoding.UTF8.GetBytes(logic);
        byte[] dataBytes = Encoding.UTF8.GetBytes(data);
        
        // Allocate and write to WASM memory
        int logicPtr = AllocateAndWrite(logicBytes);
        int dataPtr = AllocateAndWrite(dataBytes);
        
        // Call the function
        var applyFn = _instance.GetFunction<int, int, int, int, int>("apply_json_logic");
        int result = applyFn(logicPtr, logicBytes.Length, dataPtr, dataBytes.Length);
        
        if (result >= 0)
        {
            var getResultPtr = _instance.GetFunction<int>("get_result_ptr");
            int resultPtr = getResultPtr();
            return ReadString(resultPtr, result);
        }
        else
        {
            var getErrorPtr = _instance.GetFunction<int>("get_error_ptr");
            int errorPtr = getErrorPtr();
            throw new Exception(ReadString(errorPtr, -result));
        }
    }
}
```

### Python with wasmer

```python
from wasmer import Store, Module, Instance

def apply_json_logic(instance, logic: str, data: str) -> str:
    logic_bytes = logic.encode('utf-8')
    data_bytes = data.encode('utf-8')
    
    # Allocate and write to WASM memory
    logic_ptr = allocate_and_write(instance, logic_bytes)
    data_ptr = allocate_and_write(instance, data_bytes)
    
    # Call the function
    result = instance.exports.apply_json_logic(
        logic_ptr, len(logic_bytes),
        data_ptr, len(data_bytes)
    )
    
    if result >= 0:
        result_ptr = instance.exports.get_result_ptr()
        return read_string(instance, result_ptr, result)
    else:
        error_ptr = instance.exports.get_error_ptr()
        raise Exception(read_string(instance, error_ptr, -result))
```

## Build Instructions

### For JavaScript, Java/Chicory (no system dependencies)

```bash
# Build for wasm32-unknown-unknown target
cargo build --target wasm32-unknown-unknown --release

# The output will be in:
# target/wasm32-unknown-unknown/release/jsonlogic_rs.wasm
```

### For Go, .NET, Python (with WASI support)

```bash
# Build for wasm32-wasip1 target
cargo build --target wasm32-wasip1 --release

# The output will be in:
# target/wasm32-wasip1/release/jsonlogic_rs.wasm
```

### Adding WASM targets (if not already installed)

```bash
rustup target add wasm32-unknown-unknown
rustup target add wasm32-wasip1
```

## Error Handling

All errors are returned as JSON strings in the format:

```json
{"error": "error message here"}
```

### Error Types

1. **Input Validation Errors**
   - `"Logic pointer is null"`
   - `"Data pointer is null"`
   - `"Logic length is negative"`
   - `"Data length is negative"`

2. **UTF-8 Errors**
   - `"Invalid UTF-8 in logic: ..."`
   - `"Invalid UTF-8 in data: ..."`

3. **JSON Parsing Errors**
   - `"Failed to parse logic JSON: ..."`
   - `"Failed to parse data JSON: ..."`

4. **JSONLogic Evaluation Errors**
   - `"JSONLogic evaluation error: ..."`

5. **Serialization Errors**
   - `"Failed to serialize result: ..."`

## Buffer Size Limitations

- Both result and error buffers are 1MB (1,048,576 bytes)
- If a result exceeds the buffer size, it will be truncated
- Use `get_buffer_size()` to check the maximum size
- For very large results, consider splitting the operation or processing in chunks

## Thread Safety

The implementation uses `std::sync::Mutex` for thread safety of the static buffers. While WASM is typically single-threaded, this ensures safe operation in all environments.

## Migration from wasm-bindgen

If you were using the previous `wasm-bindgen` based interface:

**Before (wasm-bindgen):**
```javascript
import init, { apply } from 'jsonlogic-rs';

await init();
const result = apply(logic, data);
```

**After (C FFI):**
```javascript
const wasmInstance = await WebAssembly.instantiate(wasmModule);
const result = applyJsonLogic(wasmInstance, JSON.stringify(logic), JSON.stringify(data));
```

The main differences:
1. Input must be JSON strings (not JavaScript objects)
2. Manual memory management required
3. Works with any WASM runtime, not just JavaScript
