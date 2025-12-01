# WASM FFI Interface

This document describes the C-style FFI (Foreign Function Interface) for the json-logic-rs WASM module. This interface allows the JsonLogic library to be used from multiple languages (Java/Chicory, JavaScript, Python, Go, .NET) through a single WASM module.


## Overview

The FFI interface provides a simple C-compatible interface that works with any WASM runtime. All outputs (including errors) are returned as valid JSON, making the API easy to use.

The API uses a **packed pointer** approach: `apply_json_logic` returns a single 64-bit value containing both the output pointer and length, so you only need to call one function.

## Memory Model

The module uses a single static buffer for output storage:

```
┌─────────────────────────────────────────────────────────────────┐
│                         WASM Memory                             │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │              Output Buffer (1MB)                         │   │
│  │  Stores all output as valid JSON:                        │   │
│  │  - Results: the JSON evaluation result                   │   │
│  │  - Errors: {"error": "message"}                          │   │
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
int64_t apply_json_logic(
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
- A packed 64-bit value where:
  - High 32 bits: pointer to the output buffer
  - Low 32 bits: length of the output in bytes
- The output is always valid JSON stored in the output buffer
- On success: the JSON result of evaluation
- On error: `{"error": "error message"}`

**Unpacking the result:**
```
pointer = (result >> 32) & 0xFFFFFFFF
length = result & 0xFFFFFFFF
```

### `get_buffer_size`

```c
int32_t get_buffer_size();
```

Returns the maximum buffer size (1MB = 1,048,576 bytes). Use this to detect if results might be truncated.

## Usage Pattern

```text
1. Write logic JSON string to WASM memory
2. Write data JSON string to WASM memory  
3. Call apply_json_logic(logic_ptr, logic_len, data_ptr, data_len)
4. Unpack the result: pointer = result >> 32, length = result & 0xFFFFFFFF
5. Read `length` bytes from `pointer`
6. Parse the JSON - it's either the result or {"error": "message"}
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
    
    // Call the function - returns packed pointer
    const result = wasmInstance.exports.apply_json_logic(
        logicPtr, logicBytes.length,
        dataPtr, dataBytes.length
    );
    
    // Unpack the result (64-bit BigInt in JS)
    const ptr = Number(result >> 32n);
    const len = Number(result & 0xFFFFFFFFn);
    
    // Read the output
    const outputBytes = new Uint8Array(memory.buffer, ptr, len);
    const output = JSON.parse(decoder.decode(outputBytes));
    
    // Check if it's an error
    if (output.error) {
        throw new Error(output.error);
    }
    return output;
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
        
        // Call the function - returns packed pointer
        long result = instance.export("apply_json_logic")
            .apply(logicPtr, logicBytes.length, dataPtr, dataBytes.length);
        
        // Unpack the result
        int ptr = (int)(result >> 32);
        int len = (int)(result & 0xFFFFFFFF);
        
        // Read the output
        String output = readString(ptr, len);
        
        // Parse and check for errors
        JsonObject json = JsonParser.parseString(output).getAsJsonObject();
        if (json.has("error")) {
            throw new RuntimeException(json.get("error").getAsString());
        }
        return output;
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
    
    // Call the function - returns packed pointer
    results, _ := mod.ExportedFunction("apply_json_logic").Call(ctx,
        uint64(logicPtr), uint64(len(logicBytes)),
        uint64(dataPtr), uint64(len(dataBytes)),
    )
    
    result := results[0]
    
    // Unpack the result
    ptr := uint32(result >> 32)
    length := uint32(result & 0xFFFFFFFF)
    
    // Read the output
    output := readString(mod, ptr, length)
    
    // Parse and check for errors
    var parsed map[string]interface{}
    json.Unmarshal([]byte(output), &parsed)
    if errMsg, ok := parsed["error"].(string); ok {
        return "", errors.New(errMsg)
    }
    return output, nil
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
        
        // Call the function - returns packed pointer
        var applyFn = _instance.GetFunction<int, int, int, int, long>("apply_json_logic");
        long result = applyFn(logicPtr, logicBytes.Length, dataPtr, dataBytes.Length);
        
        // Unpack the result
        int ptr = (int)(result >> 32);
        int len = (int)(result & 0xFFFFFFFF);
        
        // Read the output
        string output = ReadString(ptr, len);
        
        // Parse and check for errors
        var json = JsonSerializer.Deserialize<JsonElement>(output);
        if (json.TryGetProperty("error", out var error))
        {
            throw new Exception(error.GetString());
        }
        return output;
    }
}
```

### Python with wasmer

```python
from wasmer import Store, Module, Instance
import json

def apply_json_logic(instance, logic: str, data: str) -> str:
    logic_bytes = logic.encode('utf-8')
    data_bytes = data.encode('utf-8')
    
    # Allocate and write to WASM memory
    logic_ptr = allocate_and_write(instance, logic_bytes)
    data_ptr = allocate_and_write(instance, data_bytes)
    
    # Call the function - returns packed pointer
    result = instance.exports.apply_json_logic(
        logic_ptr, len(logic_bytes),
        data_ptr, len(data_bytes)
    )
    
    # Unpack the result
    ptr = (result >> 32) & 0xFFFFFFFF
    length = result & 0xFFFFFFFF
    
    # Read the output
    output = read_string(instance, ptr, length)
    
    # Parse and check for errors
    parsed = json.loads(output)
    if isinstance(parsed, dict) and 'error' in parsed:
        raise Exception(parsed['error'])
    return parsed
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

All outputs are valid JSON. Errors are returned as JSON objects:

```json
{"error": "error message here"}
```

### Error Types

1. **Input Validation Errors**
   - `"logic pointer is null"`
   - `"data pointer is null"`
   - `"logic length is negative"`
   - `"data length is negative"`

2. **UTF-8 Errors**
   - `"invalid UTF-8 in logic: ..."`
   - `"invalid UTF-8 in data: ..."`

3. **JSON Parsing Errors**
   - `"failed to parse logic JSON: ..."`
   - `"failed to parse data JSON: ..."`

4. **JSONLogic Evaluation Errors**
   - `"evaluation error: ..."`

5. **Serialization Errors**
   - `"failed to serialize result: ..."`

## Buffer Size Limitations

- The output buffer is 1MB (1,048,576 bytes)
- If a result exceeds the buffer size, it will be truncated
- Use `get_buffer_size()` to check the maximum size
- For very large results, consider splitting the operation or processing in chunks

## Thread Safety

The implementation uses `std::sync::Mutex` for thread safety of the static buffer. While WASM is typically single-threaded, this ensures safe operation in all environments.

## Migration from wasm-bindgen

If you were using the previous `wasm-bindgen` based interface:

**Before (wasm-bindgen):**
```javascript
import init, { apply } from 'jsonlogic-rs';

await init();
const result = apply(logic, data);
```

**After (C FFI with packed pointer):**
```javascript
const wasmInstance = await WebAssembly.instantiate(wasmModule);
const result = applyJsonLogic(wasmInstance, JSON.stringify(logic), JSON.stringify(data));
```

The main differences:
1. Input must be JSON strings (not JavaScript objects)
2. Single function call returns packed pointer with output location
3. All outputs (including errors) are valid JSON
4. Works with any WASM runtime, not just JavaScript
