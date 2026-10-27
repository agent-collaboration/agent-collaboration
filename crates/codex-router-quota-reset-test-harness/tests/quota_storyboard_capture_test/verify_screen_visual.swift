import Foundation

// Oracles for the 2026-10-23 visual refinement (C2/C6/C9). Each binds actual
// native region geometry: Vision splits one painted terminal row into several
// boxes and enumerates columns separately, so joined recognized text can both
// split a real row and combine values from different rows.
extension VerifyScreenImage {
  static func regionsHaveValidBounds(_ regions: [ScreenTextRegion]) -> Bool {
    regions.allSatisfy { region in
      let box = region.bounds
      return [box.minX, box.minY, box.width, box.height].allSatisfy(\.isFinite)
        && box.width > 0 && box.height > 0 && box.minX >= 0 && box.minY >= 0
        && box.maxX <= 1 && box.maxY <= 1
    }
  }

  static func sharesTerminalRow(_ first: ScreenTextRegion, _ second: ScreenTextRegion) -> Bool {
    min(first.bounds.maxY, second.bounds.maxY) - max(first.bounds.minY, second.bounds.minY)
      >= min(first.bounds.height, second.bounds.height) / 2
  }

  // Native boxes on one terminal row have slightly different vertical centers.
  // Group them into rows (top first, each ordered by x). A box that overlaps
  // two rows is ambiguous and returns nil: a value on another row must never
  // complete this row.
  static func terminalRows(_ regions: [ScreenTextRegion]) -> [[ScreenTextRegion]]? {
    var rows: [[ScreenTextRegion]] = []
    for region in regions.sorted(by: { $0.bounds.midY > $1.bounds.midY }) {
      let matchingRows = rows.indices.filter { index in
        rows[index].allSatisfy { sharesTerminalRow(region, $0) }
      }
      guard matchingRows.count <= 1 else { return nil }
      if let index = matchingRows.first {
        rows[index].append(region)
      } else {
        rows.append([region])
      }
    }
    return rows.map { $0.sorted { $0.bounds.minX < $1.bounds.minX } }
  }

  static func terminalRowText(_ row: [ScreenTextRegion]) -> String {
    normalizedMarkerText(row.map(\.text).joined(separator: " "))
  }

  // 2026-10-23 C6: the actual Pool details dialog is bounded by its title and
  // its single Close action. The browse header keeps a "Pool details" action
  // above the dialog; the dialog title is the nearest such title above Close.
  static func poolDialogFrame(_ regions: [ScreenTextRegion])
    -> (title: ScreenTextRegion, close: ScreenTextRegion)?
  {
    guard regionsHaveValidBounds(regions) else { return nil }
    let closes = regions.filter { normalizedMarkerText($0.text) == "close" }
    let surfaceTitles = regions.filter { normalizedMarkerText($0.text).hasPrefix("quota status") }
    guard closes.count == 1, let close = closes.first,
      let title = regions.filter({ region in
        normalizedMarkerText(region.text).hasPrefix("pool details")
          && region.bounds.minY > close.bounds.maxY
          && !surfaceTitles.contains { sharesTerminalRow(region, $0) }
      }).min(by: { $0.bounds.minY < $1.bounds.minY })
    else { return nil }
    return (title, close)
  }

  static func insidePoolBody(
    _ region: ScreenTextRegion, frame: (title: ScreenTextRegion, close: ScreenTextRegion)
  ) -> Bool {
    region.bounds.maxY <= frame.title.bounds.minY + 0.0000001
      && region.bounds.minY >= frame.close.bounds.maxY - 0.0000001
  }

  // A full-screen pass can drop a lone count after its label: the
  // calibration-004 hard-case original read "Pool unknown:" with no "0" box.
  // The checker re-recognizes the dialog from its title row to its Close row
  // and this rectangle is that region of interest (normalized, lower-left).
  static func poolBodyRecognitionBounds(_ regions: [ScreenTextRegion]) -> CGRect? {
    guard let frame = poolDialogFrame(regions) else { return nil }
    let minX = max(0, frame.title.bounds.minX - frame.title.bounds.height)
    let minY = frame.close.bounds.minY
    let maxY = frame.title.bounds.maxY
    guard maxY > minY, minX < 1 else { return nil }
    return CGRect(x: minX, y: minY, width: 1 - minX, height: maxY - minY)
  }

  // The second pass alone supplies the dialog body inside its region of
  // interest: first-pass body boxes there are dropped, never combined with it,
  // so a count is never borrowed from a different recognition. Second-pass
  // boxes outside the body (its title and Close rows) are discarded; the
  // first pass keeps every box outside that rectangle.
  static func mergingPoolBody(
    firstPass: [ScreenTextRegion], bodyPass: [ScreenTextRegion], bounds: CGRect
  ) -> [ScreenTextRegion] {
    guard let frame = poolDialogFrame(firstPass), regionsHaveValidBounds(bodyPass) else {
      return firstPass
    }
    return firstPass.filter { !(insidePoolBody($0, frame: frame) && $0.bounds.minX >= bounds.minX) }
      + bodyPass.filter { insidePoolBody($0, frame: frame) }
  }

  // C6: bind the census strictly inside that dialog body. Background rows and
  // the header never qualify.
  static func missingPoolCensus(
    checkpoint: ScreenCheckpoint, regions: [ScreenTextRegion]
  ) -> [String] {
    let expected = checkpoint.requiredPoolCensus
    guard !expected.isEmpty else { return [] }
    guard let frame = poolDialogFrame(regions) else { return expected.map { "pool " + $0 } }
    let body = regions.filter { insidePoolBody($0, frame: frame) }
    guard let rows = terminalRows(body) else { return expected.map { "pool " + $0 } }
    func firstCapture(_ pattern: String, in text: String) -> [String] {
      guard let expression = try? NSRegularExpression(pattern: pattern) else { return [] }
      return expression.matches(in: text, range: NSRange(text.startIndex..., in: text))
        .compactMap { match -> String? in
          guard let range = Range(match.range(at: 1), in: text) else { return nil }
          return String(text[range])
        }
    }
    let missing = expected.compactMap { association in
      let parts = association.split(separator: " ")
      let label = NSRegularExpression.escapedPattern(
        for: parts.dropLast().joined(separator: " "))
      let count = String(parts.last ?? "")
      // The dialog renders "Label: N"; OCR may drop or detach the colon. A
      // count Vision splits off belongs to the label only as the label box's
      // immediate right neighbour within two cells; a far-right underlay box on
      // the same terminal row never completes it.
      // Exactly this count position may read the font's slashed zero as ø
      // (hard 40x24 Pool ROI "Pool unknown: Ø"), as the measured
      // observation age does; no other text is normalized. A count is a whole
      // token: no digit or letter after it and no second count beside it.
      func poolCountToken(_ token: String) -> String { token == "ø" ? "0" : token }
      let countToken = "([0-9]+|ø)(?![0-9\\p{L}]|[ \\t]+[0-9ø])"
      var values: [String] = []
      for row in rows {
        for (index, region) in row.enumerated() {
          let text = normalizedMarkerText(region.text)
          values += firstCapture(
            "(?<![a-z0-9])" + label + "[ \\t]*:?[ \\t]+" + countToken, in: text
          ).map(poolCountToken)
          guard
            text.range(of: "(?<![a-z0-9])" + label + "[ \\t]*:?$", options: .regularExpression)
              != nil, index + 1 < row.count
          else { continue }
          let next = row[index + 1]
          let cell = region.bounds.width / Double(max(1, region.text.count))
          let gap = next.bounds.minX - region.bounds.maxX
          if gap >= -cell / 2 && gap <= 2 * cell {
            values += firstCapture("^" + countToken, in: normalizedMarkerText(next.text)).map(poolCountToken)
          }
        }
      }
      return values == [count] ? nil : "pool " + association
    }
    // The hard fixture's credential notice must be a body row of the modal; the
    // browse header's own "Credentials locked" behind it never counts.
    let noticeMissing =
      checkpoint.requiresPoolNotice
      && !rows.contains { terminalRowText($0).contains("credentials locked") }
    return missing + (noticeMissing ? ["pool notice credentials locked"] : [])
  }

  // Fixture order of the visual-hard-cases accounts; window markers name one.
  static let windowAccountOrder = ["alpha", "beta"]

  static let windowAccountNames: [String: [String]] = [
    "alpha": ["東京", "openai", "research", "account", "alpha", "long", "name"],
    "beta": ["claude", "research", "account", "beta", "full", "name"],
  ]

  struct WindowAccountNameBlock {
    let rows: [[ScreenTextRegion]]
  }

  // Secondary native words can locate the first row of a wrapped full name.
  // Only exact complete identities in adjacent Account-column rows qualify;
  // unread CJK, prefix guesses and duplicate complete identities do not.
  static func windowAccountNameBlocks(
    primary: [ScreenTextRegion], nameWords: [ScreenTextRegion]
  ) -> [String: WindowAccountNameBlock] {
    guard !nameWords.isEmpty, regionsHaveValidBounds(nameWords),
      let heading = uniqueAccountHeader(primary) else { return [:] }
    let cell = heading.bounds.width / Double("Account".count)
    let rightBound = primary.filter {
      $0.bounds.minX > heading.bounds.maxX && sharesTerminalRow($0, heading)
    }.map(\.bounds.minX).min() ?? 1
    let column = nameWords.filter {
      $0.bounds.maxY <= heading.bounds.minY + 0.0000001
        && $0.bounds.minX >= heading.bounds.minX - 1.5 * cell
        && $0.bounds.maxX <= rightBound
    }
    guard let rows = terminalRows(column) else { return [:] }
    func adjacent(_ upper: [ScreenTextRegion], _ lower: [ScreenTextRegion]) -> Bool {
      guard let upperBottom = upper.map(\.bounds.minY).min(),
        let lowerTop = lower.map(\.bounds.maxY).max(),
        let height = (upper + lower).map(\.bounds.height).min() else { return false }
      return upperBottom - lowerTop < height
    }
    var blocks: [String: WindowAccountNameBlock] = [:]
    for (account, name) in windowAccountNames {
      var matches: [WindowAccountNameBlock] = []
      for start in rows.indices {
        var words: [String] = []
        var end = start
        while true {
          words += rows[end].map { normalizedMarkerText($0.text) }
          guard words.count < name.count, end + 1 < rows.count,
            adjacent(rows[end], rows[end + 1]) else { break }
          end += 1
        }
        if words == name { matches.append(WindowAccountNameBlock(rows: Array(rows[start...end]))) }
      }
      if matches.count == 1, let match = matches.first { blocks[account] = match }
    }
    return blocks
  }

  static func windowAccountKey(_ region: ScreenTextRegion) -> String? {
    let text = normalizedMarkerText(region.text)
    let keys = windowAccountOrder.filter { key in
      text.range(of: "(?<![a-z0-9-])" + key + "(?![a-z0-9-])", options: .regularExpression)
        != nil
    }
    return keys.count == 1 ? keys.first : nil
  }

  // C2: a window line is "<pct> · <window> · <reset>" (or "No data · …") on one
  // terminal row of the Quota-left column, inside the named account's block.
  // Markers are "window <account> <pct|no data> <window> <reset>"; "—" is the
  // unknown reset. Vision may read "·" as "•", ":", ".", "-" or nothing, and splits the
  // row into several boxes. A missing window carries no meter glyph on its row.
  //
  // Column: below a visible "Quota left" heading, from its left edge to the
  // next heading on that row (the whole width when the grid folds). With no
  // heading visible (a scrolled folded pose) the column is the whole width.
  // Account: the nearest account name at or above the row must be the named
  // account; with none above, the nearest name below must be the next account
  // in fixture order. A pose with no visible name relies on the fixture's
  // account-unique values and the tape's continuity.
  static func missingWindowLines(
    _ expected: [String], regions: [ScreenTextRegion], nameWords: [ScreenTextRegion] = []
  ) -> [String] {
    guard !expected.isEmpty else { return [] }
    guard regionsHaveValidBounds(regions) else { return expected.map { "line " + $0 } }
    let headings = regions.filter { normalizedMarkerText($0.text).hasPrefix("quota left") }
    // "Quota left • reset" + "in" can be two native observations of one
    // heading. Its exact immediate completion is not the next group column.
    let headingText = #"^quota left[\s·•:.\-]{1,6}reset in$"#
    func headingRightEdge(_ heading: ScreenTextRegion) -> Double {
      guard normalizedMarkerText(heading.text).range(of: headingText, options: .regularExpression) == nil,
        let next = regions.filter({
          $0.bounds.minX > heading.bounds.maxX && sharesTerminalRow($0, heading)
        }).min(by: { $0.bounds.minX < $1.bounds.minX }),
        normalizedMarkerText(heading.text + " " + next.text)
          .range(of: headingText, options: .regularExpression) != nil
      else { return heading.bounds.maxX }
      return next.bounds.maxX
    }
    let column =
      headings.isEmpty
      ? regions
      : regions.filter { region in
        headings.contains { heading in
          let headingRight = headingRightEdge(heading)
          let cell = (headingRight - heading.bounds.minX) / Double("Quota left · reset in".count)
          let rightBound =
            regions.filter {
              $0.bounds.minX > headingRight && sharesTerminalRow($0, heading)
            }.map(\.bounds.minX).min() ?? 1
          return region.bounds.maxY <= heading.bounds.minY + 0.0000001
            && region.bounds.minX >= heading.bounds.minX - 1.5 * cell
            && region.bounds.maxX <= rightBound
        }
      }
    guard let rows = terminalRows(column) else { return expected.map { "line " + $0 } }
    let blocks = windowAccountNameBlocks(primary: regions, nameWords: nameWords)
    let names = regions.compactMap { region -> (String, ScreenTextRegion)? in
      guard let key = windowAccountKey(region) else { return nil }
      guard let block = blocks[key],
        block.rows.contains(where: { row in row.contains { sharesTerminalRow($0, region) } }),
        let start = block.rows.first?.first else { return (key, region) }
      return (key, start)
    }
    func rowBelongs(_ row: [ScreenTextRegion], to account: String) -> Bool {
      guard let rowTop = row.map(\.bounds.minY).min(),
        let rowHeight = row.map(\.bounds.height).max(),
        let accountIndex = windowAccountOrder.firstIndex(of: account)
      else { return false }
      let atOrAbove = names.filter { $0.1.bounds.minY >= rowTop - rowHeight / 2 }
      if let nearest = atOrAbove.min(by: { $0.1.bounds.minY < $1.1.bounds.minY }) {
        return nearest.0 == account
      }
      guard let below = names.max(by: { $0.1.bounds.minY < $1.1.bounds.minY }) else {
        return true
      }
      return windowAccountOrder.firstIndex(of: below.0) == accountIndex + 1
    }
    return expected.filter { marker in
      let parts = marker.dropFirst("window ".count).split(separator: " ").map(String.init)
      guard parts.count >= 4 else { return true }
      let account = parts[0]
      let isMissing = parts[1...2] == ["no", "data"]
      let value = isMissing ? "no data" : parts[1]
      let window = isMissing ? parts[3] : parts[2]
      let reset = parts.dropFirst(isMissing ? 4 : 3).joined(separator: " ")
      guard !reset.isEmpty else { return true }
      let separator = #"[\s·•:.\-]{1,6}"#
      let resetPattern =
        reset == "—"
        ? "[—–-]"
        : NSRegularExpression.escapedPattern(for: reset).replacingOccurrences(of: " ", with: #"\s?"#)
      let pattern =
        "(?<![0-9a-z])" + NSRegularExpression.escapedPattern(for: value) + separator
        + NSRegularExpression.escapedPattern(for: window) + separator + resetPattern
        + #"(?![0-9a-z])"#
      return !rows.contains { row in
        let text = terminalRowText(row)
        guard text.range(of: pattern, options: .regularExpression) != nil else { return false }
        if isMissing && text.range(of: "[█░▓▒]", options: .regularExpression) != nil {
          return false
        }
        return rowBelongs(row, to: account)
      }
    }.map { "line " + $0 }
  }

  static let browseCensusLabels = [
    "accounts", "usable", "reserve", "blocked", "pool unknown", "excluded", "fresh", "stale",
    "data unknown",
  ]

  // C6: routine census counts must not return to the browse header. The header
  // is every row above the topmost group heading (Account / Quota left /
  // Weekly pace / Extra), or at and above the filter toolbar's "N/N accounts"
  // row when the headings have scrolled away. Account rows keep their own
  // stale warnings ("week stale", "Credits stale") and are not header census.
  // Each terminal row is judged alone. The retained header client count
  // ("Clients N") and the toolbar's "visible/loaded accounts" are not census.
  static func browseCensusLeaks(_ regions: [ScreenTextRegion]) -> [String] {
    guard regionsHaveValidBounds(regions) else { return ["invalid geometry"] }
    let headings = regions.filter {
      let text = normalizedMarkerText($0.text)
      return ["account", "weekly pace", "extra"].contains(text) || text.hasPrefix("quota left")
    }
    let toolbars = regions.filter {
      normalizedMarkerText($0.text).range(
        of: "(?<![0-9])[0-9]+/[0-9]+ accounts\\b", options: .regularExpression) != nil
    }
    let header: [ScreenTextRegion]
    if let topHeading = headings.max(by: { $0.bounds.minY < $1.bounds.minY }) {
      header = regions.filter { $0.bounds.midY > topHeading.bounds.minY }
    } else if let toolbar = toolbars.max(by: { $0.bounds.minY < $1.bounds.minY }) {
      header = regions.filter { $0.bounds.midY >= toolbar.bounds.minY }
    } else {
      header = regions
    }
    guard let rows = terminalRows(header) else { return ["ambiguous header rows"] }
    let rowTexts = rows.map(terminalRowText)
    return browseCensusLabels.filter { label in
      rowTexts.contains { text in
        text.range(
          of: "(?<![a-z0-9/])" + NSRegularExpression.escapedPattern(for: label)
            + "[ \\t]*:?[ \\t]+[0-9]+(?![0-9/])",
          options: .regularExpression) != nil
      }
    }
  }

  // C9: Enter activates the focused action; only Ctrl(+Shift)+Enter is a fixed
  // Resume/Fork shortcut. Any "Enter Resume" not led by Ctrl is a false label.
  static func labelsEnterAsFixedResume(_ recognizedText: String) -> Bool {
    guard
      let expression = try? NSRegularExpression(
        pattern: #"(ctrl\W{0,3}(?:shift\W{0,3})?)?(?:enter|↵)\W{0,3}resume"#)
    else { return true }
    return expression.matches(
      in: recognizedText, range: NSRange(recognizedText.startIndex..., in: recognizedText)
    ).contains { $0.range(at: 1).location == NSNotFound }
  }
}
