import Foundation

// Scopes captured Sessions fields to the active dialog and its fixed identity.
// These helpers never read a fixture, synthesize OCR, or borrow background text.
extension VerifyScreenImage {
  static func sessionsDialogRegions(
    _ regions: [ScreenTextRegion], title: String,
    action: String
  ) -> [ScreenTextRegion] {
    let headings = regions.filter { normalizedMarkerText($0.text).hasPrefix(title) }
    let actions = regions.filter {
      normalizedMarkerText($0.text).split(separator: " ").contains(Substring(action))
    }
    guard headings.count == 1, actions.count == 1,
      let heading = headings.first, let footer = actions.first,
      heading.bounds.minY > footer.bounds.maxY
    else { return [] }
    // One cell of tolerance: a native field-label box can start left of its
    // dialog heading (New session: heading x.1500, Machine.1484);
    // background session columns sit several cells outside the dialog.
    let cell = heading.bounds.width / Double(max(1, heading.text.count))
    return regions.filter {
      $0.bounds.maxY <= heading.bounds.minY + 0.0000001
        && $0.bounds.minY >= footer.bounds.maxY - 0.0000001
        && $0.bounds.minX >= heading.bounds.minX - cell
    }
  }

  static func sessionsChoiceQueryPairs(
    _ regions: [ScreenTextRegion], title: String,
    field: String, expectedQuery: String = "zznomatchzz", noMatch: Bool = true
  ) -> [String] {
    let body = sessionsDialogRegions(regions, title: title, action: "select")
    guard regionsHaveValidBounds(body) else { return [] }
    let labels = body.filter {
      let text = normalizedMarkerText($0.text)
      return text == field || text == field + ":" || text.hasPrefix(field + " search:")
    }
    guard labels.count == 1, let label = labels.first else { return [] }
    let queries = body.filter { normalizedMarkerText($0.text).contains("search:") }
    let nearby = queries.filter { query in
      let overlap =
        min(query.bounds.maxY, label.bounds.maxY)
        - max(query.bounds.minY, label.bounds.minY)
      return
        (overlap >= min(query.bounds.height, label.bounds.height) / 2
        && query.bounds.minX >= label.bounds.minX)
        // Distinct adjacent native rows can have touching OCR boxes. Use the
        // same half-taller-height centre rule as the no-match row below.
        || (label.bounds.midY - query.bounds.midY >= max(query.bounds.height, label.bounds.height) / 2
          && label.bounds.minY - query.bounds.maxY <= label.bounds.height * 1.5
          && query.bounds.minX >= label.bounds.minX)
    }.sorted {
      if abs($0.bounds.midY - $1.bounds.midY) > 0.001 {
        return $0.bounds.midY > $1.bounds.midY
      }
      return $0.bounds.minX < $1.bounds.minX
    }
    // C7 renders an end-position caret (U+258F) after the query. Vision may
    // drop it or read it as one bar-like glyph; the exact caret is required
    // by the tape's terminal-text wait, so this oracle allows at most one.
    guard let query = nearby.first else { return [] }
    let queryText = normalizedMarkerText(query.text)
    let inlineQuery = queryText.hasSuffix("search: " + expectedQuery)
        || queryText.range(
          of: "search: " + NSRegularExpression.escapedPattern(for: expectedQuery) + "[▏|li!]$",
          options: .regularExpression) != nil
    let valueRow: ScreenTextRegion
    if inlineQuery {
      valueRow = query
    } else {
      // At 32 columns SearchSelect wraps its query beneath the Search label.
      // Keep both original observations: the exact value must be the single
      // nearest lower row, aligned with Search, before the no-match row.
      guard queryText == "search:",
        let rows = terminalRows(body.filter {
          query.bounds.midY - $0.bounds.midY
            >= max(query.bounds.height, $0.bounds.height) / 2
        }), let first = rows.first, first.count == 1, let value = first.first,
        abs(value.bounds.minX - query.bounds.minX)
          <= query.bounds.width / Double("Search:".count),
        query.bounds.minY - value.bounds.maxY <= max(query.bounds.height, value.bounds.height),
        query.bounds.minY - value.bounds.maxY >= -min(query.bounds.height, value.bounds.height) / 2,
        normalizedMarkerText(value.text).range(
          of: "^" + NSRegularExpression.escapedPattern(for: expectedQuery) + "[▏|li!]?$",
          options: .regularExpression) != nil
      else { return [] }
      valueRow = value
    }
    guard
      nearby.dropFirst().allSatisfy({ $0.bounds != query.bounds }),
      !noMatch
        || body.contains(where: {
          // The nested no-match row sits directly below the query; Vision's
          // adjacent-row boxes may touch, so require a distinct lower row by
          // centre (half the taller row) within two query heights.
          normalizedMarkerText($0.text) == "no matching choices"
            && valueRow.bounds.midY - $0.bounds.midY
              >= max(valueRow.bounds.height, $0.bounds.height) / 2
            && valueRow.bounds.minY - $0.bounds.maxY <= valueRow.bounds.height * 2
            && $0.bounds.minX >= label.bounds.minX
        })
    else { return [] }
    return ["field " + field + " query " + expectedQuery]
  }

  // 2026-10-23 Details layout (picker_detail_view.rs render_structured_details):
  // a fixed context row "<title> … [tag]" (the tag is right-aligned and Vision
  // may box it separately), then a scrolling body of FormFields "Session ID:",
  // "Endpoint:", "State:", "Client:", "Machine:", optional job/peer fields and
  // "Working directory:" (stacked below the label under 52 columns), then the
  // "Original title" section holding the full title and the "Conversation"
  // section. The action row "Close  Copy session ID  Copy path" sits above the
  // hint row "… Esc Close"; the dialog footer is that action row.
  static let sessionsDetailsFieldLabels = [
    "session id", "endpoint", "state", "client", "machine", "job handle", "job state",
    "peer status", "working directory",
  ]
  static let sessionsDetailsSections = ["original title", "conversation"]

  static func sessionsDetailsBody(_ regions: [ScreenTextRegion]) -> [ScreenTextRegion] {
    guard regionsHaveValidBounds(regions) else { return [] }
    let headings = regions.filter { normalizedMarkerText($0.text).hasPrefix("session details") }
    guard headings.count == 1, let heading = headings.first else { return [] }
    // A right-aligned box can extend slightly above its own row (
    // 120x36: tag [000.0011] maxY.6939, heading minY.6934); a body row is one
    // whose centre lies below the heading's bottom edge.
    let below = regions.filter { $0.bounds.midY < heading.bounds.minY }
    // Only the actual action row bounds the dialog; the "… Esc Close" hint
    // never stands in for it.
    let actionRows = below.filter {
      let text = normalizedMarkerText($0.text)
      return text == "close" || text.hasPrefix("close ")
    }
    guard actionRows.count == 1, let footer = actionRows.first else { return [] }
    // The dialog's right edge is its heading row (title plus Help action);
    // background session columns to its right never join a body row.
    let rightEdge = regions.filter { sharesTerminalRow($0, heading) && $0.bounds.minX >= heading.bounds.minX }
      .map(\.bounds.maxX).max() ?? 1
    // One cell of tolerance either side: native text boxes start up to 0.0018
    // left of the heading's box (cua-sessions-calibration-003 wide original),
    // while background columns sit several cells outside the dialog.
    let cell = heading.bounds.width / Double(max(1, heading.text.count))
    // The action row's box can reach into the row above it (76x30:
    // Close maxY.3986, Working directory minY.3952); a body row is one whose
    // centre lies above the action row's top edge.
    return below.filter {
      $0.bounds.midY > footer.bounds.maxY
        && $0.bounds.minX >= heading.bounds.minX - cell
        && $0.bounds.maxX <= rightEdge + cell
    }
  }

  static func sessionsDetailsPairs(_ regions: [ScreenTextRegion]) -> [String] {
    let body = sessionsDetailsBody(regions)
    guard !body.isEmpty, let rows = terminalRows(body) else { return [] }
    let capturedTitle = "claude gamma session · project-a"
    let capturedTag = "[000.0011]"
    // Vision reads the title's "·" as "•" or "-"; only that separator varies.
    func canonical(_ text: String) -> String {
      text.replacingOccurrences(of: #" [·•\-] "#, with: " · ", options: .regularExpression)
    }
    let rowTexts = rows.map { canonical(terminalRowText($0)) }
    // The context row ends with the captured tag. The source clips the title
    // before its reserved tag: a visible prefix binds the context but cannot
    // prove the full title.
    let contexts = rowTexts.indices.filter { index in
      let text = rowTexts[index]
      guard text.hasSuffix(" " + capturedTag) else { return false }
      let visibleTitle = String(text.dropLast(capturedTag.count + 1))
        .trimmingCharacters(in: .whitespaces)
      return visibleTitle.hasPrefix("claude gamma") && capturedTitle.hasPrefix(visibleTitle)
    }
    guard contexts.count == 1, let contextIndex = contexts.first else { return [] }
    var pairs = ["details context claude gamma [000.0011]"]
    if rowTexts[contextIndex] == capturedTitle + " " + capturedTag {
      pairs.append("details title " + capturedTitle)
    }
    let content = Array(rowTexts[(contextIndex + 1)...])
    func fieldLabel(_ text: String) -> String? {
      sessionsDetailsFieldLabels.first { text == $0 || text.hasPrefix($0 + ":") }
    }
    func isBoundary(_ text: String) -> Bool {
      fieldLabel(text) != nil || sessionsDetailsSections.contains(text)
    }
    // A section runs from the row after its start to the next field label or
    // section heading (or the end of the visible body).
    func section(after start: Int) -> String {
      let rest = content[(start + 1)...]
      let end = rest.firstIndex(where: isBoundary) ?? content.endIndex
      return content[(start + 1)..<end].joined(separator: " ")
    }
    let expected = [
      ("working directory", "/repo/project-a", "details path "),
      ("session id", "00000000-0000-4000-8000-000000000011", "details session id "),
      ("endpoint", "claude gamma", "details endpoint "),
      ("state", "idle", "details state "),
    ]
    for (label, value, prefix) in expected {
      let starts = content.indices.filter { fieldLabel(content[$0]) == label }
      guard starts.count == 1, let start = starts.first else { continue }
      let sameRow = String(content[start].dropFirst(label.count))
        .trimmingCharacters(in: .whitespaces)
      let ownValue = sameRow.hasPrefix(":") ? String(sameRow.dropFirst()) : sameRow
      let text = (ownValue + " " + section(after: start)).trimmingCharacters(in: .whitespaces)
      let exact =
        label == "session id" || label == "working directory"
        ? text.replacingOccurrences(of: " ", with: "") == value : text == value
      if exact { pairs.append(prefix + value) }
    }
    let titleStarts = content.indices.filter { content[$0] == "original title" }
    if titleStarts.count == 1, let start = titleStarts.first,
      section(after: start) == capturedTitle
    {
      pairs.append("details title " + capturedTitle)
    }
    return Array(Set(pairs)).sorted()
  }
}
