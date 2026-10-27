import Foundation

// 2026-10-23 C1/C5: Weekly pace is its own group. Wide browse puts it in a
// column under its header; folded browse repeats a "Weekly pace" heading inside
// every account block. The pace value only counts when it sits in the selected
// account's own Weekly pace region, never in another account, column or group.
extension VerifyScreenImage {
  private static func validBoxes(_ words: [ScreenTextRegion]) -> Bool {
    words.allSatisfy { word in
      let box = word.bounds
      return [box.minX, box.minY, box.width, box.height].allSatisfy(\.isFinite)
        && box.width > 0 && box.height > 0 && box.minX >= 0 && box.minY >= 0
        && box.maxX <= 1 && box.maxY <= 1
    }
  }

  private static func sameRow(_ left: ScreenTextRegion, _ right: ScreenTextRegion) -> Bool {
    min(left.bounds.maxY, right.bounds.maxY) - max(left.bounds.minY, right.bounds.minY)
      >= min(left.bounds.height, right.bounds.height) / 2
  }

  // A heading word stands alone on its row: column gaps, not one-cell word gaps.
  private static func standaloneHeadings(
    _ text: String, words: [ScreenTextRegion], cellWidth: Double
  ) -> [ScreenTextRegion] {
    words.filter { candidate in
      normalizedMarkerText(candidate.text) == text
        && !words.contains { other in
          other.bounds != candidate.bounds && sameRow(other, candidate)
            && (abs(other.bounds.minX - candidate.bounds.maxX) < 1.5 * cellWidth
              || abs(candidate.bounds.minX - other.bounds.maxX) < 1.5 * cellWidth)
        }
    }
  }

  // "Weekly pace" headings: the word pair on one row, one cell apart, nothing
  // else within a column gap of the pair.
  private static func weeklyPaceHeadings(
    words: [ScreenTextRegion], cellWidth: Double
  ) -> [(weekly: ScreenTextRegion, pace: ScreenTextRegion)] {
    words.filter { normalizedMarkerText($0.text) == "weekly" }.compactMap { weekly in
      let paces = words.filter {
        normalizedMarkerText($0.text) == "pace" && sameRow($0, weekly)
          && $0.bounds.minX > weekly.bounds.maxX
          && $0.bounds.minX - weekly.bounds.maxX < 1.5 * cellWidth
      }
      guard paces.count == 1, let pace = paces.first else { return nil }
      let crowded = words.contains { other in
        other.bounds != weekly.bounds && other.bounds != pace.bounds && sameRow(other, weekly)
          && (abs(weekly.bounds.minX - other.bounds.maxX) < 1.5 * cellWidth
            || abs(other.bounds.minX - pace.bounds.maxX) < 1.5 * cellWidth)
      }
      return crowded ? nil : (weekly, pace)
    }
  }

  // Exactly one "burn unavailable" in reading order inside the region.
  private static func singleBurnUnavailable(_ region: [ScreenTextRegion]) -> Bool {
    let ordered = region.sorted {
      sameRow($0, $1) ? $0.bounds.minX < $1.bounds.minX : $0.bounds.midY > $1.bounds.midY
    }
    let texts = ordered.map { normalizedMarkerText($0.text) }
    let starts = texts.indices.filter { texts[$0] == "burn" }
    guard starts.count == 1, let start = starts.first, start + 1 < texts.count,
      texts[start + 1] == "unavailable"
    else { return false }
    // One primary phrase; any other "unavailable" must complete the separate
    // secondary "reset pace unavailable" line that wraps below it (
    // 120x36 Alpha: burn unavailable / reset pace / unavailable).
    return texts.indices.allSatisfy { index in
      texts[index] != "unavailable" || index == start + 1
        || (index >= 2 && texts[index - 2] == "reset" && texts[index - 1] == "pace")
    }
  }

  static func selectedWeeklyPaceMatches(
    words: [ScreenTextRegion], selectedName: String?, expectedName: String,
    hasGutter: (ScreenTextRegion, ScreenTextRegion) -> Bool,
    hasSelectedText: (ScreenTextRegion) -> Bool
  ) -> Bool {
    guard !words.isEmpty, validBoxes(words) else { return false }
    let cellWidth: Double
    if let account = words.first(where: { normalizedMarkerText($0.text) == "account" }) {
      cellWidth = account.bounds.width / Double("Account".count)
    } else if let weekly = words.first(where: { normalizedMarkerText($0.text) == "weekly" }) {
      cellWidth = weekly.bounds.width / Double("Weekly".count)
    } else {
      return false
    }
    let headings = standaloneHeadings("account", words: words, cellWidth: cellWidth)
    let paceHeadings = weeklyPaceHeadings(words: words, cellWidth: cellWidth)
    guard let top = headings.max(by: { $0.bounds.midY < $1.bounds.midY }) else {
      // Short-grid continuity: no account start is visible, so the body is the
      // continuation of the same selected account in the same child. Its first
      // Weekly pace heading owns the value.
      guard let pace = paceHeadings.max(by: { $0.weekly.bounds.midY < $1.weekly.bounds.midY })
      else { return false }
      let rowHeight = pace.weekly.bounds.height
      let bottom =
        standaloneHeadings("extra", words: words, cellWidth: cellWidth).filter {
          $0.bounds.maxY < pace.weekly.bounds.minY
        }.map(\.bounds.maxY).max() ?? 0
      return singleBurnUnavailable(words.filter {
        $0.bounds.maxY <= pace.weekly.bounds.minY + rowHeight / 4
          && $0.bounds.minY >= bottom - rowHeight / 4
          && $0.bounds.minX >= pace.weekly.bounds.minX - cellWidth
      })
    }
    let rowHeight = top.bounds.height
    let anchors = words.filter {
      abs($0.bounds.minX - top.bounds.minX) <= cellWidth && $0.bounds.maxY < top.bounds.minY
        && hasSelectedText($0) && hasGutter($0, top)
    }
    // A visible account start without the selected anchor is another account.
    guard anchors.count == 1, let anchor = anchors.first,
      selectedName.map(compactHelpIdentity) == compactHelpIdentity(expectedName)
    else { return false }
    let headerRowPace = paceHeadings.filter { sameRow($0.weekly, top) }
    if headerRowPace.count == 1, let header = headerRowPace.first {
      // Wide: the selected block ends at the two-blank-row account separator.
      guard paceHeadings.count == 1 else { return false }
      let right =
        words.filter { sameRow($0, top) && $0.bounds.minX > header.pace.bounds.maxX }
        .map(\.bounds.minX).min() ?? 1
      let left = header.weekly.bounds.minX - cellWidth / 2
      let below = words.filter { $0.bounds.maxY <= anchor.bounds.maxY + rowHeight / 4 }
        .sorted { $0.bounds.maxY > $1.bounds.maxY }
      var blockBottom = anchor.bounds.minY
      for word in below {
        if blockBottom - word.bounds.maxY >= 2 * rowHeight { break }
        blockBottom = min(blockBottom, word.bounds.minY)
      }
      let region = words.filter {
        $0.bounds.minX >= left && $0.bounds.maxX <= right
          && $0.bounds.minY >= blockBottom - rowHeight / 4
          && $0.bounds.maxY <= anchor.bounds.maxY + rowHeight / 4
      }
      return singleBurnUnavailable(region)
    }
    // Folded: the selected block starts at its anchor.
    let blockTop = anchor.bounds.maxY
    let column = paceHeadings.filter {
      abs($0.weekly.bounds.minX - top.bounds.minX) <= cellWidth
        && $0.weekly.bounds.maxY < blockTop
    }
    guard let pace = column.max(by: { $0.weekly.bounds.midY < $1.weekly.bounds.midY }) else {
      return false
    }
    // No other account may start between the block top and its pace heading.
    let interveningAccount = headings.contains {
      $0.bounds.maxY < anchor.bounds.minY && $0.bounds.midY > pace.weekly.bounds.midY
    }
    guard !interveningAccount else { return false }
    let extras = standaloneHeadings("extra", words: words, cellWidth: cellWidth).filter {
      abs($0.bounds.minX - top.bounds.minX) <= cellWidth && $0.bounds.maxY < pace.weekly.bounds.minY
    }
    let nextAccounts = headings.filter { $0.bounds.maxY < pace.weekly.bounds.minY }
    let bottom = (extras + nextAccounts).map(\.bounds.maxY).max() ?? 0
    let region = words.filter {
      $0.bounds.maxY <= pace.weekly.bounds.minY + rowHeight / 4
        && $0.bounds.minY >= bottom - rowHeight / 4
        && $0.bounds.minX >= top.bounds.minX - cellWidth
    }
    return singleBurnUnavailable(region)
  }

  static func checkSelectedPaceControls() -> (count: Int, failures: [String]) {
    let height = 0.025
    func word(_ text: String, _ x: Double, _ y: Double, _ width: Double = 0.06) -> ScreenTextRegion {
      ScreenTextRegion(text: text, bounds: CGRect(x: x, y: y, width: width, height: height))
    }
    let alpha = "東京 OpenAI Research Account Alpha Long Name"
    let beta = "Claude Research Account Beta Full Name"
    // Wide: one header row; Alpha anchor at 0.80, Beta begins after a gap.
    let wideHeader = [
      word("Account", 0.05, 0.90, 0.07), word("Quota", 0.40, 0.90), word("left", 0.47, 0.90),
      word("Weekly", 0.62, 0.90), word("pace", 0.69, 0.90), word("Extra", 0.80, 0.90),
    ]
    let wideAlpha = [
      word("東京", 0.05, 0.80), word("83%", 0.40, 0.80), word("burn", 0.62, 0.80),
      word("unavailable", 0.67, 0.80, 0.10), word("62.5k", 0.80, 0.80),
      word("reset", 0.62, 0.77), word("pace", 0.67, 0.77),
    ]
    let wideBeta = [
      word("Claude", 0.05, 0.65), word("75%", 0.40, 0.65), word("burn", 0.62, 0.65),
      word("unavailable", 0.67, 0.65, 0.10),
    ]
    // Folded: Alpha block (heading 0.95, anchor 0.90, pace heading 0.70,
    // value 0.66, Extra 0.60), then Beta block from 0.45.
    let foldedAlpha = [
      word("Account", 0.05, 0.95, 0.07), word("東京", 0.05, 0.90),
      word("Weekly", 0.05, 0.70), word("pace", 0.12, 0.70),
      word("burn", 0.05, 0.66), word("unavailable", 0.10, 0.66, 0.10),
      word("Extra", 0.05, 0.60), word("62.5k", 0.05, 0.56),
    ]
    let foldedBeta = [
      word("Account", 0.05, 0.45, 0.07), word("Claude", 0.05, 0.40),
      word("Weekly", 0.05, 0.25), word("pace", 0.12, 0.25),
      word("burn", 0.05, 0.21), word("unavailable", 0.10, 0.21, 0.10),
    ]
    // Short pose: the selected account's start is scrolled away above.
    let continuity = [
      word("99%", 0.05, 0.80), word("5h", 0.10, 0.80),
      word("Weekly", 0.05, 0.70), word("pace", 0.12, 0.70),
      word("burn", 0.05, 0.66), word("unavailable", 0.10, 0.66, 0.10),
    ]
    func without(_ words: [ScreenTextRegion], _ x: Double, _ y: Double) -> [ScreenTextRegion] {
      words.filter { !(abs($0.bounds.minX - x) < 0.001 && abs($0.bounds.minY - y) < 0.001) }
    }
    let cases: [(String, [ScreenTextRegion], String?, Double?, Bool)] = [
      ("pace-wide-associated", wideHeader + wideAlpha + wideBeta, alpha, 0.80, true),
      ("pace-folded-associated", foldedAlpha + foldedBeta, alpha, 0.90, true),
      ("pace-folded-continuity", continuity, nil, nil, true),
      ("pace-wide-foreign-selected", wideHeader + wideAlpha + wideBeta, beta, 0.80, false),
      ("pace-wide-no-selected", wideHeader + wideAlpha + wideBeta, nil, nil, false),
      (
        "pace-wide-only-beta-value",
        wideHeader + without(without(wideAlpha, 0.62, 0.80), 0.67, 0.80) + wideBeta, alpha, 0.80,
        false
      ),
      (
        "pace-wide-value-in-quota-column",
        wideHeader + without(without(wideAlpha, 0.62, 0.80), 0.67, 0.80)
          + [word("burn", 0.40, 0.77), word("unavailable", 0.45, 0.77, 0.10)], alpha, 0.80, false
      ),
      (
        "pace-wide-value-in-extra-column",
        wideHeader + without(without(wideAlpha, 0.62, 0.80), 0.67, 0.80)
          + [word("burn", 0.80, 0.77), word("unavailable", 0.85, 0.77, 0.10)], alpha, 0.80, false
      ),
      (
        "pace-wide-duplicate-value",
        wideHeader + wideAlpha + [word("burn", 0.62, 0.74), word("unavailable", 0.67, 0.74, 0.10)],
        alpha, 0.80, false
      ),
      (
        "pace-wide-native-reset-metadata",
        wideHeader + wideAlpha + [word("unavailable", 0.62, 0.74, 0.10)] + wideBeta, alpha, 0.80, true
      ),
      (
        "pace-wide-bare-extra-unavailable",
        wideHeader + without(without(wideAlpha, 0.62, 0.77), 0.67, 0.77)
          + [word("unavailable", 0.62, 0.74, 0.10)] + wideBeta, alpha, 0.80, false
      ),
      (
        "pace-wide-reset-metadata-without-burn",
        wideHeader + without(without(wideAlpha, 0.62, 0.80), 0.67, 0.80)
          + [word("unavailable", 0.62, 0.74, 0.10)] + wideBeta, alpha, 0.80, false
      ),
      (
        "pace-folded-only-beta-value",
        without(without(foldedAlpha, 0.05, 0.66), 0.10, 0.66) + foldedBeta, alpha, 0.90, false
      ),
      // No second account: only the Extra heading may exclude this value.
      (
        "pace-folded-value-in-extra",
        without(without(foldedAlpha, 0.05, 0.66), 0.10, 0.66)
          + [word("burn", 0.05, 0.52), word("unavailable", 0.10, 0.52, 0.10)],
        alpha, 0.90, false
      ),
      (
        "pace-folded-missing-heading",
        without(without(foldedAlpha, 0.05, 0.70), 0.12, 0.70) + foldedBeta, alpha, 0.90, false
      ),
      (
        "pace-folded-heading-crowded-by-name",
        foldedAlpha.map {
          $0.text == "Weekly" && $0.bounds.minY == 0.70 ? word("Weekly", 0.05, 0.70) : $0
        } + [word("Research", 0.19, 0.70)] + foldedBeta, alpha, 0.90, false
      ),
      ("pace-folded-foreign-selected", foldedAlpha + foldedBeta, beta, 0.90, false),
      (
        "pace-continuity-unselected-account-starts-above-pace",
        [word("Account", 0.05, 0.95, 0.07), word("Claude", 0.05, 0.90)] + continuity, nil, nil,
        false
      ),
      (
        "pace-continuity-value-below-extra",
        without(without(continuity, 0.05, 0.66), 0.10, 0.66)
          + [word("Extra", 0.05, 0.60), word("burn", 0.05, 0.55),
            word("unavailable", 0.10, 0.55, 0.10)], nil, nil, false
      ),
      (
        "pace-split-word",
        without(foldedAlpha, 0.10, 0.66) + [word("unavail", 0.10, 0.66), word("able", 0.16, 0.66)]
          + foldedBeta, alpha, 0.90, false
      ),
      ("pace-malformed", foldedAlpha + [word("burn", -0.1, 0.5)], alpha, 0.90, false),
      ("pace-no-headings", [word("burn", 0.05, 0.66), word("unavailable", 0.1, 0.66)], alpha, 0.66, false),
    ]
    var failures: [String] = []
    for (name, words, selected, anchorY, expected) in cases {
      let result = selectedWeeklyPaceMatches(
        words: words, selectedName: selected, expectedName: alpha,
        hasGutter: { region, _ in anchorY.map { abs(Double(region.bounds.minY) - $0) < 0.001 } ?? false },
        hasSelectedText: { region in
          anchorY.map { abs(Double(region.bounds.minY) - $0) < 0.001 } ?? false
        })
      if result != expected { failures.append(name) }
    }
    for (name, field, expected) in [
      ("pace-observe-associated", true, true), ("pace-observe-no-flat-fallback", false, false),
    ] {
      let result = observe(checkpoint: .componentQuotaBrowsePaceValue,
        recognizedLines: ["Quota status", "burn unavailable"], selectedPaceField: field)
      if (!result.missingMarkers.contains("burn unavailable")) != expected { failures.append(name) }
    }
    return (cases.count + 2, failures)
  }
}
