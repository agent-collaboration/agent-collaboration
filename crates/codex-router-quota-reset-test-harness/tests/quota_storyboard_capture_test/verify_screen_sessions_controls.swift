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
          region(item.text.replacingOccurrences(of: "[000.0011]", with: "[000.0012]"),
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
    return (noMatchCases.count + 4, failures)
  }
}
