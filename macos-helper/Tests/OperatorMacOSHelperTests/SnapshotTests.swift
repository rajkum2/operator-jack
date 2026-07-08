import XCTest

@testable import operator_macos_helper

/// Tests for the pure snapshot helpers. The AX-dependent traversal (interactive
/// detection, ref minting, tree filtering) is exercised by the live acceptance
/// harness since it requires a running app and Accessibility permission.
final class SnapshotTests: XCTestCase {

    func testShortRoleStripsAXPrefixAndLowercases() {
        XCTAssertEqual(shortRole("AXButton"), "button")
        XCTAssertEqual(shortRole("AXStaticText"), "statictext")
        XCTAssertEqual(shortRole("AXWebArea"), "webarea")
        // No AX prefix: just lowercased.
        XCTAssertEqual(shortRole("Group"), "group")
        // Defensive: empty / unusual input does not crash.
        XCTAssertEqual(shortRole("AX"), "")
    }
}
