import Foundation

extension VerifyScreenImage {
  // These are native-reader protocol controls, never original-image evidence.
  // C8 keeps the parent field while SearchSelect expands inside its value.
  static func checkSessionsOwnerCorrectionControls() -> (count: Int, failures: [String]) {
    func region(_ text: String, _ x: Double, _ y: Double, _ width: Double = 0.7)
      -> ScreenTextRegion
    {
      ScreenTextRegion(text: text, bounds: CGRect(x: x, y: y, width: width, height: 0.03))
    }
    let horizontal = [
      region("New session Help", 0.1, 0.92),
      region("Machine", 0.1, 0.72, 0.15),
      region("Search: zznomatchzz", 0.4, 0.72, 0.45),
      region("No matching choices", 0.4, 0.68, 0.45),
      region("Cancel Select", 0.1, 0.2),
    ]
    let stacked = [
      region("New session Help", 0.1, 0.92),
      region("Machine", 0.1, 0.76, 0.2),
      region("Search: zznomatchzz", 0.14, 0.72, 0.6),
      region("No matching choices", 0.14, 0.68, 0.6),
      region("Cancel Select", 0.1, 0.2),
    ]
    let noMatchCases: [(String, [ScreenTextRegion], Bool)] = [
      ("c8-query-in-value-column", horizontal, true),
      ("c8-query-in-stacked-value", stacked, true),
      ("c8-query-missing-field", horizontal.filter { $0.text != "Machine" }, false),
      (
        "c8-query-other-field",
        horizontal.map { $0.text == "Machine" ? region("Client", 0.1, 0.72, 0.15) : $0 },
        false
      ),
      (
        "c8-query-background-label",
        horizontal.filter { $0.text != "Machine" } + [region("Machine", 0.1, 0.98, 0.15)],
        false
      ),
      (
        "c8-query-wrong-nearest-query",
        horizontal + [region("Search: another-query", 0.27, 0.72, 0.12)], false
      ),
      (
        "c8-query-result-outside-dialog",
        horizontal.filter { $0.text != "No matching choices" }
          + [region("No matching choices", 0.4, 0.12, 0.45)], false
      ),
    ]
    var failures: [String] = []
    for (name, regions, shouldPass) in noMatchCases {
      let result = observe(
        checkpoint: .compactSessionsNewNoMatch, recognizedLines: regions.map(\.text),
        regions: regions)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    let details = [
      region("Session details Help", 0.1, 0.94),
      region("Claude Gamma session · project-a [000.0011]", 0.1, 0.89),
      region("Title: Claude Gamma session · project-a", 0.1, 0.81),
      region("Client: Claude Code", 0.1, 0.76),
      region("Machine: This machine", 0.1, 0.71),
      region("Session ID: 00000000-0000-4000-8000-000000000011", 0.1, 0.66),
      region("Endpoint: Claude Gamma", 0.1, 0.61),
      region("State: Idle", 0.1, 0.56),
      region("Working directory: /repo/project-a", 0.1, 0.51),
      region("Close Copy session ID Copy path", 0.1, 0.2),
    ]
    let valid = observe(
      checkpoint: .compactSessionsDetails, recognizedLines: details.map(\.text), regions: details)
    let requiredPairs: Set<String> = [
      "details context claude gamma [000.0011]",
      "details title claude gamma session · project-a",
      "details path /repo/project-a",
      "details session id 00000000-0000-4000-8000-000000000011",
      "details endpoint claude gamma", "details state idle",
    ]
    if !requiredPairs.isSubset(of: Set(valid.recognizedFieldPairs)) {
      failures.append("c8-details-full-values-are-scoped-fields")
    }
    for (name, regions) in [
      (
        "c8-details-wrong-captured-tag",
        details.map { item in
          region(
            item.text.replacingOccurrences(of: "[000.0011]", with: "[000.0012]"),
            item.bounds.minX, item.bounds.minY, item.bounds.width)
        }
      ),
      (
        "c8-details-missing-fixed-context",
        details.filter { !$0.text.contains("[000.0011]") }
      ),
      (
        "c8-details-background-context",
        details.filter { !$0.text.contains("[000.0011]") }
          + [region("Claude Gamma session · project-a [000.0011]", 0.1, 0.98)]
      ),
    ] {
      let result = observe(
        checkpoint: .compactSessionsDetails, recognizedLines: regions.map(\.text), regions: regions)
      if result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty {
        failures.append(name)
      }
    }
    for (query, shouldPass) in [("Checkout", true), ("project-a", false)] {
      let fields = [
        region("Session filters Help", 0.1, 0.92),
        region("Folder", 0.1, 0.72, 0.15), region("Search: " + query, 0.4, 0.72, 0.45),
        region("Cancel Select", 0.1, 0.2),
      ]
      let result = observe(
        checkpoint: .compactSessionsFolderChoice,
        recognizedLines: fields.map(\.text), regions: fields)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append("c8-folder-query-" + query)
      }
    }
    return (noMatchCases.count + 6, failures)
  }
}

extension VerifyScreenImage {
  static func currentQueryControlRegions(_ lines: [String]) -> [ScreenTextRegion] {
    var expanded: [String] = []
    var previousLabel = false
    for line in lines {
      let parts = line.components(separatedBy: ": ")
      if parts.count == 2
        && ["Machine", "Client", "Account provider", "Disabled accounts"].contains(parts[0])
      {
        expanded += [parts[0], "Search: " + parts[1]]
        previousLabel = false
      } else if ["Client:", "Machine:", "Account provider:", "Disabled accounts:"].contains(line) {
        expanded.append(String(line.dropLast()))
        previousLabel = true
      } else if previousLabel {
        expanded.append("Search: " + line)
        previousLabel = false
      } else {
        expanded.append(line)
      }
    }
    return expanded.enumerated().map { index, text in
      ScreenTextRegion(
        text: text,
        bounds: CGRect(
          x: text.hasPrefix("Search:") ? 0.14 : 0.1,
          y: 0.95 - Double(index) * 0.06, width: 0.7, height: 0.03))
    }
  }
}

extension VerifyScreenImage {
  static func checkRetainedSessionsControls() -> (count: Int, failures: [String]) {
    func region(_ text: String, _ x: Double, _ y: Double, _ width: Double, _ height: Double)
      -> ScreenTextRegion
    {
      ScreenTextRegion(text: text, bounds: CGRect(x: x, y: y, width: width, height: height))
    }
    var failures: [String] = []
    let sessionsChoiceCases: [(String, ScreenCheckpoint, [String], Bool)] = [
      (
        "sessions-new-machine-query", .compactSessionsNewNoMatch,
        ["New session", "Machine: zznomatchzz", "No matching choices", "Cancel Select"], true
      ),
      (
        "sessions-new-wrong-field", .compactSessionsNewNoMatch,
        ["New session", "Client: zznomatchzz", "No matching choices", "Cancel Select"], false
      ),
      (
        "sessions-new-old-start", .compactSessionsNewNoMatch,
        ["New session", "Machine: zznomatchzz", "No matching choices", "Cancel Start"], false
      ),
      (
        "sessions-new-generic-search", .compactSessionsNewNoMatch,
        ["New session", "Search: zznomatchzz", "No matching choices", "Cancel Select"], false
      ),
      (
        "sessions-client-query", .compactSessionsClientNoMatch,
        ["Session filters", "Client: zznomatchzz", "No matching choices", "Cancel Select"], true
      ),
      (
        "sessions-client-wrapped-query", .compactSessionsClientNoMatch,
        ["Session filters", "Client:", "zznomatchzz", "No matching choices", "Cancel Select"], true
      ),
      (
        "sessions-client-wrong-query", .compactSessionsClientNoMatch,
        ["Session filters", "Client: another", "No matching choices", "Cancel Select"], false
      ),
      (
        "sessions-client-parent-apply", .compactSessionsClientNoMatch,
        ["Session filters", "Client: zznomatchzz", "No matching choices", "Cancel Apply"], false
      ),
      (
        "sessions-fork-destination", .compactSessionsForkChoice,
        ["Fork session", "Destination:", "Cancel Select"], true
      ),
      (
        "sessions-fork-generic-search", .compactSessionsForkChoice,
        ["Fork session", "Search:", "Cancel Select"], false
      ),
      (
        "sessions-machine-field", .compactSessionsMachineSources,
        ["Choose machine", "Machine:", "Cancel Select"], true
      ),
      (
        "sessions-machine-missing-select", .compactSessionsMachineSources,
        ["Choose machine", "Machine:", "Cancel"], false
      ),
    ]
    for (name, checkpoint, lines, shouldPass) in sessionsChoiceCases {
      let queryCase =
        checkpoint == .compactSessionsNewNoMatch || checkpoint == .compactSessionsClientNoMatch
      let regions = queryCase ? currentQueryControlRegions(lines) : []
      let result = observe(
        checkpoint: checkpoint,
        recognizedLines: queryCase ? regions.map(\.text) : lines, regions: regions)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    let sessionsFieldCases: [(String, [ScreenTextRegion], Bool)] = [
      ("sessions-client-same-line", [region("Client Codex", 0.1, 0.7, 0.5, 0.03)], true),
      (
        "sessions-client-split-row",
        [
          region("Client", 0.1, 0.7, 0.1, 0.03),
          region("Codex", 0.3, 0.7, 0.1, 0.03),
        ], true
      ),
      (
        "sessions-client-stacked",
        [
          region("Client", 0.1, 0.7, 0.2, 0.03),
          region("Codex", 0.1, 0.66, 0.2, 0.03),
        ], true
      ),
      (
        "sessions-client-wrong-live-field",
        [
          region("Client Cursor", 0.1, 0.7, 0.5, 0.03),
          region("Draft: Codex", 0.1, 0.8, 0.5, 0.03),
        ], false
      ),
      (
        "sessions-client-background-value",
        [
          region("Client All providers", 0.1, 0.7, 0.5, 0.03),
          region("Codex Alpha", 0.1, 0.95, 0.5, 0.03),
        ], false
      ),
      (
        "sessions-client-nearest-wrong-value",
        [
          region("Client", 0.1, 0.7, 0.1, 0.03),
          region("Cursor", 0.3, 0.7, 0.1, 0.03), region("Codex", 0.6, 0.7, 0.1, 0.03),
        ], false
      ),
    ]
    for (name, fields, shouldPass) in sessionsFieldCases {
      let chrome = ["Session filters", "Help", "Cancel Apply"]
      let result = observe(
        checkpoint: .compactSessionsChooser,
        recognizedLines: chrome + fields.map(\.text), regions: fields)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    return (sessionsChoiceCases.count + sessionsFieldCases.count, failures)
  }
}
