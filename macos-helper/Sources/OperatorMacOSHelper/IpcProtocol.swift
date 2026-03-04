import Foundation

// MARK: - JSON Value

/// A type-erased JSON value for arbitrary params/results.
enum JSONValue: Codable, Equatable {
    case null
    case bool(Bool)
    case int(Int)
    case double(Double)
    case string(String)
    case array([JSONValue])
    case object([String: JSONValue])

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self = .null
        } else if let b = try? container.decode(Bool.self) {
            self = .bool(b)
        } else if let i = try? container.decode(Int.self) {
            self = .int(i)
        } else if let d = try? container.decode(Double.self) {
            self = .double(d)
        } else if let s = try? container.decode(String.self) {
            self = .string(s)
        } else if let arr = try? container.decode([JSONValue].self) {
            self = .array(arr)
        } else if let obj = try? container.decode([String: JSONValue].self) {
            self = .object(obj)
        } else {
            throw DecodingError.typeMismatch(
                JSONValue.self,
                DecodingError.Context(codingPath: decoder.codingPath, debugDescription: "Unsupported JSON type")
            )
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .null:
            try container.encodeNil()
        case .bool(let b):
            try container.encode(b)
        case .int(let i):
            try container.encode(i)
        case .double(let d):
            try container.encode(d)
        case .string(let s):
            try container.encode(s)
        case .array(let arr):
            try container.encode(arr)
        case .object(let obj):
            try container.encode(obj)
        }
    }
}

// MARK: - IPC Request

struct IpcRequest: Codable {
    let id: String
    let method: String
    let params: [String: JSONValue]
}

// MARK: - IPC Response

struct IpcResponse: Codable {
    let id: String
    let ok: Bool
    let result: [String: JSONValue]?
    let error: IpcErrorPayload?

    static func success(id: String, result: [String: JSONValue]) -> IpcResponse {
        IpcResponse(id: id, ok: true, result: result, error: nil)
    }

    static func failure(id: String, code: String, message: String, retryable: Bool = false, details: [String: JSONValue] = [:]) -> IpcResponse {
        IpcResponse(
            id: id,
            ok: false,
            result: nil,
            error: IpcErrorPayload(code: code, message: message, retryable: retryable, details: details)
        )
    }
}

// MARK: - IPC Error Payload

struct IpcErrorPayload: Codable {
    let code: String
    let message: String
    let retryable: Bool
    let details: [String: JSONValue]

    init(code: String, message: String, retryable: Bool, details: [String: JSONValue] = [:]) {
        self.code = code
        self.message = message
        self.retryable = retryable
        self.details = details
    }
}
