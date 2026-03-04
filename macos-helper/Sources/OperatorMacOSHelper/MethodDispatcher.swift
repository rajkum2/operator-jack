import Foundation

/// Dispatches IPC method calls to their handler functions.
struct MethodDispatcher {
    /// Handler signature: takes params, returns result dict or throws.
    typealias Handler = ([String: JSONValue]) throws -> [String: JSONValue]

    private var handlers: [String: Handler] = [:]

    mutating func register(_ method: String, handler: @escaping Handler) {
        handlers[method] = handler
    }

    func dispatch(request: IpcRequest) -> IpcResponse {
        guard let handler = handlers[request.method] else {
            return IpcResponse.failure(
                id: request.id,
                code: "METHOD_NOT_FOUND",
                message: "Unknown method: \(request.method)"
            )
        }

        do {
            let result = try handler(request.params)
            return IpcResponse.success(id: request.id, result: result)
        } catch let error as HelperError {
            return IpcResponse.failure(
                id: request.id,
                code: error.code,
                message: error.message,
                retryable: error.retryable,
                details: error.details
            )
        } catch {
            return IpcResponse.failure(
                id: request.id,
                code: "INTERNAL_ERROR",
                message: error.localizedDescription
            )
        }
    }
}

/// A structured error that helper methods can throw.
struct HelperError: Error {
    let code: String
    let message: String
    let retryable: Bool
    let details: [String: JSONValue]

    init(code: String, message: String, retryable: Bool = false, details: [String: JSONValue] = [:]) {
        self.code = code
        self.message = message
        self.retryable = retryable
        self.details = details
    }
}
