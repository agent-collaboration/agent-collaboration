import Foundation

// Native field and fixed-selector-column matching controls (moved unchanged
// from verify_screen_identity_controls.swift when it crossed 900 lines).
extension VerifyScreenImage {
  static func checkNativeFieldMatchingControls() -> (count: Int, failures: [String]) {
    func region(_ text: String, _ x: Double, _ y: Double, _ width: Double = 0.2) -> ScreenTextRegion {
      ScreenTextRegion(text: text, bounds: CGRect(x: x, y: y, width: width, height: 0.04))
    }
    let base = [region("Weekly floor", 0.08, 0.8, 0.3), region("[alpha-tag]", 0.08, 0.73, 0.5),
      region("Floor", 0.08, 0.64, 0.11), region("1%", 0.4, 0.64, 0.07),
      region("Cancel", 0.08, 0.2), region("Save", 0.35, 0.2),
      region("0% disables protection maximum 15%", 0.08, 0.5, 0.8)]
    func changed(_ index: Int, _ value: ScreenTextRegion) -> [ScreenTextRegion] {
      var result = base; result[index] = value; return result
    }
    let cases: [(String, [ScreenTextRegion], Bool, Bool)] = [
      ("floor-active-one", base, true, true),
      ("floor-recognition-order", Array(base.reversed()), true, true),
      ("floor-width-brackets", changed(1, region("［alpha-tag］", 0.08, 0.73, 0.5)), true, true),
      ("floor-zero", changed(3, region("0%", 0.4, 0.64, 0.07)), true, false),
      ("floor-two", changed(3, region("2%", 0.4, 0.64, 0.07)), true, false),
      ("floor-guidance-fifteen", changed(3, region("15%", 0.4, 0.64, 0.07)), true, false),
      ("floor-arrow-is-not-value", changed(3, region("4", 0.4, 0.64, 0.07)), true, false),
      ("floor-background-one", changed(3, region("1%", 0.4, 0.9, 0.07)), true, false),
      ("floor-other-row", changed(3, region("1%", 0.4, 0.5, 0.07)), true, false),
      ("floor-duplicate-value", base + [base[3]], true, false),
      ("floor-nearer-wrong", base + [region("0%", 0.3, 0.64, 0.07)], true, false),
      ("floor-wrong-title", changed(0, region("Monthly floor", 0.08, 0.8, 0.3)), true, false),
      ("floor-wrong-tag", changed(1, region("[beta-tag]", 0.08, 0.73, 0.5)), true, false),
      ("floor-duplicate-field", base + [base[2]], true, false),
      ("floor-no-yellow", base, false, false),
      ("floor-invalid-bounds", changed(3, region("1%", .nan, 0.64, 0.07)), true, false),
      ("floor-missing-action", base.enumerated().filter { $0.offset != 5 }.map(\.element), true, false),
      ("floor-guidance-only", base.enumerated().filter { $0.offset != 3 }.map(\.element)
        + [region("1%", 0.4, 0.5, 0.07)], true, false),
    ]
    var failures: [String] = []
    for (name, rows, yellow, expected) in cases {
      if (selectedFloorDraftValue(rows, selectedText: { _ in yellow }) == "1%") != expected {
        failures.append(name)
      }
    }
    let header = region("Account", 0.2, 0.8, 0.14)
    let first = region("Claude", 0.2, 0.7, 0.12)
    let continuation = region("Account", 0.21, 0.6, 0.14)
    let firstColumn = accountSelectorBounds(header: header, row: first)
    let continuationColumn = accountSelectorBounds(header: header, row: continuation)
    if firstColumn?.minX != continuationColumn?.minX || firstColumn?.maxX != continuationColumn?.maxX
      || abs((firstColumn?.minX ?? 0) - 0.16) > 0.0000001
      || abs((firstColumn?.maxX ?? 0) - 0.2) > 0.0000001 { failures.append("fixed-selector-column") }
    for (name, invalidHeader, row) in [
      ("selector-wrong-header", region("Status", 0.2, 0.8), first),
      ("selector-negative-column", region("Account", 0.01, 0.8, 0.14), first),
      ("selector-over-header", header, region("Claude", 0.2, 0.9)),
      ("selector-invalid-row", header, region("Claude", .nan, 0.7)),
    ] {
      if accountSelectorBounds(header: invalidHeader, row: row) != nil { failures.append(name) }
    }
    let creditChoices = checkCreditChoiceFocusControls()
    failures.append(contentsOf: creditChoices.failures)
    let accountGutters = checkAccountGutterPixelControls()
    failures.append(contentsOf: accountGutters.failures)
    let splitQueries = checkSplitQueryFieldControls()
    failures.append(contentsOf: splitQueries.failures)
    return (cases.count + 5 + creditChoices.count + accountGutters.count + splitQueries.count, failures)
  }

  static func checkSplitQueryFieldControls() -> (count: Int, failures: [String]) {
    func box(_ text: String, _ x: Double, _ y: Double, _ w: Double, _ h: Double) -> ScreenTextRegion {
      ScreenTextRegion(text: text, bounds: CGRect(x: x, y: y, width: w, height: h))
    }
    // Native 32x24 Credit usage: the Search label and exact query are on
    // adjacent painted rows; No matching choices directly follows the value.
    let heading = box("Credit usage", 0.0963663121, 0.6643696556, 0.3728882272, 0.0413708293)
    let field = box("Use credits", 0.1028571456, 0.5867768597, 0.3314285714, 0.0330578512)
    let search = box("Search:", 0.1540054518, 0.5404471029, 0.2177033779, 0.0389405046)
    let value = box("zznomatchzz", 0.1600000028, 0.5000000004, 0.3485714286, 0.0413223140)
    let noMatch = box("No matching choices", 0.1600000073, 0.4586776861, 0.5828571429, 0.0371900826)
    let actions = [box("Cancel", 0.1026395728, 0.2554611882, 0.1832922799, 0.0303999373),
      box("Select", 0.3371428588, 0.2520661159, 0.1942857143, 0.0371900826)]
    let base = [heading, field, search, value, noMatch] + actions
    func changingValue(_ text: String, _ x: Double = 0.1600000028, _ y: Double = 0.5000000004) -> [ScreenTextRegion] {
      [heading, field, search, box(text, x, y, value.bounds.width, value.bounds.height), noMatch] + actions
    }
    let cases: [(String, [ScreenTextRegion], Bool)] = [
      ("query-native-stacked-value", base, true),
      ("query-native-stacked-caret", changingValue("zznomatchzz▏"), true),
      ("query-stacked-wrong-value", changingValue("another"), false),
      ("query-stacked-extra-content", changingValue("zznomatchzz extra"), false),
      ("query-stacked-other-column", changingValue("zznomatchzz", 0.5), false),
      ("query-stacked-distant-row", changingValue("zznomatchzz", 0.16, 0.35), false),
      ("query-stacked-duplicate-value", base + [value], false),
      ("query-stacked-intervening-field", base + [box("Other field", 0.16, 0.52, 0.3, 0.015)], false),
      ("query-stacked-missing-value", base.filter { $0.text != "zznomatchzz" }, false),
    ]
    var failures: [String] = []
    for (name, regions, expected) in cases {
      let pairs = sessionsChoiceQueryPairs(regions, title: "credit usage", field: "use credits")
      if pairs.contains("field use credits query zznomatchzz") != expected { failures.append(name) }
    }
    return (cases.count, failures)
  }

  static func checkAccountGutterPixelControls() -> (count: Int, failures: [String]) {
    // The Account heading's native left edge can include one column of a
    // continuation label's left bearing. That column is not a selector glyph.
    var bytes = [UInt8](repeating: 0, count: 100 * 100 * 4)
    func paint(_ xs: Range<Int>, _ ys: Range<Int>) {
      for y in ys { for x in xs {
        let offset = y * 400 + x * 4
        bytes[offset] = 220; bytes[offset + 1] = 220; bytes[offset + 2] = 20
      } }
    }
    paint(16..<20, 29..<40) // Actual selector footprint, four columns.
    paint(23..<40, 29..<39) // First selected-name row.
    paint(19..<35, 42..<51) // Continuation's left bearing enters the gutter by one column.
    let pixels = OriginalChoicePixels(controlWidth: 100, controlHeight: 100, controlBytes: bytes)
    let header = ScreenTextRegion(text: "Account", bounds: CGRect(x: 0.2, y: 0.85, width: 0.14, height: 0.04))
    let cases: [(String, ScreenTextRegion, Bool)] = [
      ("account-selector-real-footprint", ScreenTextRegion(text: "Claude", bounds: CGRect(x: 0.21, y: 0.6, width: 0.25, height: 0.12)), true),
      ("account-continuation-one-column-is-not-selector", ScreenTextRegion(text: "Beta", bounds: CGRect(x: 0.19, y: 0.48, width: 0.16, height: 0.12)), false),
    ]
    var failures: [String] = []
    for (name, word, expected) in cases {
      if pixels.selectedAccountGutter(beside: word, header: header) != expected { failures.append(name) }
    }
    return (cases.count, failures)
  }

  static func checkCreditChoiceFocusControls() -> (count: Int, failures: [String]) {
    func box(_ text: String, _ x: Double, _ y: Double, _ width: Double) -> ScreenTextRegion {
      ScreenTextRegion(text: text, bounds: CGRect(x: x, y: y, width: width, height: 0.06))
    }
    let context = [box("Credit usage", 0.1, 0.8, 0.7), box("[alpha-tag]", 0.2, 0.7, 0.5)]
    let allow = box("Allow", 0.3, 0.43, 0.32)
    let disallow = box("Disallow", 0.38, 0.5, 0.36)
    let actions = [box("Cancel", 0.15, 0.2, 0.2), box("Select", 0.4, 0.2, 0.2)]
    let raw = box(">Allow", 0.3, 0.43, 0.32)
    func pixels(glyphColumns: Int = 6, glyphRows: Int = 8, siblingYellow: Bool = false,
      selectEnabled: Bool = true) -> OriginalChoicePixels {
      var bytes = [UInt8](repeating: 0, count: 300 * 200 * 4)
      func paint(_ xs: Range<Int>, _ ys: Range<Int>, _ rgb: (UInt8, UInt8, UInt8)) {
        for y in ys { for x in xs {
          let offset = y * 300 * 4 + x * 4
          bytes[offset] = rgb.0; bytes[offset + 1] = rgb.1; bytes[offset + 2] = rgb.2
        } }
      }
      paint(94..<(94 + glyphColumns), 104..<(104 + glyphRows), (220, 220, 20))
      paint(116..<180, 104..<112, (220, 220, 20))
      if siblingYellow { paint(116..<180, 90..<98, (220, 220, 20)) }
      if selectEnabled { paint(122..<170, 150..<158, (240, 240, 240)) }
      return OriginalChoicePixels(controlWidth: 300, controlHeight: 200, controlBytes: bytes)
    }
    let base = [allow, disallow] + actions
    let cases: [(String, [ScreenTextRegion], [ScreenTextRegion], OriginalChoicePixels, String?)] = [
      ("credit-fused-allow", base, context + [raw], pixels(), "allow"),
      ("credit-spaced-allow", [box("Allow", 0.38, 0.43, 0.24), disallow] + actions,
        context + [box("> Allow", 0.3, 0.43, 0.32)], pixels(), "allow"),
      ("credit-fused-white-glyph", base, context + [raw], pixels(glyphColumns: 0), nil),
      ("credit-yellow-label-no-attested-glyph", base, context + [allow], pixels(glyphColumns: 0), nil),
      ("credit-fused-yellow-sibling", base, context + [raw], pixels(siblingYellow: true), nil),
      ("credit-fused-missing-sibling", [allow] + actions, context + [raw], pixels(), nil),
      ("credit-fused-sibling-outside", [allow, box("Disallow", 0.8, 0.5, 0.1)] + actions,
        context + [raw], pixels(), nil),
      ("credit-fused-sibling-distant", [allow, box("Disallow", 0.38, 0.65, 0.36)] + actions,
        context + [raw], pixels(), nil),
      ("credit-fused-one-column-glyph", base, context + [raw], pixels(glyphColumns: 1), nil),
      ("credit-fused-short-glyph", base, context + [raw], pixels(glyphRows: 2), nil),
      ("credit-fused-disabled-select", base, context + [raw], pixels(selectEnabled: false), nil),
      ("credit-fused-duplicate-word", base + [allow], context + [raw], pixels(), nil),
      ("credit-fused-ambiguous-prefix", base, context + [raw, box(">Disallow", 0.38, 0.5, 0.36)],
        pixels(), nil),
      ("credit-fused-wrong-identity", base, [context[0], box("[beta-tag]", 0.2, 0.7, 0.5), raw],
        pixels(), nil),
      ("credit-fused-selected-disallow", [box("Disallow", 0.3, 0.43, 0.32), box("Allow", 0.38, 0.5, 0.36)] + actions,
        context + [box(">Disallow", 0.3, 0.43, 0.32)], pixels(), "disallow"),
    ]
    var failures: [String] = []
    for (name, words, rows, original, expected) in cases {
      let selected = selectedCreditChoice(words: words, rawRows: rows, pixels: original)
        .map { normalizedMarkerText($0.text) }
      if selected != expected { failures.append(name) }
    }
    let titleCases: [(String, [ScreenTextRegion], Bool)] = [
      ("forbidden-title-separate-rows",
        [box("St Upd New", 0.64, 0.78, 0.24), box("Session filters", 0.08, 0.66, 0.43)], false),
      ("forbidden-title-one-observation", [box("New session Help", 0.08, 0.66, 0.55)], true),
      ("forbidden-title-one-row-split",
        [box("New", 0.08, 0.66, 0.12), box("session", 0.22, 0.66, 0.25)], true),
      ("forbidden-title-reversed-recognition-order",
        [box("session", 0.22, 0.66, 0.25), box("New", 0.08, 0.66, 0.12)], true),
      ("forbidden-title-invalid-bounds", [box("unreadable", .nan, 0.66, 0.2)], true),
      ("forbidden-title-ambiguous-rows",
        [box("upper", 0.08, 0.65, 0.2), box("lower", 0.08, 0.5, 0.2),
          ScreenTextRegion(text: "overlapping", bounds: CGRect(x: 0.3, y: 0.2, width: 0.2, height: 0.55))],
        true),
    ]
    for (name, regions, forbidden) in titleCases {
      let result = observe(checkpoint: .compactSessionsClientNoMatch,
        recognizedLines: regions.map(\.text), regions: regions)
      if result.unexpectedMarkers.contains("New session") != forbidden { failures.append(name) }
    }
    return (cases.count + titleCases.count, failures)
  }
}

// In-memory RGB samples exercise the real pixel classifier without an image
// encoder, temporary image files or a replacement measurement callback.
private extension OriginalChoicePixels {
  init(controlWidth: Int, controlHeight: Int, controlBytes: [UInt8]) {
    width = controlWidth; height = controlHeight; stride = controlWidth * 4; bytes = controlBytes
  }
}
