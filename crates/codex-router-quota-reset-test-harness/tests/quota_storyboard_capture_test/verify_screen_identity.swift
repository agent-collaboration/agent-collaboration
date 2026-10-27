import Foundation
import Vision

extension OriginalChoicePixels {
  func enabledTextPixels(in box: CGRect) -> Int {
    guard [box.minX, box.minY, box.width, box.height].allSatisfy(\.isFinite),
      box.width > 0, box.height > 0, box.minX >= 0, box.minY >= 0,
      box.maxX <= 1, box.maxY <= 1 else { return 0 }
    let left = Int(floor(box.minX * Double(width)))
    let right = Int(ceil(box.maxX * Double(width)))
    let top = Int(floor((1 - box.maxY) * Double(height)))
    let bottom = Int(ceil((1 - box.minY) * Double(height)))
    var count = 0
    for row in top..<min(bottom, height) {
      for column in left..<min(right, width) {
        let index = row * stride + column * 4
        if bytes[index] > 180 && bytes[index + 1] > 180 && bytes[index + 2] > 180 { count += 1 }
      }
    }
    return count
  }

  func yellowPixels(in box: CGRect) -> Int {
    yellowPixelFootprint(in: box).count
  }

  private func yellowPixelFootprint(in box: CGRect)
    -> (count: Int, columns: Int, rows: Int, boxHeight: Int)
  {
    guard [box.minX, box.minY, box.width, box.height].allSatisfy(\.isFinite),
      box.width > 0, box.height > 0, box.minX >= 0, box.minY >= 0,
      box.maxX <= 1, box.maxY <= 1
    else { return (0, 0, 0, 0) }
    let left = Int(floor(box.minX * Double(width)))
    let right = Int(ceil(box.maxX * Double(width)))
    let top = Int(floor((1 - box.maxY) * Double(height)))
    let bottom = Int(ceil((1 - box.minY) * Double(height)))
    var count = 0
    var columns: Set<Int> = []
    var rows: Set<Int> = []
    for row in top..<min(bottom, height) {
      for column in left..<min(right, width) {
        let index = row * stride + column * 4
        let red = Int(bytes[index])
        let green = Int(bytes[index + 1])
        let blue = Int(bytes[index + 2])
        if red > 150 && green > 150 && blue < 150 && min(red, green) * 2 > blue * 3 {
          count += 1
          columns.insert(column)
          rows.insert(row)
        }
      }
    }
    return (count, columns.count, rows.count, bottom - top)
  }

  // Native word boxes may overlap in blank raster space. The original selected
  // ink, not an arbitrary pixel allowance, decides whether the words separate.
  func selectedWordOverlapIsBlank(_ left: ScreenTextRegion, _ right: ScreenTextRegion) -> Bool {
    let overlap = left.bounds.intersection(right.bounds)
    guard !overlap.isNull, !overlap.isEmpty,
      [overlap.minX, overlap.minY, overlap.width, overlap.height].allSatisfy(\.isFinite),
      overlap.minX >= 0, overlap.minY >= 0, overlap.maxX <= 1, overlap.maxY <= 1
    else { return false }
    return yellowPixels(in: overlap) == 0
  }

  func accountSelectorVisible(in words: [ScreenTextRegion]) -> Bool? {
    guard let header = VerifyScreenImage.uniqueAccountHeader(words) else {
      // Scrolled pose with no Account heading (48x24 Beta extra):
      // measure the same two-cell selector gutter left of the body's content
      // column. The content column is never counted (warning orange passes the
      // yellow test); an unmeasurable gutter stays unknown.
      return VerifyScreenImage.headerlessSelectorGutter(words).map { yellowPixels(in: $0) >= 4 }
    }
    let body = ScreenTextRegion(text: "", bounds: CGRect(x: header.bounds.minX, y: 0,
      width: header.bounds.width, height: header.bounds.minY / 2))
    guard let upperHalf = VerifyScreenImage.accountSelectorBounds(header: header, row: body) else {
      return nil
    }
    let column = CGRect(x: upperHalf.minX, y: 0, width: upperHalf.width,
      height: header.bounds.minY)
    return yellowPixels(in: column) >= 4
  }

  // SelectableRow reserves two terminal cells before every account. Derive the
  // same physical column from its Account header, never from continuation ink.
  func selectedAccountGutter(beside region: ScreenTextRegion, header: ScreenTextRegion) -> Bool {
    guard let gutter = VerifyScreenImage.accountSelectorBounds(header: header, row: region) else {
      return false
    }
    let glyph = yellowPixelFootprint(in: gutter)
    // A continuation's left bearing can enter this fixed column by one pixel
    // (native Beta row). Require the same two-dimensional glyph evidence as
    // the choice focus check, rather than mistaking that ink for a second arrow.
    return yellowPixels(in: region.bounds) >= 8 && glyph.count >= 4
      && glyph.columns >= 2 && glyph.boxHeight >= 4
      && glyph.rows >= max(3, glyph.boxHeight / 3)
  }

  func selectedNameGutter(beside region: ScreenTextRegion) -> Bool {
    let box = region.bounds
    let left = max(0, box.minX - 2 * box.height * Double(height) / Double(width))
    let gutter = CGRect(x: left, y: box.minY, width: box.minX - left, height: box.height)
    return yellowPixels(in: box) >= 8 && yellowPixels(in: gutter) >= 4
  }

}

extension VerifyScreenImage {
  static func uniqueAccountHeader(_ words: [ScreenTextRegion]) -> ScreenTextRegion? {
    let accounts = words.filter { normalizedMarkerText($0.text) == "account" }
    guard let table = accounts.max(by: { $0.bounds.midY < $1.bounds.midY }),
      accounts.filter({ abs($0.bounds.midY - table.bounds.midY)
        <= min($0.bounds.height, table.bounds.height) / 2 }).count == 1 else { return nil }
    return table
  }

  static func accountSelectorBounds(header: ScreenTextRegion, row: ScreenTextRegion) -> CGRect? {
    guard normalizedMarkerText(header.text) == "account" else { return nil }
    let boxes = [header.bounds, row.bounds]
    guard boxes.allSatisfy({ box in
      [box.minX, box.minY, box.width, box.height].allSatisfy(\.isFinite)
        && box.width > 0 && box.height > 0 && box.minX >= 0 && box.minY >= 0
        && box.maxX <= 1 && box.maxY <= 1
    }) else { return nil }
    let cellWidth = header.bounds.width / 7
    let left = header.bounds.minX - 2 * cellWidth
    guard left >= 0, row.bounds.maxY < header.bounds.minY else { return nil }
    return CGRect(x: left, y: row.bounds.minY,
      width: 2 * cellWidth, height: row.bounds.height)
  }

  // Body rows lie between the toolbar ("Disabled" word) and footer ("F4" word).
  // Folded headings or a validated row-start staleness label mark the content
  // column; the selector gutter is the two cells left of it. Arbitrary body
  // words and selector glyphs never set the origin. Missing or disagreeing
  // anchors leave the gutter unmeasurable.
  static func headerlessSelectorGutter(_ words: [ScreenTextRegion]) -> CGRect? {
    guard regionsHaveValidBounds(words) else { return nil }
    let toolbars = words.filter { normalizedMarkerText($0.text) == "disabled" }
    let footers = words.filter { normalizedMarkerText($0.text) == "f4" }
    guard toolbars.count == 1, footers.count == 1, let toolbar = toolbars.first,
      let footer = footers.first, toolbar.bounds.minY > footer.bounds.maxY
    else { return nil }
    let body = words.filter {
      $0.bounds.midY < toolbar.bounds.minY && $0.bounds.midY > footer.bounds.maxY
    }
    // With every heading scrolled away (48x12 Beta extra) the
    // row-start staleness label ("week stale" / "Credits stale") marks the same
    // column: its first word, nothing left of it on its row, "stale" directly
    // after it.
    func isStaleLabel(_ word: ScreenTextRegion) -> Bool {
      guard ["week", "credits"].contains(normalizedMarkerText(word.text)) else { return false }
      let cell = word.bounds.width / Double(word.text.count)
      return !body.contains { sharesTerminalRow($0, word) && $0.bounds.midX < word.bounds.minX }
        && body.contains { next in
          normalizedMarkerText(next.text) == "stale" && sharesTerminalRow(next, word)
            && next.bounds.midX > word.bounds.maxX
            && next.bounds.minX - word.bounds.maxX <= 1.5 * cell
        }
    }
    let headings = body.filter {
      ["quota", "weekly", "extra"].contains(normalizedMarkerText($0.text)) || isStaleLabel($0)
    }
    guard !headings.isEmpty else { return nil }
    let cells = headings.map { $0.bounds.width / Double($0.text.count) }.sorted()
    let cell = cells[cells.count / 2]
    let origins = headings.map(\.bounds.minX)
    guard let origin = origins.min(), let farthest = origins.max(), farthest - origin <= cell,
      origin - 2.5 * cell >= 0
    else { return nil }
    return CGRect(
      x: origin - 2.5 * cell, y: footer.bounds.maxY, width: 2.25 * cell,
      height: toolbar.bounds.minY - footer.bounds.maxY)
  }

  static func selectedFloorDraftValue(
    _ regions: [ScreenTextRegion], selectedText: (ScreenTextRegion) -> Bool
  ) -> String? {
    guard !regions.isEmpty, regions.allSatisfy({ region in
      let box = region.bounds
      return [box.minX, box.minY, box.width, box.height].allSatisfy(\.isFinite)
        && box.width > 0 && box.height > 0 && box.minX >= 0 && box.minY >= 0
        && box.maxX <= 1 && box.maxY <= 1
    }) else { return nil }
    func named(_ value: String) -> [ScreenTextRegion] {
      regions.filter { normalizedMarkerText($0.text) == value }
    }
    let titles = regions.filter {
      ["weekly floor", "weeklyfloor"].contains(normalizedMarkerText($0.text))
    }
    let tags = regions.filter { compactHelpIdentity($0.text).contains("[alpha-tag]") }
    let floors = named("floor"), cancels = named("cancel"), saves = named("save")
    guard titles.count == 1, tags.count == 1, floors.count == 1,
      cancels.count == 1, saves.count == 1,
      let title = titles.first, let tag = tags.first, let floor = floors.first,
      let cancel = cancels.first, let save = saves.first,
      title.bounds.midY > tag.bounds.midY, tag.bounds.midY > floor.bounds.midY,
      floor.bounds.minY > max(cancel.bounds.maxY, save.bounds.maxY)
    else { return nil }
    let values = regions.filter { region in
      let overlap = min(region.bounds.maxY, floor.bounds.maxY)
        - max(region.bounds.minY, floor.bounds.minY)
      return overlap >= min(region.bounds.height, floor.bounds.height) / 2
        && region.bounds.minX > floor.bounds.maxX
        && region.bounds.minX >= title.bounds.minX
        && region.bounds.maxX <= 1 - title.bounds.minX
        && normalizedMarkerText(region.text).range(of: #"^[0-9]+%$"#,
          options: .regularExpression) != nil
    }
    guard values.count == 1, let value = values.first,
      normalizedMarkerText(value.text) == "1%", selectedText(value) else { return nil }
    return "1%"
  }

  static func expandedCreditChoiceMatches(_ regions: [ScreenTextRegion], selected: String?) -> Bool {
    guard selected == "allow", !regions.isEmpty, regions.allSatisfy({ region in
      let box = region.bounds
      return [box.minX, box.minY, box.width, box.height].allSatisfy(\.isFinite)
        && box.width > 0 && box.height > 0 && box.minX >= 0 && box.minY >= 0
        && box.maxX <= 1 && box.maxY <= 1
    }) else { return false }
    func named(_ text: String) -> [ScreenTextRegion] {
      regions.filter { normalizedMarkerText($0.text) == text }
    }
    let titles = regions.filter {
      ["credit usage", "credit usage help"].contains(normalizedMarkerText($0.text))
    }
    let tags = regions.filter { normalizedMarkerText($0.text).contains("[alpha-tag]") }
    let allows = named("allow")
    let cancels = named("cancel"), selects = named("select")
    guard titles.count == 1, tags.count == 1, allows.count == 1,
      cancels.count == 1, selects.count == 1,
      let title = titles.first, let tag = tags.first,
      let allow = allows.first,
      let cancel = cancels.first, let select = selects.first
    else { return false }
    let actionOverlap = min(cancel.bounds.maxY, select.bounds.maxY)
      - max(cancel.bounds.minY, select.bounds.minY)
    guard tag.bounds.midY < title.bounds.minY,
      allow.bounds.midY < tag.bounds.minY,
      allow.bounds.midY > max(cancel.bounds.maxY, select.bounds.maxY),
      actionOverlap >= min(cancel.bounds.height, select.bounds.height) / 2
    else { return false }
    let relevant = [tag, allow, cancel, select]
    guard relevant.allSatisfy({
      $0.bounds.minX >= title.bounds.minX - 0.01
        && $0.bounds.maxX <= 1 - title.bounds.minX + 0.01
    }) else { return false }
    return !regions.contains {
      ["unavailable", "disabled"].contains(normalizedMarkerText($0.text))
        && $0.bounds.midY < tag.bounds.minY
        && $0.bounds.midY > max(cancel.bounds.maxY, select.bounds.maxY)
        && $0.bounds.minX >= title.bounds.minX - 0.01
        && $0.bounds.maxX <= 1 - title.bounds.minX + 0.01
    }
  }

  static func observationFieldIsZero(_ regions: [ScreenTextRegion]) -> Bool {
    guard !regions.isEmpty, regions.allSatisfy({ region in
      let box = region.bounds
      return [box.minX, box.minY, box.width, box.height].allSatisfy(\.isFinite)
        && box.width > 0 && box.height > 0 && box.minX >= 0 && box.minY >= 0
        && box.maxX <= 1 && box.maxY <= 1
    }) else { return false }
    let titles = regions.filter {
      let text = normalizedMarkerText(
        $0.text.replacingOccurrences(of: "[·•]", with: " ", options: .regularExpression))
      return text == "options credits" || text == "options credits help"
    }
    let tags = regions.filter { normalizedMarkerText($0.text).contains("[alpha-tag]") }
    let labels = regions.filter { normalizedMarkerText($0.text) == "observation" }
    let closes = regions.filter { normalizedMarkerText($0.text) == "close" }
    guard titles.count == 1, tags.count == 1, labels.count == 1, closes.count == 1,
      let title = titles.first, let tag = tags.first, let label = labels.first,
      let close = closes.first,
      title.bounds.midY > tag.bounds.midY, tag.bounds.midY > label.bounds.midY,
      label.bounds.midY > close.bounds.midY
    else { return false }
    let body = regions.filter {
      $0.bounds.midY < title.bounds.minY && $0.bounds.midY > close.bounds.maxY
        && $0.bounds.minX >= title.bounds.minX - 0.01
        && $0.bounds.maxX <= 1 - title.bounds.minX + 0.01
    }
    guard body.contains(where: { $0.bounds == label.bounds && $0.text == label.text }),
      body.contains(where: { $0.bounds == tag.bounds && $0.text == tag.text })
    else { return false }
    func sameRow(_ first: CGRect, _ second: CGRect) -> Bool {
      min(first.maxY, second.maxY) - max(first.minY, second.minY)
        >= min(first.height, second.height) / 2
    }
    let right = body.filter {
      $0.bounds.minX >= label.bounds.maxX && sameRow($0.bounds, label.bounds)
    }.sorted { $0.bounds.minX < $1.bounds.minX }
    let below = body.filter {
      $0.bounds.maxY <= label.bounds.minY
        && label.bounds.minY - $0.bounds.maxY <= max(label.bounds.height, $0.bounds.height)
    }.sorted { $0.bounds.maxY > $1.bounds.maxY }
    let words: [ScreenTextRegion]
    if let first = right.first {
      // The real FormField has a14-cell label column; Observation occupies11.
      // Half the measured label width bounds that gap and excludes another column.
      guard first.bounds.minX - label.bounds.maxX <= label.bounds.width / 2 else { return false }
      words = right
    } else if let first = below.first {
      words = below.filter { sameRow($0.bounds, first.bounds) }
        .sorted { $0.bounds.minX < $1.bounds.minX }
      guard let firstWord = words.first,
        abs(firstWord.bounds.minX - label.bounds.minX) <= label.bounds.width / 11 * 1.5
      else { return false }
    } else { return false }
    let characterWidth = label.bounds.width / 11
    for (left, right) in zip(words, words.dropFirst()) {
      guard right.bounds.minX >= left.bounds.maxX,
        right.bounds.minX - left.bounds.maxX <= characterWidth * 2
      else { return false }
    }
    let value = normalizedMarkerText(words.map(\.text).joined(separator: " "))
    // Exactly this measured numeric field may read the font's slashed zero as ø.
    // Preserve raw recognizedLines; no global normalization or loose digit match.
    return value == "checked 0s ago" || value == "checked øs ago"
  }

  static func contextualHelpBody(_ regions: [ScreenTextRegion]) -> String? {
    contextualHelpRegions(regions).map { normalizedMarkerText($0.map(\.text).joined(separator: " ")) }
  }

  static func contextualHelpRegions(_ regions: [ScreenTextRegion]) -> [ScreenTextRegion]? {
    guard !regions.isEmpty,
      regions.allSatisfy({ region in
        let box = region.bounds
        return [box.minX, box.minY, box.width, box.height].allSatisfy(\.isFinite)
          && box.width > 0 && box.height > 0 && box.minX >= 0 && box.minY >= 0
          && box.maxX <= 1 && box.maxY <= 1
      })
    else { return nil }
    let titles = regions.filter { normalizedMarkerText($0.text) == "quota help" }
    let closes = regions.filter { normalizedMarkerText($0.text) == "close" }
    guard titles.count == 1, closes.count == 1,
      let title = titles.first, let close = closes.first,
      title.bounds.minY > close.bounds.maxY
    else { return nil }
    let body = regions.filter {
      let box = $0.bounds
      let titleOverlap = min(box.maxY, title.bounds.maxY) - max(box.minY, title.bounds.minY)
      let closeOverlap = min(box.maxY, close.bounds.maxY) - max(box.minY, close.bounds.minY)
      return box.midY < title.bounds.minY && box.midY > close.bounds.maxY
        && title.bounds.midY - box.midY >= max(title.bounds.height, box.height) / 2
        && box.midY - close.bounds.midY >= max(close.bounds.height, box.height) / 2
        && titleOverlap < min(title.bounds.height, box.height) / 2
        && closeOverlap < min(close.bounds.height, box.height) / 2
        && $0.bounds.minX >= title.bounds.minX - 0.01
        && $0.bounds.maxX <= 1 - title.bounds.minX + 0.01
    }.sorted {
      let overlap = min($0.bounds.maxY, $1.bounds.maxY) - max($0.bounds.minY, $1.bounds.minY)
      if overlap >= min($0.bounds.height, $1.bounds.height) / 2 {
        return $0.bounds.minX < $1.bounds.minX
      }
      return $0.bounds.midY > $1.bounds.midY
    }
    return body
  }

  // Help-body frame from the first pass: its single Quota Help title above its
  // single Close. The region of interest spans those rows to the right edge.
  static func helpBodyFrame(_ regions: [ScreenTextRegion])
    -> (title: ScreenTextRegion, close: ScreenTextRegion)?
  {
    guard regionsHaveValidBounds(regions) else { return nil }
    let titles = regions.filter { normalizedMarkerText($0.text) == "quota help" }
    let closes = regions.filter { normalizedMarkerText($0.text) == "close" }
    guard titles.count == 1, closes.count == 1, let title = titles.first, let close = closes.first,
      title.bounds.minY > close.bounds.maxY
    else { return nil }
    return (title, close)
  }

  static func helpBodyRecognitionBounds(_ regions: [ScreenTextRegion]) -> CGRect? {
    guard let frame = helpBodyFrame(regions) else { return nil }
    let minX = max(0, frame.title.bounds.minX - frame.title.bounds.height)
    return CGRect(
      x: minX, y: frame.close.bounds.minY, width: 1 - minX,
      height: frame.title.bounds.maxY - frame.close.bounds.minY)
  }

  // The second pass alone supplies the help body inside its bounds (rows whose
  // centre lies strictly between the title and Close); the first pass keeps its
  // title, Close and everything outside.
  static func mergingHelpBody(
    firstPass: [ScreenTextRegion], bodyPass: [ScreenTextRegion], bounds: CGRect
  ) -> [ScreenTextRegion] {
    guard let frame = helpBodyFrame(firstPass), regionsHaveValidBounds(bodyPass) else {
      return firstPass
    }
    func insideBody(_ region: ScreenTextRegion) -> Bool {
      region.bounds.midY < frame.title.bounds.minY && region.bounds.midY > frame.close.bounds.maxY
    }
    return firstPass.filter { !(insideBody($0) && $0.bounds.minX >= bounds.minX) }
      + bodyPass.filter(insideBody)
  }

  // Compare domain name/tag values in the same measured Account field. The
  // source paints one decoration character before/after the tag; their actual
  // bracket appearance is a mandatory personal-original check, not OCR proof.
  static func contextualHelpAccountIdentityMatches(
    _ regions: [ScreenTextRegion], capturedIdentity: String
  ) -> Bool {
    guard let regions = contextualHelpRegions(regions) else { return false }
    let body = regions.map(\.text).joined(separator: " ")
    guard let label = body.range(of: #"^(?:selected\s*account\s*)?account\s*"#,
        options: [.regularExpression, .caseInsensitive]),
      let tagStart = capturedIdentity.lastIndex(of: "["), capturedIdentity.hasSuffix("]")
    else { return false }
    let expectedName = compactHelpIdentity(String(capturedIdentity[..<tagStart]))
    let expectedTag = String(
      capturedIdentity[capturedIdentity.index(after: tagStart)..<capturedIdentity.index(before: capturedIdentity.endIndex)])
    guard !expectedName.isEmpty, !expectedTag.isEmpty,
      !expectedTag.contains("["), !expectedTag.contains("]") else { return false }
    let fieldBody = String(body[label.upperBound...])
    guard let sectionPattern = try? NSRegularExpression(pattern: #"current\s*quota"#, options: .caseInsensitive)
    else { return false }
    let nextSections = sectionPattern.matches(in: fieldBody,
      range: NSRange(fieldBody.startIndex..., in: fieldBody))
    guard nextSections.count <= 1 else { return false }
    let fieldEnd: String.Index
    if let next = nextSections.first {
      guard let range = Range(next.range, in: fieldBody) else { return false }
      fieldEnd = range.lowerBound
    } else { fieldEnd = fieldBody.endIndex }
    let identity = String(fieldBody[..<fieldEnd]).trimmingCharacters(in: .whitespacesAndNewlines)
    let decoratedTagLength = expectedTag.count + 2
    guard identity.count > decoratedTagLength else { return false }
    let decoratedTag = identity.suffix(decoratedTagLength)
    guard let leading = decoratedTag.first, let trailing = decoratedTag.last,
      ["[", "［"].contains(String(leading)),
      ["]", "］"].contains(String(trailing)),
      String(decoratedTag.dropFirst().dropLast()) == expectedTag
    else { return false }
    return compactHelpIdentity(String(identity.dropLast(decoratedTagLength))) == expectedName
  }

  static func compactHelpIdentity(_ text: String) -> String {
    normalizedMarkerText(text)
      .replacingOccurrences(of: "［", with: "[")
      .replacingOccurrences(of: "］", with: "]")
      .filter { !$0.isWhitespace }
  }

  // Native Vision word boxes keep neighboring table columns out of a wrapped
  // name. Original observation order remains unchanged in the public result.
  static func recognizedPercentRegions(_ observations: [VNRecognizedTextObservation])
    -> [ScreenTextRegion]
  {
    guard let pattern = try? NSRegularExpression(pattern: #"\b[0-9]+%"#) else { return [] }
    return observations.flatMap { observation -> [ScreenTextRegion] in
      guard let candidate = observation.topCandidates(1).first else { return [] }
      let text = candidate.string
      return pattern.matches(in: text, range: NSRange(text.startIndex..., in: text)).compactMap {
        guard let range = Range($0.range, in: text),
          let box = try? candidate.boundingBox(for: range) else { return nil }
        return ScreenTextRegion(text: String(text[range]), bounds: box.boundingBox)
      }
    }
  }

  // Native enumeration can emit a whitespace placeholder with a zero box.
  // It is not a word; preserve every nonblank payload and validate its bounds
  // at the existing consumer. Primary recognizedLines remain untouched.
  static func recognizedWordContent(_ substring: String?) -> String? {
    guard let substring, substring.contains(where: { !$0.isWhitespace }) else { return nil }
    return substring
  }

  static func recognizedWordRegions(_ observations: [VNRecognizedTextObservation])
    -> [ScreenTextRegion]
  {
    observations.flatMap { observation -> [ScreenTextRegion] in
      guard let candidate = observation.topCandidates(1).first else { return [] }
      let text = candidate.string
      var words: [ScreenTextRegion] = []
      text.enumerateSubstrings(in: text.startIndex..., options: .byWords) { word, range, _, _ in
        guard let word = recognizedWordContent(word),
          let box = try? candidate.boundingBox(for: range) else { return }
        words.append(ScreenTextRegion(text: word, bounds: box.boundingBox))
      }
      return words
    }
  }

  static func selectedAccountName(
    wordRegions: [ScreenTextRegion], pixelWidth: Int,
    hasGutter: (ScreenTextRegion, ScreenTextRegion) -> Bool,
    hasSelectedText: (ScreenTextRegion) -> Bool,
    hasBlankOverlap: (ScreenTextRegion, ScreenTextRegion) -> Bool
  ) -> String? {
    guard pixelWidth > 0, !wordRegions.isEmpty,
      wordRegions.allSatisfy({ region in
        let box = region.bounds
        return [box.minX, box.minY, box.width, box.height].allSatisfy(\.isFinite)
          && box.width > 0 && box.height > 0 && box.minX >= 0 && box.minY >= 0
          && box.maxX <= 1 && box.maxY <= 1
      })
    else { return nil }
    guard let table = uniqueAccountHeader(wordRegions) else { return nil }
    // Wide browse bounds the Account group by the next heading on the same row
    // (Quota left · reset in). Folded browse repeats Account above each block
    // with nothing beside it, so the group spans the row (C1/C5, 2026-10-23).
    let cellWidth = table.bounds.width / Double("Account".count)
    let headerNeighbours = wordRegions.filter {
      abs($0.bounds.midY - table.bounds.midY)
        <= max(table.bounds.height, $0.bounds.height) / 2
        && $0.bounds.minX > table.bounds.maxX
    }
    // A heading is separated from its neighbour by a column gap; an account
    // name's own "Account" word sits one cell from its next word.
    let leftNeighbours = wordRegions.filter {
      abs($0.bounds.midY - table.bounds.midY)
        <= max(table.bounds.height, $0.bounds.height) / 2
        && $0.bounds.maxX <= table.bounds.minX
        && table.bounds.minX - $0.bounds.maxX < 1.5 * cellWidth
    }
    guard leftNeighbours.isEmpty,
      headerNeighbours.allSatisfy({ $0.bounds.minX - table.bounds.maxX >= 1.5 * cellWidth })
    else { return nil }
    let rightBound = headerNeighbours.map(\.bounds.minX).min() ?? 1
    let left = table.bounds.minX - cellWidth
    let column = wordRegions.filter { region in
      guard region.bounds.maxY < table.bounds.minY,
        region.bounds.maxX < rightBound else { return false }
      if region.bounds.minX >= left { return true }
      // The first native CJK box can include its real selector's raster edge.
      // Only selected text beside that physical glyph may cover its two cells.
      guard let gutter = accountSelectorBounds(header: table, row: region) else { return false }
      return region.bounds.minX >= gutter.minX && region.bounds.maxX > table.bounds.minX
        && hasSelectedText(region) && hasGutter(region, table)
    }.sorted {
      if abs($0.bounds.midY - $1.bounds.midY)
        <= min($0.bounds.height, $1.bounds.height) / 2
      {
        return $0.bounds.minX < $1.bounds.minX
      }
      return $0.bounds.midY > $1.bounds.midY
    }
    let anchors = column.indices.filter { index in
      let region = column[index]
      return region.bounds.minX <= table.bounds.minX + table.bounds.width / 7
        && hasSelectedText(region) && hasGutter(region, table)
    }
    guard anchors.count == 1, let start = anchors.first else { return nil }
    var name: [String] = []
    var previous: ScreenTextRegion?
    for region in column[start...] {
      guard hasSelectedText(region) else { break }
      if let previous {
        let overlap =
          min(previous.bounds.maxY, region.bounds.maxY)
          - max(previous.bounds.minY, region.bounds.minY)
        let sameRow = overlap >= min(previous.bounds.height, region.bounds.height) / 2
        if sameRow {
          guard region.bounds.midX - previous.bounds.midX
              >= max(previous.bounds.width, region.bounds.width) / 2,
            region.bounds.minX >= previous.bounds.maxX || hasBlankOverlap(previous, region)
          else { return nil }
        } else {
          // Native glyph boxes on adjacent terminal rows can slightly overlap.
          // Distinct rows already have less than half-height overlap above.
          guard previous.bounds.midY - region.bounds.midY
              >= max(previous.bounds.height, region.bounds.height) / 2,
            previous.bounds.minY - region.bounds.maxY
              <= max(previous.bounds.height, region.bounds.height)
          else { return nil }
        }
      }
      name.append(region.text)
      previous = region
    }
    return name.isEmpty ? nil : name.joined(separator: " ")
  }

  // 2026-10-23 C5 (hard 48x12): one page below the top pose the selected
  // account's name continuation is the top body row, painted selected, directly
  // over its own client line; no Account heading or account name is visible.
  static func selectedNameContinuationMatches(
    _ regions: [ScreenTextRegion], continuation: String, clientLine: String,
    hasSelectedText: (ScreenTextRegion) -> Bool
  ) -> Bool {
    guard regionsHaveValidBounds(regions), let rows = terminalRows(regions) else { return false }
    // Vision reads the client line's "·" as "•" or a spaced ":" (
    // 48x12 "Claude : 0 clients").
    func canonical(_ text: String) -> String {
      normalizedMarkerText(text).replacingOccurrences(of: "•", with: "·")
        .replacingOccurrences(of: " : ", with: " · ")
    }
    let toolbars = regions.filter { canonical($0.text).contains("disabled: shown") }
    let clients = regions.filter { canonical($0.text).contains(clientLine) }
    // Vision can split the continuation into word boxes on its one terminal row
    // (48x12 "Long" / "Name"); every box must be painted selected.
    let nameRows = rows.filter { canonical(terminalRowText($0)) == continuation }
    guard toolbars.count == 1, let toolbar = toolbars.first, clients.count == 1,
      let client = clients.first, nameRows.count == 1, let nameRow = nameRows.first,
      let firstBox = nameRow.first, nameRow.allSatisfy(hasSelectedText)
    else { return false }
    let name = ScreenTextRegion(
      text: continuation,
      bounds: nameRow.dropFirst().reduce(firstBox.bounds) { $0.union($1.bounds) })
    guard name.bounds.midY - client.bounds.midY >= max(name.bounds.height, client.bounds.height) / 2,
      name.bounds.minY - client.bounds.maxY <= max(name.bounds.height, client.bounds.height),
      abs(name.bounds.minX - client.bounds.minX)
        <= 2 * client.bounds.width / Double(max(1, client.text.count))
    else { return false }
    let body = regions.filter { $0.bounds.maxY < toolbar.bounds.minY }
    // Both the continuation and its client line are body rows below the actual
    // toolbar; an aligned pair above it (header area) never qualifies.
    guard name.bounds.maxY < toolbar.bounds.minY, client.bounds.maxY < toolbar.bounds.minY
    else { return false }
    // Nothing between the toolbar and the continuation: it is the top body row.
    guard !body.contains(where: { $0.bounds.minY > name.bounds.maxY && !sharesTerminalRow($0, name) })
    else { return false }
    return !body.contains { region in
      let text = canonical(region.text)
      return text == "account" || text.contains("research account") || windowAccountKey(region) != nil
    }
  }

}


extension VerifyScreenImage {
  static func idleReturnFieldPairs(_ regions: [ScreenTextRegion]) -> [String] {
    guard !regions.isEmpty, regions.allSatisfy({ region in
      let box = region.bounds
      return [box.minX, box.minY, box.width, box.height].allSatisfy(\.isFinite)
        && box.width > 0 && box.height > 0 && box.minX >= 0 && box.minY >= 0
        && box.maxX <= 1 && box.maxY <= 1
    }) else { return [] }
    let titles = regions.filter {
      let title = normalizedMarkerText(
        $0.text.replacingOccurrences(of: "[·•]", with: " ", options: .regularExpression))
      return title == "options resets" || title == "options resets help"
    }
    let tags = regions.filter { normalizedMarkerText($0.text).contains("[alpha-tag]") }
    let closes = regions.filter { normalizedMarkerText($0.text) == "close" }
    let inspections = regions.filter { normalizedMarkerText($0.text) == "inspect" }
    guard titles.count == 1, tags.count == 1, closes.count == 1, inspections.count == 1,
      let title = titles.first, let tag = tags.first,
      let close = closes.first, let inspect = inspections.first,
      // Adjacent row boxes can touch or overlap slightly (40x24 tag
      // maxY = title minY + 2e-10; the 76x30 tag row pass reaches.0041 into
      // the title row); the tag row's centre must lie below the title row.
      title.bounds.minY > tag.bounds.midY,
      min(close.bounds.maxY, inspect.bounds.maxY) - max(close.bounds.minY, inspect.bounds.minY)
        >= min(close.bounds.height, inspect.bounds.height) / 2 else { return [] }
    let body = regions.filter {
      $0.bounds.midY < title.bounds.minY && $0.bounds.midY > close.bounds.maxY
        && $0.bounds.minX >= title.bounds.minX - 0.01
        && $0.bounds.maxX <= 1 - title.bounds.minX + 0.01
    }
    let labels = body.filter {
      let text = normalizedMarkerText($0.text)
      return text == "eligibility" || text.hasPrefix("eligibility ")
    }
    guard labels.count == 1, let label = labels.first,
      // Adjacent terminal rows have overlapping glyph boxes in retained originals.
      // The tag center remains above the whole label, on a distinct row.
      tag.bounds.midY > label.bounds.maxY, !sharesTerminalRow(tag, label),
      body.contains(where: { $0.text == tag.text && $0.bounds == tag.bounds })
    else { return [] }
    return measuredFieldPairs(body).filter { $0 == "eligibility not checked" }
  }

  static func checkIdleReturnMatchingControls() -> (count: Int, failures: [String]) {
    func word(
      _ text: String, _ x: Double, _ y: Double, _ width: Double = 0.22, _ height: Double = 0.03
    ) -> ScreenTextRegion {
      ScreenTextRegion(text: text, bounds: CGRect(x: x, y: y, width: width, height: height))
    }
    let chrome = [word("Options · Resets Help", 0.1, 0.86, 0.55),
      word("東京 [alpha-tag]", 0.1, 0.79, 0.6), word("Close", 0.1, 0.2, 0.1),
      word("Inspect", 0.6, 0.2, 0.15)]
    let label = word("Eligibility", 0.1, 0.65, 0.25)
    let value = word("Not checked", 0.1, 0.60, 0.28)
    let field = [label, value]
    // Retained Normal48x24 full-pass boxes, followed by separate
    // footer word boxes as main projects them before calling this matcher.
    let nativeTitle = word("Options • Resets", 0.0817, 0.6653, 0.3346, 0.0413)
    let nativeTag = word("*F OpenAI Research Acco... [alpha-tag]", 0.0817, 0.6198, 0.7860, 0.0455)
    let nativeLabel = word("Eligibility", 0.0814, 0.5768, 0.2340, 0.0488)
    let nativeValue = word("Not checked", 0.0817, 0.5413, 0.2335, 0.0372)
    let nativeFooter = word("Close Credits Inspect", 0.0856, 0.2479, 0.4786, 0.0413)
    let nativeBody = [nativeTitle, nativeTag, nativeLabel, nativeValue]
    let nativeActions = [word("Close", 0.0856, 0.2479, 0.1, 0.0413),
      word("Inspect", 0.4, 0.2479, 0.16, 0.0413)]
    let cases: [(String, [ScreenTextRegion], Bool)] = [
      ("idle-return-stacked", chrome + field, true),
      ("idle-return-inline", chrome + [word("Eligibility Not checked", 0.1, 0.65, 0.6)], true),
      ("idle-return-right", chrome + [label, word("Not checked", 0.45, 0.65, 0.3)], true),
      ("idle-return-empty", chrome, false),
      ("idle-return-wrong-value", chrome + [label, word("Eligible", 0.1, 0.60)], false),
      ("idle-return-wrong-field", chrome + [word("Observation", 0.1, 0.65), value], false),
      ("idle-return-no-label", chrome + [value], false),
      ("idle-return-missing-inspect", chrome.filter { $0.text != "Inspect" } + field, false),
      ("idle-return-missing-close", chrome.filter { $0.text != "Close" } + field, false),
      ("idle-return-foreign-tag", chrome.filter { !$0.text.contains("alpha-tag") }
        + [word("東京 [beta-tag]", 0.1, 0.79, 0.6)] + field, false),
      ("idle-return-credits-pane", chrome.filter { !$0.text.hasPrefix("Options") }
        + [word("Options · Credits Help", 0.1, 0.86, 0.55)] + field, false),
      ("idle-return-duplicate-label", chrome + field + [word("Eligibility", 0.1, 0.55)], false),
      ("idle-return-background-value", chrome + [label, word("Not checked", 0.01, 0.60, 0.07)], false),
      ("idle-return-below-footer", chrome + [label, word("Not checked", 0.1, 0.1)], false),
      ("idle-return-nearest-wrong", chrome + [label, word("Available", 0.45, 0.65, 0.2),
        word("Not checked", 0.7, 0.65, 0.28)], false),
      ("idle-return-ambiguous-value", chrome + field + [word("Checked", 0.1, 0.60)], false),
      ("idle-return-malformed", chrome + field + [word("invalid", -0.1, 0.5)], false),
      ("idle-return-native-009-projected-footer", nativeBody + [nativeFooter] + nativeActions, true),
      ("idle-return-native-009-split-footer", nativeBody + nativeActions, true),
      ("idle-return-native-009-tag-on-label-row", [nativeTitle, nativeLabel, nativeValue,
        word(nativeTag.text, 0.4, 0.5768, 0.4, 0.0455), nativeFooter] + nativeActions, false),
      ("idle-return-native-009-tag-below-label", [nativeTitle, nativeLabel, nativeValue,
        word(nativeTag.text, 0.0817, 0.50, 0.7860, 0.0455), nativeFooter] + nativeActions, false),
      ("idle-return-merged-footer-no-close", nativeBody + [
        word("Credits Inspect", 0.0856, 0.2479, 0.4786, 0.0413),
        word("Inspect", 0.4, 0.2479, 0.16, 0.0413),
      ], false),
      ("idle-return-merged-footer-no-inspect", nativeBody + [
        word("Close Credits", 0.0856, 0.2479, 0.4786, 0.0413),
        word("Close", 0.0856, 0.2479, 0.1, 0.0413),
      ], false),
      ("idle-return-merged-footer-duplicate-close", nativeBody + [nativeFooter,
        word("Close", 0.0856, 0.2, 0.1, 0.0413)] + nativeActions, false),
      ("idle-return-merged-footer-duplicate-inspect", nativeBody + [nativeFooter,
        word("Inspect", 0.4, 0.2, 0.16, 0.0413)] + nativeActions, false),
    ]
    var failures: [String] = []
    for (name, regions, expected) in cases {
      if idleReturnFieldPairs(regions).contains("eligibility not checked") != expected {
        failures.append(name)
      }
    }
    return (cases.count, failures)
  }
}
