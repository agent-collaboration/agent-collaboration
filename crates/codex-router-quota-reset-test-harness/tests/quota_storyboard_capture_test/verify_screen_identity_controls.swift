import Foundation

extension VerifyScreenImage {
  static func creditChoiceControlRegions() -> [ScreenTextRegion] {
    [
      ScreenTextRegion(text: "Credit usage", bounds: CGRect(x: 0.10, y: 0.8, width: 0.4, height: 0.04)),
      ScreenTextRegion(text: "東京… [alpha-tag]", bounds: CGRect(x: 0.10, y: 0.73, width: 0.5, height: 0.04)),
      ScreenTextRegion(text: "Use credits:", bounds: CGRect(x: 0.10, y: 0.66, width: 0.4, height: 0.04)),
      ScreenTextRegion(text: "Disallow", bounds: CGRect(x: 0.15, y: 0.60, width: 0.25, height: 0.04)),
      ScreenTextRegion(text: "Allow", bounds: CGRect(x: 0.15, y: 0.55, width: 0.2, height: 0.04)),
      ScreenTextRegion(text: "Cancel", bounds: CGRect(x: 0.10, y: 0.25, width: 0.18, height: 0.04)),
      ScreenTextRegion(text: "Select", bounds: CGRect(x: 0.35, y: 0.25, width: 0.18, height: 0.04)),
    ]
  }

  static func checkCreditChoiceMatchingControls() -> (count: Int, failures: [String]) {
    let base = creditChoiceControlRegions()
    func moved(_ index: Int, text: String? = nil, y: CGFloat? = nil) -> [ScreenTextRegion] {
      var rows = base
      let original = rows[index]
      rows[index] = ScreenTextRegion(text: text ?? original.text,
        bounds: CGRect(x: original.bounds.minX, y: y ?? original.bounds.minY,
          width: original.bounds.width, height: original.bounds.height))
      return rows
    }
    let tests: [(String, [ScreenTextRegion], String?, Bool)] = [
      ("credit-choice-visible-field", base, "allow", true),
      ("credit-choice-scrolled-field-label", base.enumerated().filter { $0.offset != 2 }.map(\.element), "allow", true),
      ("credit-choice-recognition-order", Array(base.reversed()), "allow", true),
      ("credit-choice-wrong-focus", base, "disallow", false),
      ("credit-choice-no-yellow-text", base, nil, false),
      ("credit-choice-no-yellow-gutter", base, nil, false),
      ("credit-choice-ambiguous-focus", base, nil, false),
      ("credit-choice-disabled-select", base, nil, false),
      ("credit-choice-wrong-title", moved(0, text: "Account filters"), "allow", false),
      ("credit-choice-duplicate-title", base + [base[0]], "allow", false),
      ("credit-choice-wrong-tag", moved(1, text: "[beta-tag]"), "allow", false),
      ("credit-choice-duplicate-tag", base + [base[1]], "allow", false),
      ("credit-choice-background-allow", moved(4, y: 0.9), "allow", false),
      ("credit-choice-hidden-select", moved(6, y: 0.9), "allow", false),
      ("credit-choice-missing-cancel", base.enumerated().filter { $0.offset != 5 }.map(\.element), "allow", false),
      ("credit-choice-save-not-select", moved(6, text: "Save"), "allow", false),
      ("credit-choice-malformed-bounds", moved(4, y: .nan), "allow", false),
      ("credit-choice-unavailable-option", base + [ScreenTextRegion(text: "Unavailable", bounds: CGRect(x: 0.45, y: 0.55, width: 0.3, height: 0.04))], "allow", false),
      ("credit-choice-ambiguous-allow", base + [base[4]], "allow", false),
      ("credit-choice-clipped-option", moved(4, y: 0.15), "allow", false),
    ]
    var failures: [String] = []
    for (name, rows, selected, shouldPass) in tests {
      let result = observe(checkpoint: .componentQuotaPolicyChoice,
        recognizedLines: rows.map(\.text), regions: rows, selectedDecision: selected)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    return (tests.count, failures)
  }

  static func observationControlRegions(_ value: String, inline: Bool = false)
    -> [ScreenTextRegion]
  {
    var regions = [
      ScreenTextRegion(text: "Options · Credits", bounds: CGRect(x: 0.10, y: 0.8, width: 0.5, height: 0.04)),
      ScreenTextRegion(text: "東京… [alpha-tag]", bounds: CGRect(x: 0.10, y: 0.72, width: 0.5, height: 0.04)),
      ScreenTextRegion(text: "Observation", bounds: CGRect(x: 0.10, y: 0.6, width: 0.22, height: 0.04)),
      ScreenTextRegion(text: "Close", bounds: CGRect(x: 0.10, y: 0.2, width: 0.1, height: 0.04)),
    ]
    var left: CGFloat = inline ? 0.40 : 0.10
    for word in value.split(separator: " ") {
      let width = CGFloat(word.count) * 0.02
      regions.append(ScreenTextRegion(
        text: String(word), bounds: CGRect(x: left, y: inline ? 0.6 : 0.55, width: width, height: 0.04)))
      left += width + 0.02
    }
    return regions
  }

  static func checkObservationMatchingControls() -> (count: Int, failures: [String]) {
    let zero = observationControlRegions("checked 0s ago")
    func changed(_ index: Int, text: String? = nil, x: CGFloat? = nil, y: CGFloat? = nil)
      -> [ScreenTextRegion]
    {
      var rows = zero
      let original = rows[index]
      rows[index] = ScreenTextRegion(
        text: text ?? original.text,
        bounds: CGRect(x: x ?? original.bounds.minX, y: y ?? original.bounds.minY,
          width: original.bounds.width, height: original.bounds.height))
      return rows
    }
    let farValue = zero.enumerated().map { index, row in
      index < 4 ? row : ScreenTextRegion(text: row.text, bounds: row.bounds.offsetBy(dx: 0.45, dy: 0))
    }
    let aboveValue = zero.enumerated().map { index, row in
      index < 4 ? row : ScreenTextRegion(text: row.text, bounds: row.bounds.offsetBy(dx: 0, dy: 0.35))
    }
    let nearestWrong = observationControlRegions("checked 1s ago") + zero.dropFirst(4).map {
      ScreenTextRegion(text: $0.text, bounds: $0.bounds.offsetBy(dx: 0, dy: -0.07))
    }
    let tests: [(String, [ScreenTextRegion], Bool)] = [
      ("observation-measured-zero-stacked", zero, true),
      ("observation-measured-slashed-zero-stacked", observationControlRegions("checked øs ago"), true),
      ("observation-zero-inline", observationControlRegions("checked 0s ago", inline: true), true),
      ("observation-slashed-zero-inline", observationControlRegions("checked øs ago", inline: true), true),
      ("observation-reject-one-second", observationControlRegions("checked 1s ago"), false),
      ("observation-reject-sixty-seconds", observationControlRegions("checked 60s ago"), false),
      ("observation-reject-double-zero", observationControlRegions("checked 00s ago"), false),
      ("observation-reject-wrong-unit", observationControlRegions("checked 0m ago"), false),
      ("observation-reject-incomplete", observationControlRegions("checked 0s"), false),
      ("observation-reject-extra-text", observationControlRegions("checked 0s ago extra"), false),
      ("observation-reject-old-unknown", observationControlRegions("not checked"), false),
      ("observation-reject-wrong-label", changed(2, text: "Age"), false),
      ("observation-reject-duplicate-label", zero + [zero[2]], false),
      ("observation-reject-wrong-header", changed(0, text: "Options · Resets"), false),
      ("observation-reject-duplicate-header", zero + [zero[0]], false),
      ("observation-reject-wrong-tag", changed(1, text: "[beta-tag]"), false),
      ("observation-reject-duplicate-tag", zero + [zero[1]], false),
      ("observation-reject-right-column", farValue, false),
      ("observation-reject-nearest-wrong-value", nearestWrong, false),
      ("observation-reject-malformed-geometry", changed(2, y: .nan), false),
      ("observation-reject-background-value", aboveValue, false),
      ("observation-reject-missing-close", zero.enumerated().filter { $0.offset != 3 }.map(\.element), false),
      ("observation-reject-label-below-close", changed(2, y: 0.15), false),
      ("observation-reject-missing-tag", zero.enumerated().filter { $0.offset != 1 }.map(\.element), false),
    ]
    var failures: [String] = []
    for (name, rows, shouldPass) in tests {
      // Recognition order is independent of layout. Wrong geometry must fail
      // even when the raw strings alone spell the expected fact in order.
      let ordered = rows.filter { normalizedMarkerText($0.text) != "close" }
        + rows.filter { normalizedMarkerText($0.text) == "close" }
      let result = observe(checkpoint: .componentQuotaCreditsRefreshedObservation,
        recognizedLines: ordered.map(\.text), regions: rows)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    return (tests.count, failures)
  }

  static func checkIdentityMatchingControls() -> (count: Int, failures: [String]) {
    var failures: [String] = []
    let nameHeader = [
      ScreenTextRegion(
        text: "Account", bounds: CGRect(x: 0.10, y: 0.75, width: 0.21, height: 0.04)),
      ScreenTextRegion(text: "Status", bounds: CGRect(x: 0.43, y: 0.75, width: 0.18, height: 0.04)),
    ]
    // Actual original32 OCR row geometry; pixel/gutter acquisition is separately
    // exercised on the retained original, never fabricated as screenshot proof.
    let nameBody = [
      ScreenTextRegion(
        text: "CLaude", bounds: CGRect(x: 0.09, y: 0.71, width: 0.20, height: 0.035)),
      ScreenTextRegion(
        text: "Research", bounds: CGRect(x: 0.10, y: 0.665, width: 0.25, height: 0.04)),
      ScreenTextRegion(
        text: "Account", bounds: CGRect(x: 0.10, y: 0.627, width: 0.22, height: 0.035)),
      ScreenTextRegion(
        text: "Beta Full", bounds: CGRect(x: 0.10, y: 0.582, width: 0.28, height: 0.038)),
      ScreenTextRegion(text: "Name", bounds: CGRect(x: 0.10, y: 0.540, width: 0.13, height: 0.039)),
      ScreenTextRegion(
        text: "• Claude", bounds: CGRect(x: 0.09, y: 0.495, width: 0.26, height: 0.04)),
    ]
    let neighbors = [
      ScreenTextRegion(text: "beta", bounds: CGRect(x: 0.68, y: 0.71, width: 0.12, height: 0.03)),
      ScreenTextRegion(text: "detail", bounds: CGRect(x: 0.67, y: 0.67, width: 0.18, height: 0.03)),
      ScreenTextRegion(
        text: "clients", bounds: CGRect(x: 0.43, y: 0.624, width: 0.22, height: 0.037)),
    ]
    func nameRegion(
      _ original: ScreenTextRegion, text: String? = nil, x: CGFloat? = nil,
      y: CGFloat? = nil
    ) -> ScreenTextRegion {
      ScreenTextRegion(
        text: text ?? original.text,
        bounds: CGRect(
          x: x ?? original.bounds.minX, y: y ?? original.bounds.minY,
          width: original.bounds.width, height: original.bounds.height))
    }
    // 2026-10-23 C1/C5: wide browse has Account beside "Quota left · reset in";
    // folded browse repeats a lone Account heading above every account block.
    let quotaNeighbour = ScreenTextRegion(
      text: "Quota left • reset in", bounds: CGRect(x: 0.43, y: 0.75, width: 0.40, height: 0.04))
    let foldedTopHeading = ScreenTextRegion(
      text: "Account", bounds: CGRect(x: 0.10, y: 0.95, width: 0.21, height: 0.04))
    let foldedUnselected = [
      ScreenTextRegion(text: "OpenAI", bounds: CGRect(x: 0.10, y: 0.90, width: 0.20, height: 0.035)),
      ScreenTextRegion(text: "Beta", bounds: CGRect(x: 0.33, y: 0.90, width: 0.13, height: 0.035)),
    ]
    // An offscreen heading must not be replaced by a name's own "Account" word,
    // even when that word happens to align with the Account column.
    let nameWordRow = [
      ScreenTextRegion(text: "Research", bounds: CGRect(x: 0.00, y: 0.80, width: 0.09, height: 0.04)),
      ScreenTextRegion(text: "Account", bounds: CGRect(x: 0.10, y: 0.80, width: 0.21, height: 0.04)),
    ]
    let nameCases: [(String, [ScreenTextRegion], [Double], Bool)] = [
      ("selected-name-wrapped", nameHeader + nameBody + neighbors, [0.71], true),
      ("selected-name-wide-quota-heading", [nameHeader[0], quotaNeighbour] + nameBody, [0.71], true),
      ("selected-name-folded-lone-heading", [nameHeader[0]] + nameBody, [0.71], true),
      (
        "selected-name-folded-second-block",
        [foldedTopHeading] + foldedUnselected + [nameHeader[0]] + nameBody, [0.71], true
      ),
      (
        "selected-name-folded-unselected-block-gutterless",
        [foldedTopHeading] + foldedUnselected + [nameHeader[0]] + nameBody, [0.90], false
      ),
      ("selected-name-name-word-is-not-heading", nameWordRow + nameBody, [0.71], false),
      (
        "selected-name-heading-neighbour-too-close",
        [
          nameHeader[0],
          ScreenTextRegion(text: "Alpha", bounds: CGRect(x: 0.32, y: 0.75, width: 0.13, height: 0.04)),
        ] + nameBody, [0.71], false
      ),
      (
        "selected-name-ocr-order", nameHeader + neighbors + Array(nameBody.reversed()), [0.71], true
      ),
      ("selected-name-missing-gutter", nameHeader + nameBody, [], false),
      ("selected-name-other-row-gutter", nameHeader + nameBody, [0.665], false),
      ("selected-name-ambiguous-gutter", nameHeader + nameBody, [0.71, 0.665], false),
      ("selected-name-missing-geometry", [], [0.71], false),
      ("selected-name-missing-header", nameBody, [0.71], false),
      ("selected-name-ambiguous-header", nameHeader + [nameHeader[0]] + nameBody, [0.71], false),
      (
        "selected-name-missing-word",
        nameHeader + nameBody.enumerated().filter { $0.offset != 1 }.map(\.element), [0.71], false
      ),
      (
        "selected-name-wrong-account",
        nameHeader + [nameRegion(nameBody[0], text: "OpenAI")] + Array(nameBody.dropFirst()),
        [0.71], false
      ),
      (
        "selected-name-suffix",
        nameHeader + Array(nameBody.prefix(4)) + [nameRegion(nameBody[4], text: "Namex")], [0.71],
        false
      ),
      (
        "selected-name-reordered",
        nameHeader + [
          nameBody[0], nameRegion(nameBody[1], text: "Account"),
          nameRegion(nameBody[2], text: "Research"),
        ] + Array(nameBody.dropFirst(3)), [0.71], false
      ),
      (
        "selected-name-neighbor-column", nameHeader + nameBody.map { nameRegion($0, x: 0.65) },
        [0.71], false
      ),
      (
        "selected-name-other-row",
        nameHeader + nameBody.map { nameRegion($0, y: $0.bounds.minY - 0.3) }, [0.71], false
      ),
      (
        "selected-name-missing-row",
        nameHeader + [nameBody[0]]
          + nameBody.dropFirst().map { nameRegion($0, y: $0.bounds.minY - 0.1) }, [0.71], false
      ),
      (
        "selected-name-malformed",
        nameHeader + [nameRegion(nameBody[0], x: -.infinity)] + Array(nameBody.dropFirst()), [0.71],
        false
      ),
    ]
    for (name, regions, gutters, shouldPass) in nameCases {
      let selected = selectedAccountName(
        wordRegions: regions, pixelWidth: 350,
        hasGutter: { region, _ in gutters.contains { abs($0 - Double(region.bounds.minY)) < 0.001 } },
        hasSelectedText: { !$0.text.hasPrefix("•") }, hasBlankOverlap: { _, _ in false })
      let result = observe(
        checkpoint: .componentQuotaNextAccount,
        recognizedLines: [
          "Quota status", "Claude Research Account Beta Full Name", "Claude",
          "F4 Details F3 Filters · F5 Pool F1 Help · Ctrl+R Options",
        ],
        regions: [
          ScreenTextRegion(
            text: "Quota status", bounds: CGRect(x: 0.03, y: 0.92, width: 0.37, height: 0.04)),
          // C6: the current header keeps only the client count and its actions.
          ScreenTextRegion(
            text: "Clients 1 Pool details Help",
            bounds: CGRect(x: 0.5, y: 0.92, width: 0.45, height: 0.04)),
          nameHeader[0],
        ],
        selectedAccountName: selected)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    // Exact native boxes from the unchanged 32-column Alpha original, including
    // the measured 0.602-pixel OpenAI/Research overlap. These are parser controls.
    let alphaHeader = [
      ScreenTextRegion(text: "Account", bounds: CGRect(x: 0.10277579143562268, y: 0.7558840976683721, width: 0.21159127371651784, height: 0.029554118794843154)),
      ScreenTextRegion(text: "Status", bounds: CGRect(x: 0.4283069592828462, y: 0.7513521450253868, width: 0.1891003635951451, height: 0.0386180247157073)),
    ]
    let alphaWords = [
      ScreenTextRegion(text: "東京", bounds: CGRect(x: 0.0992857130612245, y: 0.7066115702479339, width: 0.11214285714285716, height: 0.04132231404958686)),
      ScreenTextRegion(text: "OpenAI", bounds: CGRect(x: 0.09663597053690288, y: 0.6599132045808573, width: 0.18958520071847101, height: 0.0438099537999177)),
      ScreenTextRegion(text: "Research", bounds: CGRect(x: 0.09714285923809517, y: 0.6239669423898072, width: 0.2514285714285714, height: 0.037190082644628086)),
      ScreenTextRegion(text: "Account", bounds: CGRect(x: 0.0969656067401427, y: 0.5820810974119723, width: 0.2232116481236049, height: 0.038317144409684145)),
      ScreenTextRegion(text: "Alpha", bounds: CGRect(x: 0.09695308140566908, y: 0.5364356953163455, width: 0.17181126185825893, height: 0.042160101173337794)),
      ScreenTextRegion(text: "Long", bounds: CGRect(x: 0.27552042059466314, y: 0.5373093753841167, width: 0.13038363865443642, height: 0.041957409913874866)),
      ScreenTextRegion(text: "Name", bounds: CGRect(x: 0.09714285594285717, y: 0.5000000003719008, width: 0.13142857142857145, height: 0.037190082644628086)),
    ]
    func alphaChanged(_ index: Int, _ value: ScreenTextRegion) -> [ScreenTextRegion] {
      var rows = alphaWords; rows[index] = value; return rows
    }
    let alphaGutter = Double(alphaWords[0].bounds.minY)
    let alphaCases: [(String, [ScreenTextRegion], [Double], Bool)] = [
      ("selected-alpha-native-overlapping-rows", alphaWords, [alphaGutter], true),
      ("selected-alpha-native-reversed-observations", Array(alphaWords.reversed()), [alphaGutter], true),
      ("selected-alpha-missing-word", Array(alphaWords.prefix(2)) + Array(alphaWords.dropFirst(3)), [alphaGutter], false),
      ("selected-alpha-reordered-words", alphaChanged(1, nameRegion(alphaWords[1], text: "Research")), [alphaGutter], false),
      ("selected-alpha-ambiguous-overlap", alphaChanged(2, nameRegion(alphaWords[2], y: 0.67)), [alphaGutter], false),
      ("selected-alpha-missing-row", alphaChanged(2, nameRegion(alphaWords[2], y: 0.60)), [alphaGutter], false),
      ("selected-alpha-duplicate-gutter", alphaWords, [alphaGutter, Double(alphaWords[1].bounds.minY)], false),
      ("selected-alpha-duplicate-word", alphaWords + [alphaWords[1]], [alphaGutter], false),
      ("selected-alpha-foreign-prefix", alphaChanged(0, nameRegion(alphaWords[0], text: "Claude")), [alphaGutter], false),
      ("selected-alpha-other-column", alphaChanged(2, nameRegion(alphaWords[2], x: 0.65)), [alphaGutter], false),
    ]
    for (name, rows, gutters, expected) in alphaCases {
      let selected = selectedAccountName(wordRegions: alphaHeader + rows, pixelWidth: 350,
        hasGutter: { row, _ in gutters.contains { abs($0 - Double(row.bounds.minY)) < 0.000001 } },
        hasSelectedText: { _ in true }, hasBlankOverlap: { _, _ in false })
      if (selected.map(compactHelpIdentity) == compactHelpIdentity("東京 OpenAI Research Account Alpha Long Name")) != expected {
        failures.append(name)
      }
    }
    // Exact current40 native metadata, not fabricated screenshot pixels.
    let fortyHeader = [
      ScreenTextRegion(text: "Account", bounds: CGRect(x: 0.07854030160860417, y: 0.751358844283845, width: 0.17625273598564997, height: 0.03447239458068352)),
      ScreenTextRegion(text: "Status", bounds: CGRect(x: 0.4212962973544973, y: 0.752066115879575, width: 0.1481481481481482, height: 0.03305785123966942)),
    ]
    let fortyWords = [
      ScreenTextRegion(text: "東京", bounds: CGRect(x: 0.05078312530330515, y: 0.7025230204336153, width: 0.14224420653449169, height: 0.046176575432139044)),
      ScreenTextRegion(text: "OpenAI", bounds: CGRect(x: 0.1927368323828319, y: 0.7017136283291471, width: 0.15925781815140339, height: 0.04626319822201064)),
      ScreenTextRegion(text: "Research", bounds: CGRect(x: 0.07393418565898333, y: 0.6647327620408203, width: 0.20398348349112053, height: 0.04243530320727129)),
      ScreenTextRegion(text: "Account", bounds: CGRect(x: 0.07407407680555549, y: 0.6198347107438016, width: 0.18518518518518517, height: 0.04132231404958686)),
      ScreenTextRegion(text: "Alpha", bounds: CGRect(x: 0.26504629578703703, y: 0.6198347107438016, width: 0.13310185185185186, height: 0.04132231404958686)),
      ScreenTextRegion(text: "Long", bounds: CGRect(x: 0.07393478543046476, y: 0.5785001606450559, width: 0.11602139472961424, height: 0.04195260607506612)),
      ScreenTextRegion(text: "Name", bounds: CGRect(x: 0.1954602940969147, y: 0.5778943405238186, width: 0.10560492232993798, height: 0.041895811222801504)),
    ]
    func fortyChanged(_ index: Int, _ value: ScreenTextRegion) -> [ScreenTextRegion] {
      var rows = fortyWords; rows[index] = value; return rows
    }
    let fortyGutter = Double(fortyWords[0].bounds.minY)
    let pixelBoundary = floor(fortyWords[0].bounds.maxX * 432)
    let selectorLeft = fortyHeader[0].bounds.minX - 2 * fortyHeader[0].bounds.width / 7
    let fortyCases: [(String, [ScreenTextRegion], [Double], Int, Bool, Bool, Bool)] = [
      ("selected-forty-native-boxes", fortyWords, [fortyGutter], 432, true, true, true),
      ("selected-forty-native-order", Array(fortyWords.reversed()), [fortyGutter], 432, true, true, true),
      ("selected-forty-one-shared-pixel", fortyChanged(1, nameRegion(fortyWords[1], x: (pixelBoundary + 0.2) / 432)), [fortyGutter], 432, true, true, true),
      ("selected-forty-touching-edge", fortyChanged(1, nameRegion(fortyWords[1], x: fortyWords[0].bounds.maxX)), [fortyGutter], 432, true, true, true),
      ("selected-forty-no-gutter", fortyWords, [], 432, true, true, false),
      ("selected-forty-outside-gutter", fortyChanged(0, nameRegion(fortyWords[0], x: selectorLeft - 1 / 432)), [fortyGutter], 432, true, true, false),
      ("selected-forty-first-word-not-yellow", fortyWords, [fortyGutter], 432, false, true, false),
      ("selected-forty-foreign-prefix", fortyChanged(0, nameRegion(fortyWords[0], text: "Claude")), [fortyGutter], 432, true, true, false),
      ("selected-forty-no-cjk", Array(fortyWords.dropFirst()), [fortyGutter], 432, true, true, false),
      ("selected-forty-duplicate-cjk", fortyWords + [fortyWords[0]], [fortyGutter], 432, true, true, false),
      ("selected-forty-two-pixel-overlap", fortyChanged(1, nameRegion(fortyWords[1], x: fortyWords[0].bounds.maxX - 2 / 432)), [fortyGutter], 432, true, false, false),
      ("selected-forty-merged-word-box", fortyChanged(1, nameRegion(fortyWords[1], x: fortyWords[0].bounds.minX + 0.03)), [fortyGutter], 432, true, true, false),
      ("selected-forty-duplicate-word", fortyWords + [fortyWords[1]], [fortyGutter], 432, true, true, false),
      ("selected-forty-reordered-text", fortyChanged(1, nameRegion(fortyWords[1], text: "Research")), [fortyGutter], 432, true, true, false),
      ("selected-forty-other-column", fortyChanged(2, nameRegion(fortyWords[2], x: 0.65)), [fortyGutter], 432, true, true, false),
      ("selected-forty-before-first-word", fortyChanged(1, nameRegion(fortyWords[1], x: 0.04)), [fortyGutter], 432, true, true, false),
      ("selected-forty-zero-image-width", fortyWords, [fortyGutter], 0, true, true, false),
      ("selected-forty-negative-image-width", fortyWords, [fortyGutter], -1, true, true, false),
      ("selected-forty-shares-two-pixel-columns", fortyChanged(1, nameRegion(fortyWords[1], x: (pixelBoundary - 0.1) / 432)), [fortyGutter], 432, true, false, false),
      ("selected-forty-continuation-gutter", fortyWords, [Double(fortyWords[2].bounds.minY)], 432, true, true, false),
      ("selected-forty-two-gutter-rows", fortyWords, [fortyGutter, Double(fortyWords[2].bounds.minY)], 432, true, true, false),
      ("selected-forty-malformed-word", fortyChanged(2, nameRegion(fortyWords[2], x: .nan)), [fortyGutter], 432, true, true, false),
    ]
    for (name, rows, gutters, width, firstYellow, blankOverlap, expected) in fortyCases {
      let selected = selectedAccountName(wordRegions: fortyHeader + rows, pixelWidth: width,
        hasGutter: { row, _ in gutters.contains { abs($0 - Double(row.bounds.minY)) < 0.0000001 } },
        hasSelectedText: { firstYellow || $0.text != "東京" },
        hasBlankOverlap: { _, _ in blankOverlap })
      if (selected.map(compactHelpIdentity) == compactHelpIdentity("東京 OpenAI Research Account Alpha Long Name")) != expected {
        failures.append(name)
      }
    }
    // Exact failed48 native boxes and observed blank-intersection premise.
    // The old raster-column guard rejects this correct selected name.
    let fortyEightHeader = [
      ScreenTextRegion(text: "Account", bounds: CGRect(x: 0.14769542510296768, y: 0.7552177602362862, width: 0.14818892126417343, height: 0.03088679392475724)),
      ScreenTextRegion(text: "Status", bounds: CGRect(x: 0.43190661357976656, y: 0.7520661160743802, width: 0.12840466926070038, height: 0.037190082644628086)),
    ]
    let fortyEightWords = [
      ScreenTextRegion(text: "東京", bounds: CGRect(x: 0.14396887307392991, y: 0.7024793388429752, width: 0.10019455252918288, height: 0.045454545454545525)),
      ScreenTextRegion(text: "OpenAI", bounds: CGRect(x: 0.2417315166536965, y: 0.7024793388429752, width: 0.13570038910505836, height: 0.045454545454545525)),
      ScreenTextRegion(text: "Research", bounds: CGRect(x: 0.14381028861230508, y: 0.6645351584668986, width: 0.17152339475164155, height: 0.04283051057295362)),
      ScreenTextRegion(text: "Account", bounds: CGRect(x: 0.14396887389105054, y: 0.6198347107438016, width: 0.15515564202334628, height: 0.045454545454545525)),
      ScreenTextRegion(text: "Alpha", bounds: CGRect(x: 0.3044747081712062, y: 0.6198347107438016, width: 0.12305447470817121, height: 0.045454545454545525)),
      ScreenTextRegion(text: "Long", bounds: CGRect(x: 0.14396887230869, y: 0.578512396694215, width: 0.09727626459143968, height: 0.04132231404958664)),
      ScreenTextRegion(text: "Name", bounds: CGRect(x: 0.24610894854085605, y: 0.578512396694215, width: 0.08852140077821011, height: 0.04132231404958664)),
    ]
    func fortyEightChanged(_ index: Int, _ value: ScreenTextRegion) -> [ScreenTextRegion] {
      var rows = fortyEightWords; rows[index] = value; return rows
    }
    let fortyEightGutter = Double(fortyEightWords[0].bounds.minY)
    let fortyEightCases: [(String, [ScreenTextRegion], [Double], Bool, Bool, Bool)] = [
      ("selected-forty-eight-native-blank-overlap", fortyEightWords, [fortyEightGutter], true, true, true),
      ("selected-forty-eight-reversed-blank-overlap", Array(fortyEightWords.reversed()), [fortyEightGutter], true, true, true),
      ("selected-forty-eight-ink-overlap", fortyEightWords, [fortyEightGutter], true, false, false),
      ("selected-forty-eight-duplicate-first", fortyEightWords + [fortyEightWords[0]], [fortyEightGutter], true, true, false),
      ("selected-forty-eight-merged-box", fortyEightChanged(1, nameRegion(fortyEightWords[1], x: fortyEightWords[0].bounds.minX + 0.01)), [fortyEightGutter], true, true, false),
      ("selected-forty-eight-foreign-prefix", fortyEightChanged(0, nameRegion(fortyEightWords[0], text: "Claude")), [fortyEightGutter], true, true, false),
      ("selected-forty-eight-missing-word", Array(fortyEightWords.prefix(2)) + Array(fortyEightWords.dropFirst(3)), [fortyEightGutter], true, true, false),
      ("selected-forty-eight-neighbor-column", fortyEightChanged(2, nameRegion(fortyEightWords[2], x: 0.65)), [fortyEightGutter], true, true, false),
      ("selected-forty-eight-no-gutter", fortyEightWords, [], true, true, false),
      ("selected-forty-eight-first-not-yellow", fortyEightWords, [fortyEightGutter], false, true, false),
      ("selected-forty-eight-wrong-cjk", fortyEightChanged(0, nameRegion(fortyEightWords[0], text: "京")), [fortyEightGutter], true, true, false),
      ("selected-forty-eight-malformed-box", fortyEightChanged(2, nameRegion(fortyEightWords[2], x: .nan)), [fortyEightGutter], true, true, false),
    ]
    for (name, rows, gutters, firstYellow, blankOverlap, expected) in fortyEightCases {
      let selected = selectedAccountName(wordRegions: fortyEightHeader + rows, pixelWidth: 514,
        hasGutter: { row, _ in gutters.contains { abs($0 - Double(row.bounds.minY)) < 0.0000001 } },
        hasSelectedText: { firstYellow || $0.text != "東京" },
        hasBlankOverlap: { _, _ in blankOverlap })
      if (selected.map(compactHelpIdentity) == compactHelpIdentity("東京 OpenAI Research Account Alpha Long Name")) != expected {
        failures.append(name)
      }
    }
    // Full actual native list, including the zero-box whitespace ingress artifact.
    let fullNativeWords: [(ScreenTextRegion, Int, Bool)] = [
      (ScreenTextRegion(text: "Quota", bounds: CGRect(x: 0.03891050610894941, y: 0.9132231404958677, width: 0.1215953307392996, height: 0.04132231404958686)), 0, false),
      (ScreenTextRegion(text: "status", bounds: CGRect(x: 0.16536964754863817, y: 0.9132231404958677, width: 0.12645914396887162, height: 0.04132231404958686)), 0, false),
      (ScreenTextRegion(text: "Pool", bounds: CGRect(x: 0.0428015690461035, y: 0.8760330578512396, width: 0.21887158048755928, height: 0.037190082644628086)), 0, false),
      (ScreenTextRegion(text: "wide", bounds: CGRect(x: 0.0428015690461035, y: 0.8760330578512396, width: 0.21887158048755928, height: 0.037190082644628086)), 0, false),
      (ScreenTextRegion(text: "responses", bounds: CGRect(x: 0.26605058365758755, y: 0.8760330578512396, width: 0.19309338521400776, height: 0.037190082644628086)), 0, false),
      (ScreenTextRegion(text: "Details", bounds: CGRect(x: 0.7003891050583657, y: 0.9132231404958677, width: 0.16050583657587558, height: 0.04132231404958686)), 0, false),
      (ScreenTextRegion(text: "Help", bounds: CGRect(x: 0.865758753057254, y: 0.9132231404958677, width: 0.08754863813229574, height: 0.04132231404958686)), 0, false),
      (ScreenTextRegion(text: "Accounts", bounds: CGRect(x: 0.49805448762587, y: 0.8801652892561983, width: 0.17509726142141135, height: 0.03305785123966942)), 0, false),
      (ScreenTextRegion(text: "2", bounds: CGRect(x: 0.6770428015564203, y: 0.8801652892561983, width: 0.04280155642023342, height: 0.03305785123966942)), 0, false),
      (ScreenTextRegion(text: "Usable", bounds: CGRect(x: 0.7587548638132295, y: 0.8801652892561983, width: 0.14396887159533078, height: 0.03305785123966942)), 0, false),
      (ScreenTextRegion(text: "2", bounds: CGRect(x: 0.9066147859922179, y: 0.8801652892561983, width: 0.0311284046692607, height: 0.03305785123966942)), 0, false),
      (ScreenTextRegion(text: "All", bounds: CGRect(x: 0.03891051932820469, y: 0.8305785123966942, width: 0.08268481005954371, height: 0.04132231404958675)), 0, false),
      (ScreenTextRegion(text: "Disabled", bounds: CGRect(x: 0.1605058365758755, y: 0.8305785123966942, width: 0.20428015564202331, height: 0.04132231404958675)), 0, false),
      (ScreenTextRegion(text: "Shown", bounds: CGRect(x: 0.36964980544747084, y: 0.8305785123966942, width: 0.1264591439688716, height: 0.04132231404958675)), 0, false),
      (ScreenTextRegion(text: "2", bounds: CGRect(x: 0.5350194552529183, y: 0.8305785123966942, width: 0.07782101167315181, height: 0.04132231404958675)), 0, false),
      (ScreenTextRegion(text: "2", bounds: CGRect(x: 0.5350194552529183, y: 0.8305785123966942, width: 0.07782101167315181, height: 0.04132231404958675)), 0, false),
      (ScreenTextRegion(text: "r", bounds: CGRect(x: 0.617704280155642, y: 0.8305785123966942, width: 0.02821011673151752, height: 0.04132231404958675)), 0, false),
      (ScreenTextRegion(text: "Account", bounds: CGRect(x: 0.14769542510296768, y: 0.7552177602362862, width: 0.14818892126417343, height: 0.03088679392475724)), 0, false),
      (ScreenTextRegion(text: "Status", bounds: CGRect(x: 0.43190661357976656, y: 0.7520661160743802, width: 0.12840466926070038, height: 0.037190082644628086)), 0, false),
      (ScreenTextRegion(text: "Quota", bounds: CGRect(x: 0.6379113987422481, y: 0.7471906858213488, width: 0.1093912087525839, height: 0.03867647864601831)), 0, false),
      (ScreenTextRegion(text: "東京", bounds: CGRect(x: 0.14396887307392991, y: 0.7024793388429752, width: 0.10019455252918288, height: 0.045454545454545525)), 217, true),
      (ScreenTextRegion(text: " ", bounds: CGRect(x: 0, y: 1, width: 0, height: 0)), 0, false),
      (ScreenTextRegion(text: "OpenAI", bounds: CGRect(x: 0.2417315166536965, y: 0.7024793388429752, width: 0.13570038910505836, height: 0.045454545454545525)), 272, true),
      (ScreenTextRegion(text: "available", bounds: CGRect(x: 0.4316596336390496, y: 0.709226566991505, width: 0.19504649537082774, height: 0.040224552154541016)), 0, false),
      (ScreenTextRegion(text: "alpha", bounds: CGRect(x: 0.6257529568563767, y: 0.7041540428851464, width: 0.12203494120200786, height: 0.046237370199408434)), 0, false),
      (ScreenTextRegion(text: "Research", bounds: CGRect(x: 0.14381028861230508, y: 0.6645351584668986, width: 0.17152339475164155, height: 0.04283051057295362)), 293, false),
      (ScreenTextRegion(text: "1", bounds: CGRect(x: 0.43190661702334626, y: 0.6652892561983471, width: 0.038910503599221824, height: 0.04132231404958675)), 0, false),
      (ScreenTextRegion(text: "client", bounds: CGRect(x: 0.47568093283073926, y: 0.6652892561983471, width: 0.12743190661478604, height: 0.04132231404958675)), 0, false),
      (ScreenTextRegion(text: "detail", bounds: CGRect(x: 0.6380320974679204, y: 0.6649353469903216, width: 0.12860506703417585, height: 0.04203013349170526)), 0, false),
      (ScreenTextRegion(text: "Account", bounds: CGRect(x: 0.14396887389105054, y: 0.6198347107438016, width: 0.15515564202334628, height: 0.045454545454545525)), 252, false),
      (ScreenTextRegion(text: "Alpha", bounds: CGRect(x: 0.3044747081712062, y: 0.6198347107438016, width: 0.12305447470817121, height: 0.045454545454545525)), 206, false),
      (ScreenTextRegion(text: "o", bounds: CGRect(x: 0.43287937743190663, y: 0.6198347107438016, width: 0.03745136186770426, height: 0.045454545454545525)), 0, false),
      (ScreenTextRegion(text: "2", bounds: CGRect(x: 0.47568093385214005, y: 0.6198347107438016, width: 0.022373538365758783, height: 0.045454545454545525)), 0, false),
      (ScreenTextRegion(text: "Weekly", bounds: CGRect(x: 0.6376114200631902, y: 0.6179287331333214, width: 0.12944642300735654, height: 0.045134268516351383)), 0, false),
      (ScreenTextRegion(text: "Long", bounds: CGRect(x: 0.14396887230869, y: 0.578512396694215, width: 0.09727626459143968, height: 0.04132231404958664)), 156, false),
      (ScreenTextRegion(text: "Name", bounds: CGRect(x: 0.24610894854085605, y: 0.578512396694215, width: 0.08852140077821011, height: 0.04132231404958664)), 160, false),
      (ScreenTextRegion(text: "resets", bounds: CGRect(x: 0.4355897722614045, y: 0.581843683908155, width: 0.12492940583581591, height: 0.03879197175837745)), 0, false),
      (ScreenTextRegion(text: "OpenAI", bounds: CGRect(x: 0.18264987641275507, y: 0.5361034851813333, width: 0.13275193982551067, height: 0.04300092665617128)), 0, false),
      (ScreenTextRegion(text: "Credits", bounds: CGRect(x: 0.4317761748178978, y: 0.5407125478360231, width: 0.15201185649471055, height: 0.03840961535114884)), 0, false),
      (ScreenTextRegion(text: "62.5k", bounds: CGRect(x: 0.431659106172037, y: 0.49509263254392644, width: 0.11333548697979995, height: 0.04287258652616144)), 0, false),
      (ScreenTextRegion(text: "fresh", bounds: CGRect(x: 0.43190661377431905, y: 0.4586776863223141, width: 0.1089494163424124, height: 0.037190082644628086)), 0, false),
      (ScreenTextRegion(text: "left", bounds: CGRect(x: 0.6420233473540855, y: 0.5371900826446281, width: 0.10505836575875493, height: 0.04132231404958686)), 0, false),
      (ScreenTextRegion(text: "7d", bounds: CGRect(x: 0.6381322962062256, y: 0.499999999535124, width: 0.04669260700389111, height: 0.037190082644628086)), 0, false),
      (ScreenTextRegion(text: "5h", bounds: CGRect(x: 0.6381322952529184, y: 0.45867768574380174, width: 0.046692607003890996, height: 0.037190082644628086)), 0, false),
      (ScreenTextRegion(text: "83", bounds: CGRect(x: 0.762645914897165, y: 0.5826446282467532, width: 0.07003891050583666, height: 0.037190082644628086)), 0, false),
      (ScreenTextRegion(text: "reset", bounds: CGRect(x: 0.7626459130350195, y: 0.5413223137913223, width: 0.1089494163424124, height: 0.037190082644628086)), 0, false),
      (ScreenTextRegion(text: "99", bounds: CGRect(x: 0.7626459138132295, y: 0.41735537165977954, width: 0.07003891050583655, height: 0.037190082644628086)), 0, false),
      (ScreenTextRegion(text: "reset", bounds: CGRect(x: 0.7704280161867705, y: 0.3801652895454546, width: 0.10116731517509725, height: 0.028925619834710647)), 0, false),
      (ScreenTextRegion(text: "left", bounds: CGRect(x: 0.6420233470261255, y: 0.36776859512987004, width: 0.10116731517509736, height: 0.045454545454545414)), 0, false),
      (ScreenTextRegion(text: "5h", bounds: CGRect(x: 0.6381322952918288, y: 0.33471074417355373, width: 0.05058365758754857, height: 0.0371900826446282)), 0, false),
      (ScreenTextRegion(text: "Weekly", bounds: CGRect(x: 0.6373335919426075, y: 0.28662172263927976, width: 0.13629168202441033, height: 0.04331167473280728)), 0, false),
      (ScreenTextRegion(text: "pace", bounds: CGRect(x: 0.7783836982387706, y: 0.2870121408446047, width: 0.09385730876996823, height: 0.04319421910057375)), 0, false),
      (ScreenTextRegion(text: "burn", bounds: CGRect(x: 0.7626459135797665, y: 0.20661157061983482, width: 0.08949416342412453, height: 0.037190082644628086)), 0, false),
      (ScreenTextRegion(text: "unavailable", bounds: CGRect(x: 0.6381322955642024, y: 0.16528925661157023, width: 0.2334630350194552, height: 0.04132231404958675)), 0, false),
      (ScreenTextRegion(text: "CLaude", bounds: CGRect(x: 0.14396887266536962, y: 0.08264462827134988, width: 0.12840466926070038, height: 0.037190082644628086)), 0, false),
      (ScreenTextRegion(text: "available", bounds: CGRect(x: 0.43190661478599224, y: 0.0826446280991735, width: 0.2013618677042801, height: 0.0371900826446282)), 0, false),
      (ScreenTextRegion(text: "beta", bounds: CGRect(x: 0.6376459143968871, y: 0.0826446280991735, width: 0.09630350194552528, height: 0.0371900826446282)), 0, false),
      (ScreenTextRegion(text: "detail", bounds: CGRect(x: 0.7383268445849547, y: 0.0826446280991735, width: 0.13326848249027234, height: 0.0371900826446282)), 0, false),
    ]
    let wordContentCases: [(String, String?, String?)] = [
      ("native-word-space-is-not-content", " ", nil),
      ("native-word-empty-is-not-content", "", nil),
      ("native-word-linebreak-is-not-content", "\t\n", nil),
      ("native-word-nbsp-is-not-content", "\u{00a0}", nil),
      ("native-word-cjk-preserved", "東京", "東京"),
      ("native-word-identifier-preserved", "Alpha-tag", "Alpha-tag"),
      ("native-word-spaced-content-not-rewritten", " Alpha ", " Alpha "),
      ("native-word-format-character-not-dropped", "\u{200b}", "\u{200b}"),
      ("native-word-nil-remains-absent", nil, nil),
    ]
    for (name, raw, expected) in wordContentCases {
      if recognizedWordContent(raw) != expected { failures.append(name) }
    }
    func selectedFullNativeWords(_ rows: [ScreenTextRegion]) -> String? {
      selectedAccountName(wordRegions: rows, pixelWidth: 514,
        hasGutter: { row, _ in fullNativeWords.contains { $0.0.text == row.text && $0.0.bounds == row.bounds && $0.2 } },
        hasSelectedText: { row in fullNativeWords.contains { $0.0.text == row.text && $0.0.bounds == row.bounds && $0.1 >= 8 } },
        hasBlankOverlap: { left, right in left.text == "東京" && right.text == "OpenAI" })
    }
    let semanticNativeWords = fullNativeWords.compactMap { row, _, _ in
      recognizedWordContent(row.text).map { ScreenTextRegion(text: $0, bounds: row.bounds) }
    }
    if selectedFullNativeWords(semanticNativeWords).map(compactHelpIdentity)
      != compactHelpIdentity("東京 OpenAI Research Account Alpha Long Name") {
      failures.append("native-word-full-58-list-preserves-selected-identity")
    }
    let malformedWordBoxes: [(String, CGRect)] = [
      ("native-word-nonblank-zero-box-rejects", CGRect(x: 0, y: 1, width: 0, height: 0)),
      ("native-word-nonblank-outside-box-rejects", CGRect(x: 1, y: 0.5, width: 0.1, height: 0.04)),
      ("native-word-nonblank-nan-box-rejects", CGRect(x: .nan, y: 0.5, width: 0.1, height: 0.04)),
    ]
    for (name, bounds) in malformedWordBoxes {
      let malformed = recognizedWordContent("Alpha").map { ScreenTextRegion(text: $0, bounds: bounds) }
      if let malformed, selectedFullNativeWords(semanticNativeWords + [malformed]) != nil {
        failures.append(name)
      }
    }
    let helpChrome = [
      ScreenTextRegion(
        text: "Quota Help", bounds: CGRect(x: 0.10, y: 0.8, width: 0.3, height: 0.04)),
      ScreenTextRegion(text: "Close", bounds: CGRect(x: 0.10, y: 0.2, width: 0.15, height: 0.04)),
      ScreenTextRegion(
        text: "Current quota", bounds: CGRect(x: 0.10, y: 0.4, width: 0.4, height: 0.04)),
    ]
    let helpName = [
      ScreenTextRegion(
        text: "Account 東京 OpenAI Research",
        bounds: CGRect(x: 0.10, y: 0.65, width: 0.7, height: 0.04)),
      ScreenTextRegion(
        text: "Account Alpha Long Name [alpha-tag]",
        bounds: CGRect(x: 0.10, y: 0.60, width: 0.7, height: 0.04)),
    ]
    // The actual48x12 Down1 body fits the complete field while the next
    // Current quota heading stays offscreen. Close still bounds the body.
    let shortHelpChrome = Array(helpChrome.prefix(2))
    let shortHelpName = [
      ScreenTextRegion(
        text: "Account", bounds: CGRect(x: 0.10, y: 0.7, width: 0.21, height: 0.04)),
      ScreenTextRegion(
        text: "東京OpenAI Research", bounds: CGRect(x: 0.40, y: 0.7, width: 0.50, height: 0.04)),
      ScreenTextRegion(
        text: "Account Alpha Long", bounds: CGRect(x: 0.40, y: 0.65, width: 0.50, height: 0.04)),
      ScreenTextRegion(
        text: "Name ［alpha-tag］", bounds: CGRect(x: 0.40, y: 0.60, width: 0.50, height: 0.04)),
    ]
    let helpCases: [(String, [ScreenTextRegion], Bool)] = [
      ("help-full-captured-name", helpChrome + helpName, true),
      ("help-ocr-order", helpChrome + Array(helpName.reversed()), true),
      (
        "help-missing-name-word",
        helpChrome + [
          helpName[0], nameRegion(helpName[1], text: "Account Alpha Name [alpha-tag]"),
        ], false
      ),
      (
        "help-reordered-name",
        helpChrome + [
          helpName[0], nameRegion(helpName[1], text: "Long Account Alpha Name [alpha-tag]"),
        ], false
      ),
      (
        "help-wrong-captured-account",
        helpChrome + [nameRegion(helpName[0], text: "Account Claude Research"), helpName[1]], false
      ),
      (
        "help-wrong-captured-tag",
        helpChrome + [
          helpName[0], nameRegion(helpName[1], text: "Account Alpha Long Name [beta-tag]"),
        ], false
      ),
      (
        "help-background-full-name",
        helpChrome + [
          nameRegion(
            helpName[0], text: "東京 OpenAI Research Account Alpha Long Name [alpha-tag]", y: 0.9)
        ], false
      ),
      (
        "help-below-close-name",
        helpChrome + [
          nameRegion(
            helpName[0], text: "東京 OpenAI Research Account Alpha Long Name [alpha-tag]", y: 0.1)
        ], false
      ),
      ("help-ambiguous-title", helpChrome + helpName + [helpChrome[0]], false),
      (
        "help-native-japanese-spacing-and-brackets",
        helpChrome + [
          nameRegion(helpName[0], text: "Account 東京OpenAI Research"),
          nameRegion(helpName[1], text: "Account Alpha Long Name ［alpha-tag］"),
        ], true
      ),
      (
        "help-reject-english-ocr-alias",
        helpChrome + [nameRegion(helpName[0], text: "Account *F OpenAI Research"), helpName[1]],
        false
      ),
      (
        "help-reject-missing-japanese-name",
        helpChrome + [nameRegion(helpName[0], text: "Account OpenAI Research"), helpName[1]],
        false
      ),
      (
        "help-reject-wrong-japanese-name",
        helpChrome + [nameRegion(helpName[0], text: "Account 京都OpenAI Research"), helpName[1]],
        false
      ),
      (
        "help-reject-extra-name-prefix",
        helpChrome + [nameRegion(helpName[0], text: "Account Extra 東京 OpenAI Research"), helpName[1]],
        false
      ),
      (
        "help-reject-extra-name-suffix",
        helpChrome + [helpName[0], nameRegion(helpName[1], text: "Account Alpha Long Name [alpha-tag] Extra")],
        false
      ),
      (
        "help-reject-missing-tag-bracket",
        helpChrome + [helpName[0], nameRegion(helpName[1], text: "Account Alpha Long Name alpha-tag］")],
        false
      ),
      (
        "help-reject-ambiguous-field-end",
        helpChrome + helpName + [helpChrome[2]], false
      ),
      ("help-short-body-full-field-before-close", shortHelpChrome + shortHelpName, true),
      (
        "help-short-body-missing-tag",
        shortHelpChrome + Array(shortHelpName.prefix(3)) + [nameRegion(shortHelpName[3], text: "Name")],
        false
      ),
      (
        "help-short-body-extra-trailing-field",
        shortHelpChrome + Array(shortHelpName.prefix(3)) + [nameRegion(shortHelpName[3], text: "Name [alpha-tag] Extra")],
        false
      ),
      (
        "help-short-body-partial-next-section",
        shortHelpChrome + shortHelpName + [nameRegion(shortHelpName[0], text: "Current", y: 0.55)],
        false
      ),
      (
        "help-short-body-adjacent-field",
        shortHelpChrome + shortHelpName + [nameRegion(shortHelpName[0], y: 0.55)], false
      ),
      (
        "help-short-body-tag-without-name",
        shortHelpChrome + [nameRegion(shortHelpName[0], text: "Account [alpha-tag]")], false
      ),
      (
        "help-malformed-region", helpChrome + helpName + [nameRegion(helpName[0], x: -.infinity)],
        false
      ),
    ]
    // Name and the complete tag VALUE remain exact; only the source-defined
    // one-character decoration slots have a separate manual appearance duty.
    let tagValueCases: [(String, String, Bool)] = [
      ("help-reject-ambiguous-leading-identifier-edge", "Account Alpha Long Name Lalpha-tag]", false),
      ("help-reject-ambiguous-trailing-identifier-edge", "Account Alpha Long Name [alpha-tagl", false),
      ("help-reject-ambiguous-both-identifier-edge", "Account Alpha Long Name Lalpha-tagl", false),
      ("help-source-width-decoration-slots", "Account Alpha Long Name \u{ff3b}alpha-tag\u{ff3d}", true),
      ("help-exact-tag-wrong-same-length", "Account Alpha Long Name [alpha-tax]", false),
      ("help-tag-wrong-uppercase-value", "Account Alpha Long Name [ALPHA-TAG]", false),
      ("help-tag-wrong-case-character", "Account Alpha Long Name [Alpha-tag]", false),
      ("help-same-name-other-tag", "Account Alpha Long Name [beta-tag]", false),
      ("help-tag-extra-leading-value", "Account Alpha Long Name [Lalpha-tag]", false),
      ("help-tag-extra-trailing-value", "Account Alpha Long Name [alpha-tagl]", false),
      ("help-tag-extra-leading-decoration", "Account Alpha Long Name [[alpha-tag]", false),
      ("help-tag-extra-trailing-decoration", "Account Alpha Long Name [alpha-tag]]", false),
      ("help-tag-internal-character", "Account Alpha Long Name [alphaXtag]", false),
      ("help-tag-internal-whitespace", "Account Alpha Long Name [alpha- tag]", false),
      ("help-tag-missing-hyphen", "Account Alpha Long Name [alphatag]", false),
      ("help-tag-truncated-value", "Account Alpha Long Name [alpha-ta]", false),
      ("help-tag-missing-value", "Account Alpha Long Name []", false),
      ("help-tag-missing-leading-slot", "Account Alpha Long Name alpha-tag]", false),
      ("help-tag-missing-trailing-slot", "Account Alpha Long Name [alpha-tag", false),
      ("help-tag-missing-both-slots", "Account Alpha Long Name alpha-tag", false),
      ("help-tag-unknown-leading-slot", "Account Alpha Long Name Xalpha-tag]", false),
      ("help-tag-unknown-trailing-slot", "Account Alpha Long Name [alpha-tagX", false),
      ("help-name-edge-character-not-discarded", "Account Alpha Long NameX [alpha-tag]", false),
      ("help-name-truncated-edge", "Account Alpha Long Nam [alpha-tag]", false),
      ("help-name-wrong-internal-character", "Account Alpha Long NaXe [alpha-tag]", false),
      ("help-duplicate-tag-field", "Account Alpha Long Name [alpha-tag] [alpha-tag]", false),
      ("help-tag-after-other-field", "Account Alpha Long Name Other [alpha-tag]", false),
      ("help-tag-value-only", "[alpha-tag]", false),
    ]
    for (name, suffix, expected) in tagValueCases {
      let regions = helpChrome + [helpName[0], nameRegion(helpName[1], text: suffix)]
      let result = observe(checkpoint: .componentQuotaConfirmHelpIdentity,
        recognizedLines: regions.map(\.text), regions: regions)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != expected {
        failures.append(name)
      }
    }
    for (name, regions, shouldPass) in helpCases {
      let result = observe(
        checkpoint: .componentQuotaConfirmHelpIdentity,
        recognizedLines: regions.map(\.text), regions: regions)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    let helpTopCases: [(String, [ScreenTextRegion], Bool)] = [
      (
        "help-selected-section-top",
        helpChrome + [
          ScreenTextRegion(
            text: "Selected account", bounds: CGRect(x: 0.10, y: 0.7, width: 0.5, height: 0.04))
        ] + helpName, true
      ),
      ("help-top-not-just-name", helpChrome + helpName, false),
      (
        "help-top-needs-field-label",
        helpChrome + [
          ScreenTextRegion(
            text: "Selected account", bounds: CGRect(x: 0.10, y: 0.7, width: 0.5, height: 0.04))
        ], false
      ),
      (
        "help-top-background-section",
        helpChrome + [
          ScreenTextRegion(
            text: "Selected account Account",
            bounds: CGRect(x: 0.10, y: 0.9, width: 0.7, height: 0.04))
        ] + helpName, false
      ),
    ]
    for (name, regions, shouldPass) in helpTopCases {
      let result = observe(
        checkpoint: .componentQuotaConfirmHelpTop,
        recognizedLines: regions.map(\.text), regions: regions)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    // Retained original32 native boxes, with wrong-position controls at the
    // measured title/body boundary. These are geometry controls, not pixels.
    let measuredHelpTitle = ScreenTextRegion(
      text: "Quota Help",
      bounds: CGRect(x: 0.0970082615, y: 0.6606248133, width: 0.3088406154, height: 0.0423867368))
    let measuredHelpClose = ScreenTextRegion(
      text: "Close",
      bounds: CGRect(x: 0.0971428585, y: 0.2520661158, width: 0.16, height: 0.0330578512))
    let measuredSelected = ScreenTextRegion(
      text: "Selected account",
      bounds: CGRect(x: 0.102857135, y: 0.6280991742, width: 0.4857142857, height: 0.0371900826))
    let measuredAccount = ScreenTextRegion(
      text: "Account",
      bounds: CGRect(x: 0.0971428585, y: 0.5826446283, width: 0.2171428571, height: 0.0371900826))
    let measuredChrome = [measuredHelpTitle, measuredHelpClose, measuredAccount]
    let helpRowCases: [(String, [ScreenTextRegion], Bool)] = [
      ("help-actual-adjacent-row-boxes", measuredChrome + [measuredSelected], true),
      (
        "help-reject-title-same-row",
        measuredChrome + [nameRegion(measuredSelected, y: measuredHelpTitle.bounds.midY - measuredSelected.bounds.height / 2)],
        false
      ),
      (
        "help-reject-title-majority-overlap",
        measuredChrome + [nameRegion(measuredSelected, y: measuredHelpTitle.bounds.minY - 0.01)],
        false
      ),
      (
        "help-reject-exact-half-title-overlap",
        measuredChrome + [nameRegion(measuredSelected, y: measuredHelpTitle.bounds.minY - measuredSelected.bounds.height / 2)],
        false
      ),
      (
        "help-reject-inside-minimum-center-separation",
        measuredChrome + [nameRegion(measuredSelected, y: measuredHelpTitle.bounds.minY - measuredSelected.bounds.height / 2 + 0.00000001)],
        false
      ),
      (
        "help-reject-above-title-row",
        measuredChrome + [nameRegion(measuredSelected, y: 0.75)], false
      ),
      (
        "help-reject-close-same-row",
        measuredChrome + [nameRegion(measuredSelected, y: measuredHelpClose.bounds.midY - measuredSelected.bounds.height / 2)],
        false
      ),
      (
        "help-reject-below-close-row",
        measuredChrome + [nameRegion(measuredSelected, y: 0.15)], false
      ),
      (
        "help-reject-ambiguous-close",
        measuredChrome + [measuredHelpClose, measuredSelected], false
      ),
      (
        "help-reject-malformed-row-box",
        measuredChrome + [nameRegion(measuredSelected, y: .nan)], false
      ),
    ]
    for (name, regions, shouldPass) in helpRowCases {
      let result = observe(
        checkpoint: .componentQuotaConfirmHelpTop,
        recognizedLines: regions.map(\.text), regions: regions)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    let decisionRegions = [
      ScreenTextRegion(
        text: "Confirm reset", bounds: CGRect(x: 0.10, y: 0.8, width: 0.4, height: 0.04)),
      ScreenTextRegion(
        text: "東京 OpenAI [alpha-tag]", bounds: CGRect(x: 0.10, y: 0.7, width: 0.6, height: 0.04)),
      ScreenTextRegion(
        text: "1 credit. Cannot undo.", bounds: CGRect(x: 0.10, y: 0.6, width: 0.6, height: 0.04)),
      ScreenTextRegion(
        text: "Reset quota? No Yes", bounds: CGRect(x: 0.10, y: 0.5, width: 0.6, height: 0.04)),
    ]
    let returnCases: [(String, String?, Bool)] = [
      ("help-return-preserves-yes", "yes", true),
      ("help-return-reset-to-no", "no", false),
      ("help-return-missing-selected-color", nil, false),
    ]
    for (name, selected, shouldPass) in returnCases {
      let result = observe(
        checkpoint: .componentQuotaConfirmHelpReturned,
        recognizedLines: decisionRegions.map(\.text) + ["Cancel Confirm"],
        regions: decisionRegions, selectedDecision: selected)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    let foreignFacts = observe(checkpoint: .componentQuotaBrowseCreditWindow,
      recognizedLines: ["Quota status", "62.5k 5h 99%"],
      selectedAccountName: "Claude Research Account Beta Full Name")
    if !foreignFacts.missingMarkers.contains("captured Alpha account; foreign selected Beta") {
      failures.append("foreign-selected-beta-cannot-borrow-alpha-facts")
    }
    let foreignWindow = observe(checkpoint: .componentQuotaBrowsePaceHeading,
      recognizedLines: ["Quota status", "5h 99% Weekly pace"],
      selectedAccountName: "Claude Research Account Beta Full Name")
    if !foreignWindow.missingMarkers.contains("captured Alpha account; foreign selected Beta") {
      failures.append("foreign-selected-beta-cannot-borrow-alpha-short-window")
    }
    let attributionCases: [(Bool?, String?, Bool)] = [
      (true, nil, false), (nil, nil, false), (true, "Unknown", false),
      (true, "*F OpenAI Research Account Alpha Long Name", false),
      (true, "東京 OpenAI Research Account Alpha Long Name", true), (false, nil, true),
    ]
    for (visibility, name, expected) in attributionCases {
      let result = observe(checkpoint: .componentQuotaBrowseCreditWindow,
        recognizedLines: ["Quota status", "62.5k 5h 99%"],
        selectedAccountName: name, selectedRowVisible: visibility)
      if result.missingMarkers.contains("visible selected account is missing or ambiguous") == expected {
        failures.append("visible-account-attribution-must-fail-closed")
      }
    }
    return (
      nameCases.count + alphaCases.count + fortyCases.count + fortyEightCases.count + wordContentCases.count + 1 + malformedWordBoxes.count + tagValueCases.count + helpCases.count + helpTopCases.count + helpRowCases.count + returnCases.count + 2 + attributionCases.count,
      failures)
  }
}
