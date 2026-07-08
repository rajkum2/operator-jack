import XCTest

@testable import operator_macos_helper

/// Unit tests for the selector-parsing and criteria layer. These cover the
/// pure logic that does not require a live AX tree; the AX-dependent behaviors
/// (findElements root matching, wait_for polling) are exercised by the live
/// acceptance harness (`make check-examples`) since they need a GUI session.
final class SelectorParamsTests: XCTestCase {

    private func params(_ selector: [String: JSONValue]) -> [String: JSONValue] {
        ["selector": .object(selector)]
    }

    func testParsesAllScalarFields() {
        let sel = SelectorParams.parse(
            from: params([
                "role": .string("AXButton"),
                "subrole": .string("AXCloseButton"),
                "name": .string("OK"),
                "identifier": .string("okButton"),
                "value": .string("42"),
                "path": .string("AXWindow/AXButton[0]"),
                "index": .int(2),
                "max_depth": .int(5),
            ]))
        XCTAssertNotNil(sel)
        XCTAssertEqual(sel?.role, "AXButton")
        XCTAssertEqual(sel?.subrole, "AXCloseButton")
        XCTAssertEqual(sel?.name, "OK")
        XCTAssertEqual(sel?.identifier, "okButton")
        XCTAssertEqual(sel?.value, "42")
        XCTAssertEqual(sel?.path, "AXWindow/AXButton[0]")
        XCTAssertEqual(sel?.index, 2)
        XCTAssertEqual(sel?.maxDepth, 5)
    }

    func testParsesContainsVariants() {
        let sel = SelectorParams.parse(
            from: params([
                "name_contains": .string("Sav"),
                "description_contains": .string("close"),
                "value_contains": .string("draft"),
            ]))
        XCTAssertEqual(sel?.nameContains, "Sav")
        XCTAssertEqual(sel?.descriptionContains, "close")
        XCTAssertEqual(sel?.valueContains, "draft")
    }

    func testParsesWindowScope() {
        let byIndex = SelectorParams.parse(
            from: params([
                "role": .string("AXButton"),
                "window": .object(["index": .int(1)]),
            ]))
        XCTAssertEqual(byIndex?.window?.index, 1)

        let byTitle = SelectorParams.parse(
            from: params([
                "role": .string("AXButton"),
                "window": .object(["title_contains": .string("Untitled")]),
            ]))
        XCTAssertEqual(byTitle?.window?.titleContains, "Untitled")
    }

    func testParsesNestedAnyOf() {
        let sel = SelectorParams.parse(
            from: params([
                "any_of": .array([
                    .object(["identifier": .string("One")]),
                    .object(["role": .string("AXButton"), "name": .string("1")]),
                ])
            ]))
        XCTAssertNotNil(sel?.anyOf)
        XCTAssertEqual(sel?.anyOf?.count, 2)
        XCTAssertEqual(sel?.anyOf?[0].identifier, "One")
        XCTAssertEqual(sel?.anyOf?[1].name, "1")
    }

    func testMissingSelectorReturnsNil() {
        XCTAssertNil(SelectorParams.parse(from: ["app": .string("Calculator")]))
    }

    // MARK: - hasMatchableCriteria (the match-everything guard)

    func testHasMatchableCriteriaTrueForContentFields() {
        var s = SelectorParams()
        s.role = "AXButton"
        XCTAssertTrue(s.hasMatchableCriteria)

        var byId = SelectorParams()
        byId.identifier = "okButton"
        XCTAssertTrue(byId.hasMatchableCriteria)
    }

    func testHasMatchableCriteriaFalseForEmptySelector() {
        XCTAssertFalse(SelectorParams().hasMatchableCriteria)
    }

    func testHasMatchableCriteriaFalseForPathOnly() {
        // `path` is parsed but not a matcher yet — a path-only selector must not
        // count as matchable (it would otherwise match every element).
        var s = SelectorParams()
        s.path = "AXWindow/AXButton[0]"
        XCTAssertFalse(s.hasMatchableCriteria)
    }

    func testHasMatchableCriteriaFalseForIndexOrWindowOnly() {
        var byIndex = SelectorParams()
        byIndex.index = 0
        XCTAssertFalse(byIndex.hasMatchableCriteria)

        var byWindow = SelectorParams()
        byWindow.window = WindowScopeParams(index: 0, titleContains: nil)
        XCTAssertFalse(byWindow.hasMatchableCriteria)
    }
}
