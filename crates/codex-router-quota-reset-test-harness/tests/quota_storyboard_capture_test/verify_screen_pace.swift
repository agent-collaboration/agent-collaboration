import Foundation

extension VerifyScreenImage {
  // This capture contract is the original 32x24 Menlo16 raster, including VHS
  // chrome. Other viewport rasters keep the existing full-word requirement.
  static func compactPaceFieldMatches(
    words: [ScreenTextRegion], pixelWidth: Int, pixelHeight: Int, selectedName: String?
  ) -> Bool? {
    guard pixelWidth == 350, pixelHeight == 484 else { return nil }
    guard selectedName.map(compactHelpIdentity)
      == compactHelpIdentity("東京 OpenAI Research Account Alpha Long Name"),
      words.allSatisfy({ word in
        let box = word.bounds
        return [box.minX, box.minY, box.width, box.height].allSatisfy(\.isFinite)
          && box.width > 0 && box.height > 0 && box.minX >= 0 && box.minY >= 0
          && box.maxX <= 1 && box.maxY <= 1
      }), let account = uniqueAccountHeader(words) else { return false }
    let headerRow = words.filter {
      abs($0.bounds.midY - account.bounds.midY)
        <= min($0.bounds.height, account.bounds.height) / 2
    }
    let statusHeaders = headerRow.filter { normalizedMarkerText($0.text) == "status" }
    let quotaHeaders = headerRow.filter { normalizedMarkerText($0.text) == "quota" }
    guard statusHeaders.count == 1, quotaHeaders.count == 1,
      let status = statusHeaders.first, let quota = quotaHeaders.first,
      account.bounds.maxX < status.bounds.minX,
      status.bounds.maxX < quota.bounds.minX else { return false }
    // The next account bounds Alpha's facts even when its Status says
    // Credits unknown. A value from that account can never stand in for Alpha.
    let accountLeft = account.bounds.minX - account.bounds.width / 7
    let beta = words.filter {
      normalizedMarkerText($0.text) == "claude"
        && $0.bounds.midY < account.bounds.minY
        && $0.bounds.minX >= accountLeft
        && $0.bounds.maxX < status.bounds.minX
    }
    guard beta.count == 1, let nextAccount = beta.first else { return false }
    let quotaWords = words.filter {
      $0.bounds.midY < quota.bounds.minY && $0.bounds.midY > nextAccount.bounds.maxY
        && $0.bounds.minX > status.bounds.maxX && $0.bounds.midX >= quota.bounds.minX
    }.sorted { left, right in
      if abs(left.bounds.midY - right.bounds.midY)
        <= min(left.bounds.height, right.bounds.height) / 2 {
        return left.bounds.minX < right.bounds.minX
      }
      return left.bounds.midY > right.bounds.midY
    }
    let tokens = quotaWords.map { normalizedMarkerText($0.text) }
    guard tokens.filter({ $0 == "pace" }).count == 1,
      tokens.filter({ $0 == "unknown" }).count == 1,
      let paceIndex = tokens.firstIndex(of: "pace"), paceIndex > 0
    else { return false }
    let labelIndex = paceIndex - 1
    guard labelIndex + 2 < tokens.count,
      Array(tokens[labelIndex...]) == ["weekly", "pace", "unknown"]
    else { return false }
    let field = Array(quotaWords[labelIndex...])
    for (upper, lower) in zip(field, field.dropFirst()) {
      let rowHeight = max(upper.bounds.height, lower.bounds.height)
      guard upper.bounds.midY - lower.bounds.midY >= rowHeight / 2,
        upper.bounds.minY - lower.bounds.maxY <= rowHeight else { return false }
    }
    return true
  }

  static func checkCompactPaceControls() -> (count: Int, failures: [String]) {
    func word(_ text: String, _ x: Double, _ y: Double, _ width: Double = 0.12) -> ScreenTextRegion {
      ScreenTextRegion(text: text, bounds: CGRect(x: x, y: y, width: width, height: 0.025))
    }
    let alpha = "東京 OpenAI Research Account Alpha Long Name"
    let headers = [word("Account", 0.1, 0.85, 0.2), word("Status", 0.45, 0.85),
      word("Quota", 0.7, 0.85)]
    let facts = [word("Weekly", 0.7, 0.55), word("pace", 0.7, 0.51),
      word("Unknown", 0.7, 0.47, 0.2), word("Claude", 0.1, 0.3),
      word("Credits", 0.45, 0.3), word("unknown", 0.45, 0.26)]
    let cases: [(String, [ScreenTextRegion], String?, Int, Int, Bool?)] = [
      ("compact-pace-associated", headers + facts, alpha, 350, 484, true),
      ("compact-pace-real-all-facts", headers + facts + [word("Weekly", 0.7, 0.79),
        word("window", 0.7, 0.75), word("83%", 0.7, 0.71), word("reset", 0.7, 0.67),
        word("7d", 0.7, 0.63), word("5h", 0.7, 0.6), word("99%", 0.7, 0.58)],
        alpha, 350, 484, true),
      ("compact-pace-window-not-pace-label", headers + facts.filter { $0.text != "Weekly" }
        + [word("Weekly", 0.7, 0.79), word("window", 0.7, 0.75)], alpha, 350, 484, false),
      ("compact-pace-account-glyph-bearing", headers + facts.filter { $0.text != "Claude" }
        + [word("Claude", 0.095, 0.3)], alpha, 350, 484, true),
      ("compact-pace-missing-value", headers + facts.filter { $0.text != "Unknown" }, alpha, 350, 484, false),
      ("compact-pace-no-weekly", headers + facts.filter { $0.text != "Weekly" }, alpha, 350, 484, false),
      ("compact-pace-no-pace", headers + facts.filter { $0.text != "pace" }, alpha, 350, 484, false),
      ("compact-pace-status-borrow", headers + facts.filter { $0.text != "Unknown" }
        + [word("Unknown", 0.45, 0.47)], alpha, 350, 484, false),
      ("compact-pace-credits-borrow", headers + facts.filter { $0.text != "Unknown" }, alpha, 350, 484, false),
      ("compact-pace-beta-borrow", headers + facts.filter { $0.text != "Unknown" }
        + [word("Unknown", 0.7, 0.2)], alpha, 350, 484, false),
      ("compact-pace-foreign-selected", headers + facts, "Claude Research Account Beta Full Name", 350, 484, false),
      ("compact-pace-no-selected", headers + facts, nil, 350, 484, false),
      ("compact-pace-ambiguous-value", headers + facts + [word("Unknown", 0.7, 0.43)], alpha, 350, 484, false),
      ("compact-pace-split-word", headers + facts.filter { $0.text != "Unknown" }
        + [word("Un", 0.7, 0.47), word("known", 0.7, 0.43)], alpha, 350, 484, false),
      ("compact-pace-nearest-wrong", headers + facts + [word("Known", 0.7, 0.49)], alpha, 350, 484, false),
      ("compact-pace-reversed", headers + facts.filter { $0.text != "pace" }
        + [word("pace", 0.7, 0.6)], alpha, 350, 484, false),
      ("compact-pace-wrong-column-label", headers + facts.filter { $0.text != "Weekly" }
        + [word("Weekly", 0.45, 0.55)], alpha, 350, 484, false),
      ("compact-pace-header-duplicate", headers + facts + [word("Quota", 0.75, 0.85)], alpha, 350, 484, false),
      ("compact-pace-header-missing", headers.filter { $0.text != "Quota" } + facts, alpha, 350, 484, false),
      ("compact-pace-malformed", headers + facts + [word("invalid", -0.1, 0.5)], alpha, 350, 484, false),
      ("compact-pace-distant-value", headers + facts.filter { $0.text != "Unknown" }
        + [word("Unknown", 0.7, 0.35)], alpha, 350, 484, false),
      ("compact-pace-overlapping-value", headers + facts.filter { $0.text != "Unknown" }
        + [word("Unknown", 0.7, 0.505)], alpha, 350, 484, false),
      ("compact-pace-wide-raster", headers + facts, alpha, 430, 484, nil),
      ("compact-pace-short-raster", headers + facts, alpha, 350, 256, nil),
    ]
    var failures: [String] = []
    for (name, words, selected, width, height, expected) in cases {
      if compactPaceFieldMatches(words: words, pixelWidth: width, pixelHeight: height,
        selectedName: selected) != expected { failures.append(name) }
    }
    for (name, field, expected) in [("compact-pace-observe-associated", true, true),
      ("compact-pace-observe-no-flat-fallback", false, false)] {
      let result = observe(checkpoint: .componentQuotaBrowsePaceValue,
        recognizedLines: ["Quota status", "burn unavailable Unknown"], compactPaceField: field)
      if (!result.missingMarkers.contains("burn unavailable")) != expected { failures.append(name) }
    }
    return (cases.count + 2, failures)
  }
}
