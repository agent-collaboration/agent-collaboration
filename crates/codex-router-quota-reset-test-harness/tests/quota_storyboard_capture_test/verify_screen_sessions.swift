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
    return regions.filter {
      $0.bounds.maxY <= heading.bounds.minY + 0.0000001
        && $0.bounds.minY >= footer.bounds.maxY - 0.0000001
        && $0.bounds.minX >= heading.bounds.minX - 0.0000001
    }
  }

  static func sessionsChoiceQueryPairs(
    _ regions: [ScreenTextRegion], title: String,
    field: String, expectedQuery: String = "zznomatchzz", noMatch: Bool = true
  ) -> [String] {
    let body = sessionsDialogRegions(regions, title: title, action: "select")
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
        || (query.bounds.maxY <= label.bounds.minY
          && label.bounds.minY - query.bounds.maxY <= label.bounds.height * 1.5
          && query.bounds.minX >= label.bounds.minX)
    }.sorted {
      if abs($0.bounds.midY - $1.bounds.midY) > 0.001 {
        return $0.bounds.midY > $1.bounds.midY
      }
      return $0.bounds.minX < $1.bounds.minX
    }
    guard let query = nearby.first,
      normalizedMarkerText(query.text).hasSuffix("search: " + expectedQuery),
      nearby.dropFirst().allSatisfy({ $0.bounds != query.bounds }),
      !noMatch
        || body.contains(where: {
          normalizedMarkerText($0.text) == "no matching choices"
            && $0.bounds.maxY <= query.bounds.minY
            && query.bounds.minY - $0.bounds.maxY <= query.bounds.height * 2
            && $0.bounds.minX >= label.bounds.minX
        })
    else { return [] }
    return ["field " + field + " query " + expectedQuery]
  }

  static func sessionsDetailsPairs(_ regions: [ScreenTextRegion]) -> [String] {
    let body = sessionsDialogRegions(regions, title: "session details", action: "close")
    let capturedTitle = "claude gamma session · project-a"
    let capturedTagSuffix = " [000.0011]"
    let contexts = body.filter {
      let text = normalizedMarkerText($0.text)
      guard text.hasSuffix(capturedTagSuffix) else { return false }
      let visibleTitle = String(text.dropLast(capturedTagSuffix.count))
      // The source clips this label before its separately reserved tag. A
      // visible prefix can bind the context, but cannot prove the full title.
      return visibleTitle.hasPrefix("claude gamma") && capturedTitle.hasPrefix(visibleTitle)
    }
    guard contexts.count == 1, let context = contexts.first else { return [] }
    let content = body.filter { $0.bounds.maxY < context.bounds.minY }
    let labels = [
      "title", "client", "machine", "session id", "endpoint", "state",
      "activity", "model", "working directory",
    ]
    func fieldLabel(_ text: String) -> String? {
      labels.first {
        text == $0 || text == $0 + ":" || text.hasPrefix($0 + ": ") || text.hasPrefix($0 + " ")
      }
    }
    let expected = [
      ("title", "claude gamma session · project-a", "details title "),
      ("working directory", "/repo/project-a", "details path "),
      ("session id", "00000000-0000-4000-8000-000000000011", "details session id "),
      ("endpoint", "claude gamma", "details endpoint "),
      ("state", "idle", "details state "),
    ]
    var pairs = ["details context claude gamma [000.0011]"]
    if normalizedMarkerText(context.text) == capturedTitle + capturedTagSuffix {
      pairs.append("details title claude gamma session · project-a")
    }
    for (label, value, prefix) in expected {
      let starts = content.filter { fieldLabel(normalizedMarkerText($0.text)) == label }
      guard starts.count == 1, let start = starts.first else { continue }
      let boundaries = content.filter {
        $0.bounds.maxY < start.bounds.minY && fieldLabel(normalizedMarkerText($0.text)) != nil
      }
      let lowerEdge = boundaries.map(\.bounds.maxY).max() ?? 0
      let values = content.filter {
        $0.bounds.midY <= start.bounds.midY + 0.0000001
          && $0.bounds.minY > lowerEdge
          && ($0.bounds.minX >= start.bounds.minX || $0.bounds == start.bounds)
      }.sorted {
        if abs($0.bounds.midY - $1.bounds.midY) > min($0.bounds.height, $1.bounds.height) / 2 {
          return $0.bounds.midY > $1.bounds.midY
        }
        return $0.bounds.minX < $1.bounds.minX
      }
      var text = normalizedMarkerText(values.map(\.text).joined(separator: " "))
      if text.hasPrefix(label + ":") {
        text = String(text.dropFirst(label.count + 1))
      } else if text.hasPrefix(label + " ") {
        text = String(text.dropFirst(label.count + 1))
      }
      text = text.trimmingCharacters(in: .whitespacesAndNewlines)
      let exact =
        label == "session id" || label == "working directory"
        ? text.replacingOccurrences(of: " ", with: "") == value : text == value
      if exact { pairs.append(prefix + value) }
    }
    return Array(Set(pairs)).sorted()
  }
}
