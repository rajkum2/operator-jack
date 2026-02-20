use std::io::{BufRead, Write};

use crate::IpcError;

/// Maximum line size for NDJSON messages (1 MiB).
const MAX_LINE_BYTES: usize = 1_048_576;

/// Serializes `value` as a single JSON line followed by `\n`, then flushes.
/// Returns `IpcError::InvalidResponse` if the serialized line exceeds 1 MiB.
pub fn write_ndjson_line<W: Write>(
    writer: &mut W,
    value: &impl serde::Serialize,
) -> Result<(), IpcError> {
    let json = serde_json::to_string(value)?;
    if json.len() > MAX_LINE_BYTES {
        return Err(IpcError::InvalidResponse(format!(
            "Serialized message exceeds {} byte limit ({} bytes)",
            MAX_LINE_BYTES,
            json.len()
        )));
    }
    writer.write_all(json.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

/// Reads one line from `reader`, deserializes it as JSON type `T`.
/// Returns `IpcError::HelperCrashed` on EOF, `IpcError::InvalidResponse` if
/// the line exceeds 1 MiB.
pub fn read_ndjson_line<R: BufRead, T: serde::de::DeserializeOwned>(
    reader: &mut R,
) -> Result<T, IpcError> {
    let mut line = String::new();
    let bytes_read = reader.read_line(&mut line)?;
    if bytes_read == 0 {
        return Err(IpcError::HelperCrashed(
            "EOF on helper stdout (helper process may have exited)".into(),
        ));
    }
    if line.len() > MAX_LINE_BYTES {
        return Err(IpcError::InvalidResponse(format!(
            "Response line exceeds {} byte limit ({} bytes)",
            MAX_LINE_BYTES,
            line.len()
        )));
    }
    let value: T = serde_json::from_str(line.trim())?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    #[test]
    fn test_write_and_read_roundtrip() {
        let value = serde_json::json!({"id": "1", "method": "test", "params": {}});

        // Write
        let mut buf: Vec<u8> = Vec::new();
        write_ndjson_line(&mut buf, &value).unwrap();

        // The output should end with a newline
        assert!(buf.ends_with(b"\n"));

        // Read it back
        let mut reader = BufReader::new(buf.as_slice());
        let result: serde_json::Value = read_ndjson_line(&mut reader).unwrap();
        assert_eq!(result["id"], "1");
        assert_eq!(result["method"], "test");
    }

    #[test]
    fn test_read_eof_returns_helper_crashed() {
        let mut reader = BufReader::new("".as_bytes());
        let result: Result<serde_json::Value, IpcError> = read_ndjson_line(&mut reader);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, IpcError::HelperCrashed(_)));
    }

    #[test]
    fn test_write_oversized_line_rejected() {
        // Create a value that serializes to > 1 MiB
        let big_string = "x".repeat(MAX_LINE_BYTES + 1);
        let value = serde_json::json!({"data": big_string});

        let mut buf: Vec<u8> = Vec::new();
        let result = write_ndjson_line(&mut buf, &value);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), IpcError::InvalidResponse(_)));
    }

    #[test]
    fn test_read_oversized_line_rejected() {
        let big_line = format!("{{\"x\":\"{}\"}}\n", "a".repeat(MAX_LINE_BYTES + 1));
        let mut reader = BufReader::new(big_line.as_bytes());
        let result: Result<serde_json::Value, IpcError> = read_ndjson_line(&mut reader);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), IpcError::InvalidResponse(_)));
    }

    #[test]
    fn test_read_invalid_json() {
        let mut reader = BufReader::new("not valid json\n".as_bytes());
        let result: Result<serde_json::Value, IpcError> = read_ndjson_line(&mut reader);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), IpcError::Json(_)));
    }
}
