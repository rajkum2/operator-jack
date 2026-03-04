import ApplicationServices
import AppKit
import Foundation

/// Handles `ui.keyPress` — simulates a single key press with optional modifiers.
///
/// Params:
///   - `app` (string, required): Application name.
///   - `key` (string, required): Key name (e.g. "Return", "Tab", "a", "F1").
///   - `modifiers` (string[], optional): Modifier keys (e.g. ["command", "shift"]).
///
/// Returns:
///   - `sent` (bool): Whether the key was sent.
///   - `key` (string): The key that was pressed.
///   - `modifiers` (array): Modifiers applied.
func handleKeyPress(_ params: [String: JSONValue]) throws -> [String: JSONValue] {
    try ensureAccessibility()
    let app = try extractApp(from: params)

    guard case .string(let keyName) = params["key"] else {
        throw HelperError(code: "INVALID_PARAMS", message: "Missing required 'key' parameter")
    }

    // Parse modifiers
    var modifiers: [String] = []
    if case .array(let mods) = params["modifiers"] {
        for mod in mods {
            if case .string(let m) = mod {
                modifiers.append(m)
            }
        }
    }

    // Bring app to foreground
    app.activate(options: .activateIgnoringOtherApps)
    Thread.sleep(forTimeInterval: 0.1)

    // Resolve key code
    guard let keyCode = virtualKeyCode(for: keyName) else {
        throw HelperError(code: "INVALID_PARAMS", message: "Unknown key: '\(keyName)'")
    }

    // Resolve modifier flags
    var flags = CGEventFlags()
    for mod in modifiers {
        switch mod.lowercased() {
        case "command", "cmd":
            flags.insert(.maskCommand)
        case "shift":
            flags.insert(.maskShift)
        case "option", "alt":
            flags.insert(.maskAlternate)
        case "control", "ctrl":
            flags.insert(.maskControl)
        case "fn", "function":
            flags.insert(.maskSecondaryFn)
        default:
            throw HelperError(code: "INVALID_PARAMS", message: "Unknown modifier: '\(mod)'")
        }
    }

    // Send the key event
    guard let keyDown = CGEvent(keyboardEventSource: nil, virtualKey: keyCode, keyDown: true) else {
        throw HelperError(code: "INPUT_BLOCKED", message: "Failed to create CGEvent — secure input may be active")
    }
    keyDown.flags = flags
    keyDown.post(tap: .cghidEventTap)

    guard let keyUp = CGEvent(keyboardEventSource: nil, virtualKey: keyCode, keyDown: false) else {
        throw HelperError(code: "INPUT_BLOCKED", message: "Failed to create CGEvent for key up")
    }
    keyUp.flags = flags
    keyUp.post(tap: .cghidEventTap)

    let modValues: [JSONValue] = modifiers.map { .string($0) }

    return [
        "sent": .bool(true),
        "key": .string(keyName),
        "modifiers": .array(modValues),
        "_evidence": .object(gatherEvidence(app: app)),
    ]
}

// MARK: - Virtual Key Code Lookup

/// Maps key names to macOS virtual key codes.
private func virtualKeyCode(for name: String) -> CGKeyCode? {
    switch name.lowercased() {
    // Special keys
    case "return", "enter":       return 36
    case "tab":                   return 48
    case "space":                 return 49
    case "delete", "backspace":   return 51
    case "escape", "esc":         return 53
    case "forwarddelete":         return 117

    // Arrow keys
    case "leftarrow", "left":     return 123
    case "rightarrow", "right":   return 124
    case "downarrow", "down":     return 125
    case "uparrow", "up":         return 126

    // Navigation
    case "home":                  return 115
    case "end":                   return 119
    case "pageup":                return 116
    case "pagedown":              return 121

    // Function keys
    case "f1":  return 122
    case "f2":  return 120
    case "f3":  return 99
    case "f4":  return 118
    case "f5":  return 96
    case "f6":  return 97
    case "f7":  return 98
    case "f8":  return 100
    case "f9":  return 101
    case "f10": return 109
    case "f11": return 103
    case "f12": return 111

    // Letters
    case "a": return 0
    case "b": return 11
    case "c": return 8
    case "d": return 2
    case "e": return 14
    case "f": return 3
    case "g": return 5
    case "h": return 4
    case "i": return 34
    case "j": return 38
    case "k": return 40
    case "l": return 37
    case "m": return 46
    case "n": return 45
    case "o": return 31
    case "p": return 35
    case "q": return 12
    case "r": return 15
    case "s": return 1
    case "t": return 17
    case "u": return 32
    case "v": return 9
    case "w": return 13
    case "x": return 7
    case "y": return 16
    case "z": return 6

    // Digits
    case "0": return 29
    case "1": return 18
    case "2": return 19
    case "3": return 20
    case "4": return 21
    case "5": return 23
    case "6": return 22
    case "7": return 26
    case "8": return 28
    case "9": return 25

    // Punctuation
    case "-", "minus":            return 27
    case "=", "equal":            return 24
    case "[", "leftbracket":      return 33
    case "]", "rightbracket":     return 30
    case "\\", "backslash":       return 42
    case ";", "semicolon":        return 41
    case "'", "quote":            return 39
    case ",", "comma":            return 43
    case ".", "period":           return 47
    case "/", "slash":            return 44
    case "`", "grave":            return 50

    default: return nil
    }
}
