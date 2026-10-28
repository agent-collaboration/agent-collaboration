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
    // 2026-10-23 structured Details: fields, then the Original title and
    // Conversation sections; the action row sits above the "… Esc Close" hint.
    let bodyTitleY = 0.44
    let details = [
      region("Session details Help", 0.1, 0.94),
      region("Claude Gamma session · project-a [000.0011]", 0.1, 0.89),
      region("Client actions have not been checked", 0.1, 0.84),
      region("Session ID: 00000000-0000-4000-8000-000000000011", 0.1, 0.79),
      region("Endpoint: Claude Gamma", 0.1, 0.74),
      region("State: Idle", 0.1, 0.69),
      region("Client: Claude Code", 0.1, 0.64),
      region("Machine: This machine", 0.1, 0.59),
      region("Working directory: /repo/project-a", 0.1, 0.54),
      region("Original title", 0.1, 0.49),
      region("Claude Gamma session · project-a", 0.1, bodyTitleY),
      region("Conversation", 0.1, 0.39),
      region("history unavailable", 0.1, 0.34),
      region("Close Copy session ID Copy path", 0.1, 0.25),
      region("←/→ Tab focus · Enter/Space activate · Esc Close", 0.1, 0.2),
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
    func titlePose(_ context: String, bodyTitle: Bool) -> [ScreenTextRegion] {
      details.filter {
        !$0.text.contains("[000.0011]") && (bodyTitle || $0.bounds.minY != bodyTitleY)
      } + [region(context, 0.1, 0.89)]
    }
    let titleCases: [(String, [ScreenTextRegion], Bool)] = [
      ("full-context-without-body-title", titlePose("Claude Gamma session · project-a [000.0011]", bodyTitle: false), true),
      ("extra-title-suffix-without-body", titlePose("Claude Gamma session · project-a EXTRA [000.0011]", bodyTitle: false), false),
      ("extra-title-prefix-without-body", titlePose("EXTRA Claude Gamma session · project-a [000.0011]", bodyTitle: false), false),
      ("short-context-without-body-title", titlePose("Claude Gamma [000.0011]", bodyTitle: false), false),
      ("short-context-with-exact-body-title", titlePose("Claude Gamma [000.0011]", bodyTitle: true), true),
      ("clipped-context-with-exact-body-title", titlePose("Claude Gamma sessi [000.0011]", bodyTitle: true), true),
      ("different-context-with-exact-body-title", titlePose("Claude Gamma session · project-a EXTRA [000.0011]", bodyTitle: true), false),
      ("repeated-context-tag", titlePose("Claude Gamma session · project-a [000.0011] [000.0011]", bodyTitle: false), false),
    ]
    for (name, regions, shouldPass) in titleCases {
      let result = observe(
        checkpoint: .compactSessionsDetailsTitle, recognizedLines: regions.map(\.text), regions: regions)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
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
    let layoutControls = checkSessionsDetailsLayoutControls()
    failures += layoutControls.failures
    return (noMatchCases.count + titleCases.count + 6 + layoutControls.count, failures)
  }

  // Box geometry of the retained cua-sessions-calibration-003 provider-details
  // wide original (label and value columns, a separate right-aligned tag, the
  // action row above the "Esc Close" hint, background columns either side),
  // with the fixture's actual text; plus the stacked layout below 52 columns.
  static func checkSessionsDetailsLayoutControls() -> (count: Int, failures: [String]) {
    func box(_ text: String, _ x: Double, _ y: Double, _ width: Double, _ height: Double)
      -> ScreenTextRegion
    {
      ScreenTextRegion(text: text, bounds: CGRect(x: x, y: y, width: width, height: height))
    }
    let wide = [
      box("Session details", 0.0751, 0.8429, 0.0662, 0.0143),
      box("Help", 0.3989, 0.8429, 0.0215, 0.0143),
      box("Claude Gamma session • project-a", 0.0769, 0.8286, 0.1324, 0.0143),
      box("[000.0011]", 0.3792, 0.8286, 0.0411, 0.0143),
      box("Client actions have not been checked", 0.0751, 0.8057, 0.1485, 0.0143),
      box("Session ID:", 0.0769, 0.7857, 0.0465, 0.0171),
      box("00000000-0000-4000-8000-000000000011", 0.1574, 0.7857, 0.17, 0.0171),
      box("Endpoint:", 0.0751, 0.7714, 0.04, 0.0143),
      box("Claude Gamma", 0.1574, 0.7714, 0.05, 0.0143),
      box("State:", 0.0751, 0.7600, 0.0286, 0.0143),
      box("Idle", 0.1574, 0.7600, 0.0197, 0.0143),
      box("Client:", 0.0769, 0.7457, 0.0304, 0.0171),
      box("Claude Code", 0.1574, 0.7457, 0.0483, 0.0171),
      box("Machine:", 0.0751, 0.7314, 0.035, 0.0143),
      box("This machine", 0.1574, 0.7314, 0.05, 0.0143),
      box("Working directory:", 0.0751, 0.7171, 0.0769, 0.0143),
      box("/repo/project-a", 0.1574, 0.7171, 0.0644, 0.0171),
      box("Original title", 0.0769, 0.7057, 0.0608, 0.0143),
      box("Claude Gamma session - project-a", 0.0733, 0.6914, 0.1360, 0.0143),
      box("Conversation", 0.0769, 0.6771, 0.0519, 0.0171),
      box("Close Copy session ID", 0.0751, 0.6629, 0.0930, 0.0171),
      box("Copy path", 0.1717, 0.6629, 0.0429, 0.0171),
      box("←/→ Tab focus • Enter/Space activate • Esc Close", 0.0769, 0.6486, 0.1986, 0.0171),
      box("Claude-Lo", 0.0322, 0.6943, 0.0394, 0.0114),
      box("ow", 0.4700, 0.7600, 0.0100, 0.0143),
    ]
    let allPairs = [
      "details context claude gamma [000.0011]", "details endpoint claude gamma",
      "details path /repo/project-a",
      "details session id 00000000-0000-4000-8000-000000000011", "details state idle",
      "details title claude gamma session · project-a",
    ]
    func replacing(_ text: String, with replacement: ScreenTextRegion?) -> [ScreenTextRegion] {
      wide.compactMap { $0.text == text ? replacement : $0 }
    }
    let shortContext = replacing(
      "Claude Gamma session • project-a",
      with: box("Claude Gamma session • proj", 0.0769, 0.8286, 0.11, 0.0143))
    // Below 52 columns each value stacks under its label and long values wrap.
    let stacked = [
      box("Session details", 0.05, 0.90, 0.3, 0.03), box("Help", 0.80, 0.90, 0.08, 0.03),
      box("Claude Gamma sess [000.0011]", 0.05, 0.86, 0.8, 0.03),
      box("Session ID:", 0.05, 0.80, 0.2, 0.03),
      box("00000000-0000-4000-8000-", 0.05, 0.76, 0.6, 0.03),
      box("000000000011", 0.05, 0.72, 0.3, 0.03),
      box("Endpoint:", 0.05, 0.68, 0.2, 0.03), box("Claude Gamma", 0.05, 0.64, 0.3, 0.03),
      box("State:", 0.05, 0.60, 0.15, 0.03), box("Idle", 0.05, 0.56, 0.1, 0.03),
      box("Close Copy session ID", 0.05, 0.20, 0.5, 0.03),
      box("Tab/←→ focus • Enter/Space activate", 0.05, 0.16, 0.8, 0.03),
    ]
    let stackedPairs = [
      "details context claude gamma [000.0011]", "details endpoint claude gamma",
      "details session id 00000000-0000-4000-8000-000000000011", "details state idle",
    ]
    // 76x30 native overlap: the Close box's top edge reaches 0.0034
    // into the Working directory row directly above it
    // (root-007-details-path-native.txt), applied to the wide geometry.
    let pathAboveClose = wide.filter {
      !["Original title", "Claude Gamma session - project-a", "Conversation", "Close Copy session ID",
        "Copy path", "←/→ Tab focus • Enter/Space activate • Esc Close"].contains($0.text)
    } + [
      box("Close Copy session ID", 0.0751, 0.6930, 0.0930, 0.0275),
      box("Copy path", 0.1717, 0.6930, 0.0429, 0.0275),
      box("←/→ Tab focus • Enter/Space activate • Esc Close", 0.0769, 0.6750, 0.1986, 0.0171),
    ]
    let cases: [(String, [ScreenTextRegion], [String])] = [
      ("details-wide-native-geometry", wide, allPairs),
      (
        "details-native-tag-above-row-edge",
        replacing("[000.0011]", with: box("[000.0011]", 0.3792, 0.8233, 0.0411, 0.0198)),
        allPairs
      ),
      (
        "details-tag-on-heading-row-not-context",
        replacing("[000.0011]", with: box("[000.0011]", 0.3792, 0.8429, 0.0411, 0.0143)),
        []
      ),
      (
        "details-title-only-from-original-title-section", shortContext, allPairs
      ),
      (
        "details-title-text-under-conversation-is-not-title",
        shortContext.filter { !["Claude Gamma session - project-a", "Conversation"].contains($0.text) }
          + [
            box("Conversation", 0.0769, 0.6914, 0.0519, 0.0143),
            box("Claude Gamma session - project-a", 0.0733, 0.6771, 0.1360, 0.0143),
          ],
        allPairs.filter { !$0.hasPrefix("details title") }
      ),
      (
        "details-short-context-without-section-title",
        shortContext.filter { $0.text != "Claude Gamma session - project-a" },
        allPairs.filter { !$0.hasPrefix("details title") }
      ),
      (
        "details-background-right-value-not-field",
        replacing("Idle", with: box("Idle", 0.4700, 0.7600, 0.0197, 0.0143)).filter { $0.text != "ow" },
        allPairs.filter { $0 != "details state idle" }
      ),
      (
        "details-left-background-title-not-section",
        shortContext.filter { !["Claude Gamma session - project-a", "Claude-Lo"].contains($0.text) }
          + [box("Claude Gamma session - project-a", 0.0322, 0.6914, 0.1360, 0.0143)],
        allPairs.filter { !$0.hasPrefix("details title") }
      ),
      (
        "details-wrong-state-value",
        replacing("Idle", with: box("Running", 0.1574, 0.7600, 0.04, 0.0143)),
        allPairs.filter { $0 != "details state idle" }
      ),
      // The "… Esc Close" hint never stands in for the Close action row.
      (
        "details-missing-close-action-row",
        replacing("Close Copy session ID", with: box("Copy session ID", 0.1126, 0.6629, 0.06, 0.0171)),
        []
      ),
      (
        "details-wrong-action-row",
        replacing("Close Copy session ID", with: box("Cancel Select", 0.0751, 0.6629, 0.06, 0.0171)),
        []
      ),
      (
        "details-two-action-rows-ambiguous",
        wide + [box("Close", 0.0751, 0.6300, 0.03, 0.0143)], []
      ),
      ("details-stacked-wrapped-values", stacked, stackedPairs),
      (
        "details-stacked-value-past-next-label",
        stacked.filter { $0.text != "Idle" }
          + [box("Client:", 0.05, 0.555, 0.15, 0.03), box("Idle", 0.05, 0.52, 0.1, 0.03)],
        stackedPairs.filter { $0 != "details state idle" }
      ),
      ("details-path-row-above-close-edge", pathAboveClose, allPairs),
      (
        "details-path-value-on-close-row",
        pathAboveClose.map {
          $0.text == "/repo/project-a" ? box("/repo/project-a", 0.2300, 0.6930, 0.0644, 0.0275) : $0
        },
        allPairs.filter { $0 != "details path /repo/project-a" }
      ),
      ("details-invalid-geometry", wide + [box("Idle", -0.1, 0.5, 0.1, 0.01)], []),
    ]
    var failures: [String] = []
    for (name, regions, expected) in cases {
      if sessionsDetailsPairs(regions) != expected.sorted() { failures.append(name) }
    }
    return (cases.count, failures)
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
    // Actual 015 Quota 48x24 native boxes: the query and provider label
    // overlap by .0041, but their row centres are .0372 apart.
    let stackedProviderQuery = [
      region("Account filters", 0.0817, 0.6653, 0.3152, 0.0413),
      region("Account provider", 0.0817, 0.6198, 0.3385, 0.0413),
      region("Search: zznomatchzz", 0.1167, 0.5826, 0.4086, 0.0413),
      region("No matching choices", 0.1206, 0.5413, 0.4008, 0.0372),
      region("Disabled accounts", 0.0817, 0.5000, 0.3580, 0.0372),
      region("Shown", 0.1167, 0.4584, 0.1597, 0.0378),
      region("Cancel", 0.0817, 0.2521, 0.1323, 0.0372),
      region("Select", 0.2296, 0.2521, 0.1479, 0.0372),
    ]
    let stackedProviderCases: [(String, [ScreenTextRegion], Bool)] = [
      ("provider-query-native-stacked-overlap", stackedProviderQuery, true),
      (
        "provider-query-stacked-above-label",
        stackedProviderQuery.map {
          $0.text.hasPrefix("Search:")
            ? region($0.text, 0.1167, 0.6653, 0.4086, 0.0413) : $0
        }, false
      ),
      (
        "provider-query-stacked-too-far",
        stackedProviderQuery.map {
          $0.text.hasPrefix("Search:")
            ? region($0.text, 0.1167, 0.4500, 0.4086, 0.0413)
            : $0.text == "No matching choices"
              ? region($0.text, 0.1206, 0.4087, 0.4008, 0.0372) : $0
        }, false
      ),
      (
        "provider-query-stacked-above-label-inside-body",
        stackedProviderQuery.map {
          $0.text == "Account filters"
            ? region($0.text, 0.0817, 0.8000, 0.3152, 0.0413)
            : $0.text.hasPrefix("Search:")
              ? region($0.text, 0.1167, 0.6570, 0.4086, 0.0413) : $0
        }, false
      ),
      (
        "provider-query-stacked-left-of-label",
        stackedProviderQuery.map {
          $0.text.hasPrefix("Search:")
            ? region($0.text, 0.0700, 0.5826, 0.4086, 0.0413) : $0
        }, false
      ),
      (
        "provider-query-same-row-left-of-label",
        stackedProviderQuery.map {
          $0.text.hasPrefix("Search:")
            ? region($0.text, 0.0700, 0.6198, 0.4086, 0.0413) : $0
        }, false
      ),
      (
        "provider-query-stacked-wrong-field",
        stackedProviderQuery.map {
          $0.text == "Account provider"
            ? region("Disabled accounts", 0.0817, 0.6198, 0.3385, 0.0413) : $0
        }, false
      ),
      (
        "provider-query-stacked-short-query",
        stackedProviderQuery.map {
          $0.text.hasPrefix("Search:")
            ? region("Search: zznomatch", 0.1167, 0.5826, 0.4086, 0.0413) : $0
        }, false
      ),
      (
        "provider-query-stacked-extra-character",
        stackedProviderQuery.map {
          $0.text.hasPrefix("Search:")
            ? region("Search: zznomatchzzx", 0.1167, 0.5826, 0.4086, 0.0413) : $0
        }, false
      ),
      (
        "provider-query-stacked-missing-no-match",
        stackedProviderQuery.filter { $0.text != "No matching choices" }, false
      ),
      (
        "provider-query-stacked-no-match-same-row",
        stackedProviderQuery.map {
          $0.text == "No matching choices"
            ? region($0.text, 0.1206, 0.5826, 0.4008, 0.0372) : $0
        }, false
      ),
      (
        "provider-query-stacked-duplicate-observation",
        stackedProviderQuery + [region("Search: zznomatchzz", 0.1167, 0.5826, 0.4086, 0.0413)], false
      ),
    ]
    for (name, regions, shouldPass) in stackedProviderCases {
      let pairs = sessionsChoiceQueryPairs(
        regions, title: "account filters", field: "account provider")
      if (pairs == ["field account provider query zznomatchzz"]) != shouldPass {
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
    return (sessionsChoiceCases.count + stackedProviderCases.count + sessionsFieldCases.count, failures)
  }
}
