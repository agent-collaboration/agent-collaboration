import Foundation

// Matching controls for the 2026-10-23 visual refinement (C2/C6/C7/C9): Pool
// details census binding, browse census leaks, window-line association, the
// adjacent no-match row and fixed Enter-Resume labels. Synthetic and retained
// native boxes only; they qualify the oracles, never a screenshot.
extension VerifyScreenImage {
  static func checkVisualRefinementControls() -> (count: Int, failures: [String]) {
    func region(
      _ text: String, _ x: Double, _ y: Double, _ width: Double = 0.2, _ height: Double = 0.0625
    ) -> ScreenTextRegion {
      ScreenTextRegion(text: text, bounds: CGRect(x: x, y: y, width: width, height: height))
    }
    var failures: [String] = []
    // C6: the compact census lives in the Pool details dialog. The browse
    // header keeps its own "Pool details" action above the dialog.
    let poolBackground = [
      region("Quota status", 0.02, 0.95, 0.2, 0.03),
      region("Pool details", 0.6, 0.95, 0.2, 0.03),
    ]
    let poolTitle = region("Pool details", 0.1, 0.86, 0.25, 0.03)
    let poolRows = [
      ("Accounts: 4", 0.80), ("Usable: 0", 0.76), ("Reserve: 0", 0.72), ("Blocked: 0", 0.68),
      ("Pool unknown: 1", 0.64), ("Excluded: 3", 0.60), ("Fresh: 0", 0.56), ("Stale: 0", 0.52),
      ("Data unknown: 4", 0.48), ("Clients: 0", 0.44),
    ].map { region($0.0, 0.1, $0.1, 0.4, 0.03) }
    let poolClose = region("Close", 0.1, 0.30, 0.1, 0.03)
    let poolDialog = poolBackground + [poolTitle] + poolRows + [poolClose]
    func replacingPoolRow(_ index: Int, with replacement: ScreenTextRegion) -> [ScreenTextRegion] {
      poolBackground + [poolTitle]
        + poolRows.enumerated().map { $0.offset == index ? replacement : $0.element } + [poolClose]
    }
    let poolCases: [(String, [ScreenTextRegion], Bool)] = [
      ("pool-complete-census", poolDialog, true),
      ("pool-colon-dropped", replacingPoolRow(0, with: region("Accounts 4", 0.1, 0.80, 0.4, 0.03)), true),
      (
        "pool-split-label-value-same-row",
        replacingPoolRow(4, with: region("Pool unknown:", 0.1, 0.64, 0.3, 0.03))
          + [region("1", 0.42, 0.64, 0.03, 0.03)], true
      ),
      ("pool-wrong-count", replacingPoolRow(5, with: region("Excluded: 30", 0.1, 0.60, 0.4, 0.03)), false),
      ("pool-zero-missing", poolBackground + [poolTitle] + poolRows.filter { $0.text != "Reserve: 0" } + [poolClose], false),
      ("pool-duplicate-pair", poolDialog + [region("Pool unknown: 1", 0.5, 0.40, 0.3, 0.03)], false),
      (
        "pool-value-on-next-row",
        replacingPoolRow(4, with: region("Pool unknown:", 0.1, 0.64, 0.3, 0.03))
          + [region("1", 0.42, 0.62, 0.03, 0.01)], false
      ),
      (
        "pool-census-only-in-background",
        poolBackground + [region("Accounts: 4 Usable: 0", 0.3, 0.95, 0.3, 0.03), poolTitle, poolClose],
        false
      ),
      ("pool-census-below-close", poolBackground + [poolTitle, poolClose] + poolRows.map { region($0.text, 0.1, $0.bounds.minY - 0.42, 0.4, 0.03) }, false),
      ("pool-missing-close", poolBackground + [poolTitle] + poolRows, false),
      ("pool-duplicate-close", poolDialog + [region("Close", 0.6, 0.30, 0.1, 0.03)], false),
      ("pool-account-details-instead", poolBackground + [region("Account details", 0.1, 0.86, 0.3, 0.03)] + poolRows + [poolClose], false),
      ("pool-invalid-geometry", poolDialog + [region("Accounts: 4", -0.1, 0.8)], false),
    ]
    for (name, regions, shouldPass) in poolCases {
      let passed = missingPoolCensus(checkpoint: .compactQuotaPoolDetailsComplete, regions: regions)
        .isEmpty
      if passed != shouldPass { failures.append(name) }
    }
    // Coverage poses: middle rows and the hard in-modal notice (48x12 windows).
    let hardTitle = region("Pool details", 0.1, 0.86, 0.25, 0.03)
    let hardBottom = [("Stale: 1", 0.80), ("Data unknown: 1", 0.76), ("Clients: 1", 0.72), ("Credentials locked", 0.68)]
      .map { region($0.0, 0.1, $0.1, 0.4, 0.03) }
    let headerNotice = region("Credentials locked", 0.02, 0.92, 0.25, 0.03)
    let coverageCases: [(String, ScreenCheckpoint, [ScreenTextRegion], Bool)] = [
      ("pool-middle-full-body", .compactQuotaPoolDetailsMiddle, poolDialog, true),
      (
        "pool-middle-short-window", .compactQuotaPoolDetailsMiddle,
        poolBackground + [poolTitle] + poolRows.filter { ["Reserve: 0", "Blocked: 0", "Pool unknown: 1", "Excluded: 3"].contains($0.text) } + [poolClose], true
      ),
      ("pool-middle-row-missing", .compactQuotaPoolDetailsMiddle, poolBackground + [poolTitle] + poolRows.filter { $0.text != "Excluded: 3" } + [poolClose], false),
      ("pool-middle-row-below-close", .compactQuotaPoolDetailsMiddle, poolBackground + [poolTitle] + poolRows.filter { $0.text != "Blocked: 0" } + [poolClose, region("Blocked: 0", 0.1, 0.25, 0.4, 0.03)], false),
      ("pool-hard-bottom-with-notice", .componentQuotaHardPoolDetailsBottom, poolBackground + [hardTitle] + hardBottom + [poolClose], true),
      ("pool-hard-notice-only-in-header", .componentQuotaHardPoolDetailsBottom, poolBackground + [headerNotice, hardTitle] + hardBottom.dropLast() + [poolClose], false),
      ("pool-hard-notice-missing", .componentQuotaHardPoolDetailsBottom, poolBackground + [hardTitle] + hardBottom.dropLast() + [poolClose], false),
      ("pool-hard-foreign-value", .componentQuotaHardPoolDetailsBottom, poolBackground + [hardTitle] + [region("Stale: 1", 0.1, 0.80, 0.4, 0.03), region("Data unknown: 1", 0.1, 0.76, 0.4, 0.03), region("Clients: 0", 0.1, 0.72, 0.4, 0.03), region("Credentials locked", 0.1, 0.68, 0.4, 0.03)] + [poolClose], false),
    ]
    for (name, checkpoint, regions, shouldPass) in coverageCases {
      if missingPoolCensus(checkpoint: checkpoint, regions: regions).isEmpty != shouldPass { failures.append(name) }
    }
    // The three pose subsets are exactly each fixture's literal census; dropping
    // one entry breaks the union. The hard notice is required on bottom only.
    func poseUnion(_ poses: [ScreenCheckpoint]) -> Set<String> {
      Set(poses.flatMap(\.requiredPoolCensus))
    }
    let compactPoses: [ScreenCheckpoint] = [
      .compactQuotaPoolDetails, .compactQuotaPoolDetailsMiddle, .compactQuotaPoolDetailsBottom,
    ]
    let hardPoses: [ScreenCheckpoint] = [
      .componentQuotaHardPoolDetails, .componentQuotaHardPoolDetailsMiddle,
      .componentQuotaHardPoolDetailsBottom,
    ]
    let unionCases: [(String, Set<String>, [String], Bool)] = [
      ("pool-union-compact", poseUnion(compactPoses), ScreenCheckpoint.compactPoolCensus, true),
      ("pool-union-hard", poseUnion(hardPoses), ScreenCheckpoint.hardCasePoolCensus, true),
      (
        "pool-union-missing-entry", poseUnion(compactPoses).subtracting(["excluded 3"]),
        ScreenCheckpoint.compactPoolCensus, false
      ),
    ]
    for (name, union, census, equal) in unionCases {
      if (union == Set(census) && census.count == 10) != equal { failures.append(name) }
    }
    if !ScreenCheckpoint.componentQuotaHardPoolDetailsBottom.requiresPoolNotice
      || ScreenCheckpoint.componentQuotaHardPoolDetailsMiddle.requiresPoolNotice
      || ScreenCheckpoint.componentQuotaHardPoolDetails.requiresPoolNotice
    {
      failures.append("pool-hard-notice-bottom-only")
    }
    // Native boxes from the retained 120x36 cancelled original of the old
    // layout, whose header still carried the census: now a leak control.
    let nativeCensusHeader = [
      region("Quota status", 0.016313212642740636, 0.9455587389684814, 0.10114192495921696, 0.022922636103151817),
      region("Account", 0.058712286237160745, 0.8365372328592812, 0.05875813669326061, 0.02033527669387425),
      region("Clients 0 · Fresh 0 · Stale 0 · Data unknown 4", 0.4747145155655248, 0.9484240686485196, 0.38336052202283843, 0.02005730659025784),
      region("Pool-wide: responses", 0.016313212928221882, 0.916905444448424, 0.16639477977161501, 0.025787965616045905),
      region("Accounts 4 · Usable 0 · Reserve 0 · Blocked 0 · Pool unknown", 0.1990212130505708, 0.9197707740210124, 0.5024469820554649, 0.022922635420004056),
      region("1. Excluded 3", 0.6998368699564981, 0.919770773686724, 0.12561174551386622, 0.02005730659025784),
    ]
    // Native boxes of the calibration-004 120x36 hard-case first browse: Vision
    // splits Alpha's window rows into separate value/window/reset boxes, and
    // Beta's legitimate "Credits stale" row sits above its "2 resets" row.
    let hardHeader = [
      region("Quota status", 0.0163, 0.9456, 0.1011, 0.0229),
      region("Credentials locked", 0.0163, 0.9198, 0.1501, 0.0201),
      region("Filters y Provider: All • Disabled: Shown • 2/2 accounts", 0.0163, 0.8911, 0.4747, 0.0229),
      region("Clients 1", 0.7390, 0.9456, 0.0767, 0.0229),
      region("Pool details Help", 0.8320, 0.9398, 0.1517, 0.0287),
    ]
    let hardHeadings = [
      region("Account", 0.0505, 0.8364, 0.0588, 0.0207),
      region("Quota left • reset in", 0.3997, 0.8367, 0.1762, 0.0229),
      region("Weekly pace", 0.6476, 0.8273, 0.0948, 0.0302),
      region("Extra", 0.8075, 0.8337, 0.0425, 0.0231),
    ]
    let hardAlphaName = [
      region("## OpenAI Research Account Alpha Long", 0.0392, 0.7736, 0.3361, 0.0430),
      region("Name", 0.0489, 0.7536, 0.0343, 0.0201),
      region("• OpenAI • 1 client", 0.0489, 0.7221, 0.1599, 0.0258),
      region("Credentials locked", 0.0506, 0.6963, 0.1501, 0.0258),
    ]
    let hardAlphaWeek = [
      region("92%", 0.4649, 0.7794, 0.0277, 0.0229),
      region("week", 0.5090, 0.7822, 0.0424, 0.0201),
      region("•6d 16h", 0.5546, 0.7822, 0.0701, 0.0201),
    ]
    let hardAlphaShort = [
      region("83%", 0.4649, 0.7507, 0.0277, 0.0229),
      region("5h • 2h 10m", 0.5155, 0.7536, 0.0930, 0.0201),
    ]
    let hardAlphaRest = [
      region("burn unavailable", 0.6493, 0.7794, 0.1338, 0.0229),
      region("reset pace", 0.6509, 0.7507, 0.0832, 0.0229),
      region("unavailable", 0.6476, 0.7221, 0.0946, 0.0258),
      region("62.5k credits", 0.8059, 0.7765, 0.1109, 0.0258),
      region("2 resets", 0.8075, 0.7533, 0.0669, 0.0234),
    ]
    let hardBetaName = [
      region("Claude Research Account Beta Full Name", 0.0506, 0.6132, 0.3165, 0.0229),
      region("• Claude • 0 clients", 0.0489, 0.5845, 0.1680, 0.0229),
      region("Credentials locked", 0.0489, 0.5559, 0.1517, 0.0258),
    ]
    let hardBetaWeek = [region("• 75% • week • -", 0.4502, 0.6132, 0.1338, 0.0229)]
    let hardBetaStale = [
      region("week", 0.3980, 0.5817, 0.0359, 0.0258),
      region("stale", 0.4372, 0.5845, 0.0473, 0.0229),
    ]
    let hardBetaShort = [region("No data • 5h • -", 0.3980, 0.5587, 0.1354, 0.0201)]
    let hardBetaRest = [
      region("burn unavailable", 0.6476, 0.6103, 0.1354, 0.0258),
      region("reset pace", 0.6509, 0.5845, 0.0832, 0.0229),
      region("unavailable", 0.6476, 0.5559, 0.0946, 0.0258),
      region("125.5 credits", 0.8059, 0.6103, 0.1109, 0.0258),
      region("Credits stale", 0.8059, 0.5817, 0.1109, 0.0258),
      region("2 resets", 0.8075, 0.5582, 0.0670, 0.0211),
    ]
    let hardFooter = [
      region("F4 Details F3 Filters • F5 Pool F1 Help • Ctrl+R Options • Ctrl+E Floor", 0.0163, 0.0287, 0.6101, 0.0258)
    ]
    func hardBrowse(
      alphaWeek: [ScreenTextRegion]? = nil, betaWeek: [ScreenTextRegion]? = nil,
      extra: [ScreenTextRegion] = []
    ) -> [ScreenTextRegion] {
      hardHeader + hardHeadings + hardAlphaName + (alphaWeek ?? hardAlphaWeek) + hardAlphaShort
        + hardAlphaRest + hardBetaName + (betaWeek ?? hardBetaWeek) + hardBetaStale
        + hardBetaShort + hardBetaRest + hardFooter + extra
    }
    let nativeHardBrowse = hardBrowse()
    let leakCases: [(String, [ScreenTextRegion], Bool)] = [
      ("leak-old-native-header", nativeCensusHeader, true),
      (
        "leak-colon-census",
        [region("Quota status", 0.02, 0.95, 0.2, 0.03), region("Accounts: 4 Usable: 0", 0.3, 0.95, 0.3, 0.03), region("Account", 0.02, 0.8, 0.1, 0.03)],
        true
      ),
      (
        "leak-data-unknown-only",
        [region("Quota status", 0.02, 0.95, 0.2, 0.03), region("Data unknown 4", 0.3, 0.95, 0.2, 0.03), region("Account", 0.02, 0.8, 0.1, 0.03)],
        true
      ),
      ("leak-native-hard-browse-row-stale-warnings", nativeHardBrowse, false),
      ("leak-header-stale-count", hardBrowse(extra: [region("Stale 1", 0.55, 0.9198, 0.08, 0.0201)]), true),
      (
        "leak-header-split-label-count",
        hardBrowse(extra: [region("Fresh", 0.55, 0.9198, 0.05, 0.0201), region("0", 0.61, 0.9198, 0.01, 0.0201)]),
        true
      ),
      (
        "leak-header-cross-row-not-joined",
        [hardHeader[0], hardHeader[3], hardHeader[4]]
          + [region("Credits stale", 0.55, 0.9198, 0.1, 0.0201), region("2 resets", 0.55, 0.8911, 0.07, 0.0229)]
          + hardHeadings,
        false
      ),
      // The obligation is the header census; a body row is not the header.
      ("leak-body-count-below-headings", hardBrowse(extra: [region("Stale 1", 0.40, 0.45, 0.08, 0.0229)]), false),
      (
        "leak-toolbar-fallback-scrolled",
        hardHeader + [region("Stale 1", 0.55, 0.8911, 0.08, 0.0229)] + hardBetaRest,
        true
      ),
      ("leak-toolbar-fallback-row-warning", hardHeader + hardBetaStale + hardBetaRest, false),
      (
        "leak-ambiguous-header-rows",
        hardBrowse(extra: [region("Stale", 0.55, 0.89, 0.05, 0.07)]), true
      ),
      ("leak-invalid-geometry", hardBrowse(extra: [region("Usable 1", -0.1, 0.95)]), true),
    ]
    for (name, regions, leaks) in leakCases {
      if browseCensusLeaks(regions).isEmpty == leaks { failures.append(name) }
    }
    let alphaWeekMarker = "window alpha 92% week 6d 16h"
    let foldedHeading = region("Quota left · reset in", 0.05, 0.70, 0.3, 0.03)
    let windowCases: [(String, String, [ScreenTextRegion], Bool)] = [
      ("window-native-alpha-week", alphaWeekMarker, nativeHardBrowse, true),
      ("window-native-alpha-5h", "window alpha 83% 5h 2h 10m", nativeHardBrowse, true),
      ("window-native-beta-week", "window beta 75% week —", nativeHardBrowse, true),
      ("window-native-beta-5h", "window beta no data 5h —", nativeHardBrowse, true),
      ("window-foreign-account", "window alpha 75% week —", nativeHardBrowse, false),
      ("window-foreign-account-5h", "window alpha no data 5h —", nativeHardBrowse, false),
      (
        "window-moved-into-beta-block",
        alphaWeekMarker,
        hardBrowse(alphaWeek: [
          region("92%", 0.4649, 0.5300, 0.0277, 0.0229), region("week", 0.5090, 0.5328, 0.0424, 0.0201),
          region("•6d 16h", 0.5546, 0.5328, 0.0701, 0.0201),
        ]),
        false
      ),
      (
        "window-split-across-rows",
        alphaWeekMarker,
        hardBrowse(alphaWeek: [
          region("92%", 0.4649, 0.7794, 0.0277, 0.0229), region("week", 0.5090, 0.7250, 0.0424, 0.0201),
          region("•6d 16h", 0.5546, 0.7250, 0.0701, 0.0201),
        ]),
        false
      ),
      (
        "window-in-pace-column",
        alphaWeekMarker,
        hardBrowse(alphaWeek: [
          region("92%", 0.6476, 0.6900, 0.0277, 0.0229), region("week", 0.6800, 0.6928, 0.0424, 0.0201),
          region("•6d 16h", 0.7300, 0.6928, 0.0701, 0.0201),
        ]),
        false
      ),
      (
        "window-wrong-percent",
        alphaWeekMarker,
        hardBrowse(alphaWeek: [region("82%", 0.4649, 0.7794, 0.0277, 0.0229)] + hardAlphaWeek.dropFirst()),
        false
      ),
      (
        "window-unknown-reset-is-not-a-duration",
        "window beta 75% week —",
        hardBrowse(betaWeek: [region("• 75% • week • 6d", 0.4502, 0.6132, 0.1338, 0.0229)]),
        false
      ),
      (
        "window-missing-fabricated-bar",
        "window beta no data 5h —",
        hardHeadings + hardBetaName + [region("███ No data • 5h • -", 0.3980, 0.5587, 0.1354, 0.0201)],
        false
      ),
      (
        "window-folded-own-heading",
        alphaWeekMarker,
        [region("OpenAI Research Account Alpha Long Name", 0.05, 0.80, 0.4, 0.03), foldedHeading,
         region("█████░░ 92% · week · 6d 16h", 0.05, 0.66, 0.5, 0.03)],
        true
      ),
      (
        "window-scrolled-no-name-before-next-account",
        alphaWeekMarker,
        [region("92% · week · 6d 16h", 0.05, 0.80, 0.4, 0.03),
         region("Claude Research Account Beta Full Name", 0.05, 0.60, 0.4, 0.03)],
        true
      ),
      (
        "window-scrolled-row-after-own-name-below",
        "window beta 75% week —",
        [region("75% · week · —", 0.05, 0.80, 0.4, 0.03),
         region("Claude Research Account Beta Full Name", 0.05, 0.60, 0.4, 0.03)],
        false
      ),
      // 48x12 Beta short reads "No data · 5h · —" as "No data : 5h • -".
      ("window-scrolled-colon-separator", "window beta no data 5h —", [region("No data : 5h • -", 0.05, 0.80, 0.4, 0.03)], true),
      ("window-colon-wrong-window", "window beta no data 5h —", [region("No data : week • -", 0.05, 0.80, 0.4, 0.03)], false),
      ("window-invalid-geometry", alphaWeekMarker, hardBrowse(extra: [region("92%", 0.4649, -0.1)]), false),
    ]
    for (name, marker, regions, present) in windowCases {
      if missingWindowLines([marker], regions: regions).isEmpty != present {
        failures.append(name)
      }
    }
    // Native boxes of the calibration-003 120x36 provider no-match original:
    // the nested no-match row touches the query row (overlap 0.0029).
    let nativeNoMatch = [
      region("Account filters", 0.1485, 0.6934, 0.1272, 0.0258),
      region("Account provider", 0.1485, 0.6619, 0.1354, 0.0287),
      region("Disabled accounts", 0.1501, 0.6132, 0.1419, 0.0229),
      region("Search: zznomatchzzl", 0.3230, 0.6619, 0.1631, 0.0287),
      region("Cancel", 0.1500, 0.2805, 0.0507, 0.0235),
      region("Select", 0.2153, 0.2774, 0.0524, 0.0240),
    ]
    let adjacentRowCases: [(String, [ScreenTextRegion], Bool)] = [
      (
        "no-match-native-adjacent-row",
        nativeNoMatch + [region("No matching choices", 0.3214, 0.6361, 0.1615, 0.0287)], true
      ),
      (
        "no-match-same-row",
        nativeNoMatch + [region("No matching choices", 0.5000, 0.6619, 0.1615, 0.0287)], false
      ),
      (
        "no-match-above-query",
        nativeNoMatch + [region("No matching choices", 0.3214, 0.6800, 0.1615, 0.0100)], false
      ),
      (
        "no-match-too-far-below",
        nativeNoMatch + [region("No matching choices", 0.3214, 0.5500, 0.1615, 0.0287)], false
      ),
    ]
    for (name, regions, associated) in adjacentRowCases {
      let pairs = sessionsChoiceQueryPairs(
        regions, title: "account filters", field: "account provider")
      if (pairs == ["field account provider query zznomatchzz"]) != associated {
        failures.append(name)
      }
    }
    // Native boxes of the 120x36 New session no-match original: the
    // Machine label box starts 0.0016 left of the dialog heading.
    let nativeNewSession = [
      region("New session", 0.1500, 0.6954, 0.0915, 0.0276),
      region("Client", 0.1468, 0.6645, 0.0539, 0.0263),
      region("Codex y", 0.2642, 0.6645, 0.0604, 0.0263),
      region("Search: zznomatchzz", 0.2659, 0.6418, 0.1615, 0.0229),
      region("No matching choices", 0.2659, 0.6103, 0.1599, 0.0258),
      region("Directory", 0.1481, 0.5806, 0.0774, 0.0285),
      region("Cancel", 0.1501, 0.2808, 0.0506, 0.0229),
      region("Select", 0.2153, 0.2777, 0.0523, 0.0233),
    ]
    let newSessionCases: [(String, [ScreenTextRegion], Bool)] = [
      ("no-match-native-label-left-of-heading", nativeNewSession + [region("Machine", 0.1484, 0.6416, 0.0604, 0.0233)], true),
      ("no-match-label-in-background-column", nativeNewSession + [region("Machine", 0.0571, 0.6416, 0.0604, 0.0233)], false),
    ]
    for (name, regions, associated) in newSessionCases {
      let pairs = sessionsChoiceQueryPairs(regions, title: "new session", field: "machine")
      if (pairs == ["field machine query zznomatchzz"]) != associated { failures.append(name) }
    }
    // native boxes: Credit usage no-match (C7 nested query).
    let nativePolicy = [
      region("Credit usage", 0.1485, 0.6905, 0.1028, 0.0287),
      region("Balance", 0.1501, 0.6132, 0.0587, 0.0229),
      region("Available • 62,500", 0.2806, 0.6046, 0.1533, 0.0315),
      region("Use credits", 0.1501, 0.5587, 0.0914, 0.0229),
      region("Cancel", 0.1468, 0.3066, 0.0538, 0.0258),
      region("Select", 0.2153, 0.3066, 0.0506, 0.0229),
    ]
    let policyQuery = region("Search: zznomatchzz", 0.3002, 0.5559, 0.1615, 0.0258)
    let policyNoMatch = region("No matching choices", 0.2985, 0.5272, 0.1599, 0.0258)
    let policyCases: [(String, [ScreenTextRegion], Bool)] = [
      ("policy-native-nested-query", nativePolicy + [policyQuery, policyNoMatch], true),
      ("policy-query-on-balance-row", nativePolicy + [region("Search: zznomatchzz", 0.3002, 0.6132, 0.1615, 0.0229), policyNoMatch], false),
      ("policy-query-outside-dialog", nativePolicy + [region("Search: zznomatchzz", 0.0100, 0.5559, 0.1300, 0.0258), policyNoMatch], false),
      ("policy-no-match-row-missing", nativePolicy + [policyQuery], false),
    ]
    for (name, regions, associated) in policyCases {
      let pairs = sessionsChoiceQueryPairs(regions, title: "credit usage", field: "use credits")
      if (pairs == ["field use credits query zznomatchzz"]) != associated { failures.append(name) }
    }
    // native boxes: refreshed Credits FormField columns.
    let freshnessLabels = [
      region("Status", 0.1501, 0.5874, 0.0506, 0.0201),
      region("Freshness", 0.1501, 0.5587, 0.0750, 0.0201),
      region("Observation", 0.1501, 0.5301, 0.0914, 0.0229),
    ]
    let freshnessValues = [
      region("Available • 63,000", 0.2822, 0.5788, 0.1517, 0.0315),
      region("checked 0s ago", 0.2838, 0.5272, 0.1158, 0.0258),
    ]
    let freshnessCases: [(String, [ScreenTextRegion], Bool)] = [
      ("freshness-native-columns", freshnessLabels + freshnessValues + [region("Fresh", 0.2822, 0.5557, 0.0441, 0.0232)], true),
      ("freshness-wrong-value", freshnessLabels + freshnessValues + [region("Stale", 0.2822, 0.5557, 0.0441, 0.0232)], false),
      ("freshness-value-on-observation-row", freshnessLabels + freshnessValues + [region("Fresh", 0.4100, 0.5301, 0.0441, 0.0232)], false),
      ("freshness-value-missing", freshnessLabels + freshnessValues, false),
    ]
    for (name, regions, paired) in freshnessCases {
      if measuredFieldPairs(regions).contains("freshness fresh") != paired { failures.append(name) }
    }
    // help identity: the zh-Hans body pass replaces the first-pass
    // Latin misread; title, Close and outside rows stay first-pass.
    let helpTitle = region("Quota Help", 0.1484, 0.6904, 0.0849, 0.0290)
    let helpClose = region("Close", 0.1484, 0.2778, 0.0441, 0.0261)
    let helpFirstPass = [
      helpTitle, region("Account", 0.1484, 0.6674, 0.0604, 0.0234),
      region("*F OpenAI Research Account Alpha Long Name [alpha-tag]", 0.2887, 0.6619, 0.4698, 0.0315),
      region("Current quota", 0.1501, 0.6103, 0.1093, 0.0258), helpClose,
    ]
    let helpBodyPass = [
      region("Account", 0.1497, 0.6674, 0.0603, 0.0252), region("東京", 0.2914, 0.6676, 0.0309, 0.0248),
      region("OpenAI Research Account Alpha Long Name ［alpha-tag］", 0.3191, 0.6648, 0.4379, 0.0276),
      region("Current quota", 0.1501, 0.6103, 0.1093, 0.0258),
    ]
    let alphaIdentity = "東京 openai research account alpha long name [alpha-tag]"
    let helpMergeCases: [(String, [ScreenTextRegion], [ScreenTextRegion], Bool)] = [
      ("help-body-pass-recovers-cjk", helpFirstPass, helpBodyPass, true),
      ("help-first-pass-latin-misread", helpFirstPass, [], false),
      ("help-body-pass-wrong-tag", helpFirstPass, helpBodyPass.map { $0.text.contains("alpha-tag") ? region("OpenAI Research Account Alpha Long Name ［beta-tag］", 0.3191, 0.6648, 0.4379, 0.0276) : $0 }, false),
      ("help-body-pass-outside-body-ignored", helpFirstPass, [region("東京 OpenAI Research Account Alpha Long Name ［alpha-tag］", 0.2914, 0.7300, 0.47, 0.0276)], false),
    ]
    for (name, firstPass, bodyPass, matches) in helpMergeCases {
      let merged = helpBodyRecognitionBounds(firstPass).map {
        mergingHelpBody(firstPass: firstPass, bodyPass: bodyPass, bounds: $0)
      } ?? firstPass
      if contextualHelpAccountIdentityMatches(merged, capturedIdentity: alphaIdentity) != matches {
        failures.append(name)
      }
    }
    if helpBodyRecognitionBounds(helpFirstPass.filter { $0.text != "Close" }) != nil {
      failures.append("help-body-bounds-need-single-close")
    }
    // 48x24 Beta extra words: header-less gutter geometry from the
    // folded group headings; a selector glyph never moves the origin.
    let gutterGlyph = region("❯", 0.0700, 0.7479, 0.0200, 0.0413)
    let gutterWords = [
      region("Disabled", 0.20, 0.8306, 0.14, 0.0413),
      region("Credentials", 0.1167, 0.7479, 0.24, 0.0413),
      region("Quota", 0.1206, 0.7107, 0.0900, 0.0372),
      region("Weekly", 0.1167, 0.4959, 0.1200, 0.0413),
      region("Extra", 0.1164, 0.3337, 0.1173, 0.0393),
      region("125.5", 0.1167, 0.2479, 0.11, 0.0413),
      region("F4", 0.0389, 0.0826, 0.045, 0.0413),
    ]
    // 48x12 Beta extra: every heading scrolled away; the row-start
    // "Credits stale" label marks the content column.
    let headinglessWords = gutterWords.filter { !["Quota", "Weekly", "Extra"].contains($0.text) }
    let staleLabelWords = headinglessWords + [
      region("Credits", 0.1167, 0.4132, 0.1400, 0.0413), region("stale", 0.2800, 0.4132, 0.1000, 0.0413),
    ]
    let gutterCases: [(String, [ScreenTextRegion], Bool)] = [
      ("gutter-headerless-band", gutterWords, true),
      ("gutter-glyph-does-not-move-origin", gutterWords + [gutterGlyph], true),
      ("gutter-no-headings", gutterWords.filter { !["Quota", "Weekly", "Extra"].contains($0.text) }, false),
      (
        "gutter-disagreeing-headings",
        gutterWords.map { $0.text == "Weekly" ? region("Weekly", 0.3000, 0.4959, 0.1200, 0.0413) : $0 }, false
      ),
      ("gutter-stale-label-band", staleLabelWords, true),
      ("gutter-stale-label-glyph-does-not-move-origin", staleLabelWords + [gutterGlyph], true),
      (
        "gutter-unit-credits-not-label",
        headinglessWords + [region("credits", 0.2400, 0.2479, 0.1400, 0.0413)], false
      ),
      (
        "gutter-stale-on-other-row",
        headinglessWords + [
          region("Credits", 0.1167, 0.4132, 0.1400, 0.0413), region("stale", 0.2800, 0.3500, 0.1000, 0.0413),
        ],
        false
      ),
      (
        "gutter-stale-label-not-row-start",
        headinglessWords + [
          region("2", 0.1167, 0.4132, 0.0200, 0.0413), region("Credits", 0.1600, 0.4132, 0.1400, 0.0413),
          region("stale", 0.3233, 0.4132, 0.1000, 0.0413),
        ],
        false
      ),
      (
        "gutter-stale-label-disagrees-with-headings",
        gutterWords + [
          region("Credits", 0.3000, 0.4132, 0.1400, 0.0413), region("stale", 0.4633, 0.4132, 0.1000, 0.0413),
        ],
        false
      ),
      ("gutter-credits-without-stale", headinglessWords + [region("Credits", 0.1167, 0.4132, 0.1400, 0.0413), region("used", 0.2800, 0.4132, 0.1000, 0.0413)], false),
      ("gutter-no-toolbar", gutterWords.filter { $0.text != "Disabled" }, false),
      ("gutter-no-footer", gutterWords.filter { $0.text != "F4" }, false),
    ]
    for (name, words, hasBand) in gutterCases {
      guard let band = headerlessSelectorGutter(words) else {
        if hasBand { failures.append(name) }
        continue
      }
      // The band lies left of the heading origin and contains the glyph's cells.
      if !hasBand || band.maxX > 0.1164 || band.minX > gutterGlyph.bounds.minX
        || band.maxX < gutterGlyph.bounds.maxX
      {
        failures.append(name)
      }
    }
    let enterResumeCases: [(String, String, Bool)] = [
      ("enter-resume-modified-wide", "←/→ tab focus · enter/space activate · esc close\nctrl+enter resume · ctrl+shift+enter fork", false),
      ("enter-resume-modified-narrow", "tab/←→ focus · enter/space activate\nctrl+↵ resume · ctrl+shift+↵ fork", false),
      ("enter-resume-ocr-spaced-modifier", "ctrl+ enter resume", false),
      ("enter-resume-fixed-label", "enter resume · esc close", true),
      ("enter-resume-fixed-after-activate", "enter/space activate · enter resume", true),
      ("enter-resume-fixed-glyph", "↵ resume", true),
    ]
    for (name, text, fixed) in enterResumeCases {
      if labelsEnterAsFixedResume(text) != fixed { failures.append(name) }
    }
    // A split count binds only to its own label's immediate right neighbour.
    let splitLabel = region("Pool unknown:", 0.1, 0.64, 0.13, 0.03)
    let splitCases: [(String, [ScreenTextRegion], Bool)] = [
      ("pool-split-count-adjacent", replacingPoolRow(4, with: splitLabel) + [region("1", 0.235, 0.64, 0.01, 0.03)], true),
      ("pool-split-count-far-right-underlay", replacingPoolRow(4, with: splitLabel) + [region("1", 0.85, 0.64, 0.01, 0.03)], false),
      (
        "pool-split-count-nearer-wrong",
        replacingPoolRow(4, with: splitLabel)
          + [region("3", 0.235, 0.64, 0.01, 0.03), region("1", 0.85, 0.64, 0.01, 0.03)], false
      ),
      (
        "pool-split-count-after-other-box",
        replacingPoolRow(4, with: splitLabel)
          + [region("stale", 0.235, 0.64, 0.05, 0.03), region("1", 0.29, 0.64, 0.01, 0.03)], false
      ),
    ]
    for (name, regions, shouldPass) in splitCases {
      let passed = missingPoolCensus(checkpoint: .compactQuotaPoolDetailsComplete, regions: regions)
        .isEmpty
      if passed != shouldPass { failures.append(name) }
    }
    // The second recognition alone supplies the dialog body inside its bounds.
    let droppedDigitFirstPass = replacingPoolRow(4, with: splitLabel)
    let mergeCases: [(String, [ScreenTextRegion], [ScreenTextRegion], Bool)] = [
      ("pool-body-pass-recovers-dropped-count", droppedDigitFirstPass, poolRows, true),
      // The native second pass also returns its own title and Close rows
      // (calibration-004: "Pool details" and "CLose"); they never join the frame.
      (
        "pool-body-pass-own-title-and-close-rows",
        droppedDigitFirstPass,
        [region("Pool details", 0.1, 0.861, 0.25, 0.03)] + poolRows + [region("CLose", 0.1, 0.301, 0.1, 0.03)],
        true
      ),
      // Isolated borrowing: only the first pass has Reserve and only the second
      // pass has the other rows, so keeping first-pass body boxes would pass
      // with no duplicate to mask it.
      (
        "pool-body-pass-reserve-only-in-first-pass",
        poolBackground + [poolTitle] + poolRows.filter { $0.text == "Reserve: 0" } + [poolClose],
        poolRows.filter { $0.text != "Reserve: 0" }, false
      ),
      (
        "pool-body-pass-missing-row-not-borrowed-from-first-pass",
        poolDialog, poolRows.filter { $0.text != "Reserve: 0" }, false
      ),
      (
        "pool-body-pass-outside-body-ignored",
        droppedDigitFirstPass,
        poolRows.filter { $0.text != "Pool unknown: 1" } + [region("Pool unknown: 1", 0.1, 0.2, 0.3, 0.03)],
        false
      ),
      (
        "pool-body-pass-duplicate-count",
        droppedDigitFirstPass, poolRows + [region("Pool unknown: 1", 0.5, 0.40, 0.3, 0.03)], false
      ),
      ("pool-body-pass-invalid-geometry", droppedDigitFirstPass, poolRows + [region("1", 0.5, -0.1)], false),
    ]
    for (name, firstPass, bodyPass, shouldPass) in mergeCases {
      guard let bounds = poolBodyRecognitionBounds(firstPass) else {
        failures.append(name)
        continue
      }
      let merged = mergingPoolBody(firstPass: firstPass, bodyPass: bodyPass, bounds: bounds)
      let passed = missingPoolCensus(checkpoint: .compactQuotaPoolDetailsComplete, regions: merged)
        .isEmpty
      if passed != shouldPass { failures.append(name) }
    }
    if poolBodyRecognitionBounds(poolBackground + [poolTitle] + poolRows) != nil {
      failures.append("pool-body-bounds-need-single-close")
    }
    // C1: Beta's Extra pose through observe(): a readable selection must be
    // Beta; unreadable passes only with the selector column measured empty.
    let betaExtraLines = [
      "Quota status", "Credentials locked", "125.5 credits", "Credits stale",
      "F4 Details F3 Filters · F5 Pool F1 Help · Ctrl+R Options · Ctrl+E Floor",
    ]
    let betaExtraCases: [(String, String?, Bool?, Bool)] = [
      ("beta-extra-valid-beta", "Claude Research Account Beta Full Name", true, true),
      ("beta-extra-valid-beta-unknown-visibility", "Claude Research Account Beta Full Name", nil, true),
      ("beta-extra-foreign-alpha", "OpenAI Research Account Alpha Long Name", true, false),
      ("beta-extra-visible-unreadable", nil, true, false),
      ("beta-extra-unknown-visibility", nil, nil, false),
      ("beta-extra-offscreen-selection", nil, false, true),
    ]
    for (name, selected, visibility, shouldPass) in betaExtraCases {
      let result = observe(
        checkpoint: .componentQuotaHardBetaExtra, recognizedLines: betaExtraLines,
        selectedAccountName: selected, selectedRowVisible: visibility)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    // C5 hard 48x12: the selected name continuation is the top body row, painted
    // selected, directly over its own client line. The one-page-down layout is
    // header, toolbar, continuation, client line, credential row.
    let continuationHeader = [
      region("Quota status", 0.03, 0.92, 0.2, 0.03),
      region("Credentials locked", 0.03, 0.88, 0.25, 0.03),
      region("All · Disabled: Shown · 2/2 ▾", 0.03, 0.84, 0.45, 0.03),
    ]
    func continuationPage(_ name: String, _ client: String) -> [ScreenTextRegion] {
      continuationHeader + [
        region(name, 0.07, 0.78, 0.15, 0.03),
        region(client, 0.07, 0.74, 0.3, 0.03),
        region("Credentials locked", 0.07, 0.70, 0.25, 0.03),
      ]
    }
    let alphaPage = continuationPage("Long Name", "◎ OpenAI · 1 client")
    let betaPage = continuationPage("Name", "◆ Claude · 0 clients")
    // 48x12 Alpha: Vision splits the continuation into two word boxes.
    let splitAlphaPage = continuationHeader + [
      region("Long", 0.07, 0.78, 0.07, 0.03), region("Name", 0.15, 0.78, 0.07, 0.03),
      region("◎ OpenAI · 1 client", 0.07, 0.74, 0.3, 0.03), region("Credentials locked", 0.07, 0.70, 0.25, 0.03),
    ]
    let alphaWant = ("long name", "openai · 1 client")
    let betaWant = ("name", "claude · 0 clients")
    let continuationCases: [(String, [ScreenTextRegion], (String, String), Bool, Bool)] = [
      ("continuation-alpha", alphaPage, alphaWant, true, true),
      ("continuation-beta", betaPage, betaWant, true, true),
      ("continuation-beta-ocr-bullets", continuationPage("Name", "• Claude • 0 clients"), betaWant, true, true),
      ("continuation-beta-ocr-colon", continuationPage("Name", "◆ Claude : 0 clients"), betaWant, true, true),
      ("continuation-colon-wrong-count", continuationPage("Name", "◆ Claude : 1 clients"), betaWant, true, false),
      ("continuation-alpha-split-word-boxes", splitAlphaPage, alphaWant, true, true),
      (
        "continuation-split-across-rows",
        continuationHeader + [
          region("Long", 0.07, 0.78, 0.07, 0.03), region("Name", 0.07, 0.74, 0.07, 0.03),
          region("◎ OpenAI · 1 client", 0.07, 0.70, 0.3, 0.03),
        ],
        alphaWant, true, false
      ),
      (
        "continuation-split-reordered",
        continuationHeader + [
          region("Name", 0.07, 0.78, 0.07, 0.03), region("Long", 0.15, 0.78, 0.07, 0.03),
          region("◎ OpenAI · 1 client", 0.07, 0.74, 0.3, 0.03), region("Credentials locked", 0.07, 0.70, 0.25, 0.03),
        ],
        alphaWant, true, false
      ),
      ("continuation-alpha-expected-on-beta-image", betaPage, alphaWant, true, false),
      ("continuation-beta-expected-on-alpha-image", alphaPage, betaWant, true, false),
      ("continuation-unselected", alphaPage, alphaWant, false, false),
      (
        "continuation-background-name-not-adjacent",
        continuationHeader + [
          region("Long Name", 0.6, 0.66, 0.15, 0.03), region("◎ OpenAI · 1 client", 0.07, 0.74, 0.3, 0.03),
        ],
        alphaWant, true, false
      ),
      (
        "continuation-below-client-line",
        continuationHeader + [
          region("◎ OpenAI · 1 client", 0.07, 0.78, 0.3, 0.03), region("Long Name", 0.07, 0.74, 0.15, 0.03),
        ],
        alphaWant, true, false
      ),
      (
        "continuation-aligned-pair-above-toolbar",
        [
          region("Quota status", 0.03, 0.96, 0.2, 0.03),
          region("Long Name", 0.07, 0.92, 0.15, 0.03), region("◎ OpenAI · 1 client", 0.07, 0.88, 0.3, 0.03),
          region("All · Disabled: Shown · 2/2 ▾", 0.03, 0.84, 0.45, 0.03),
          region("Credentials locked", 0.07, 0.78, 0.25, 0.03),
        ],
        alphaWant, true, false
      ),
      ("continuation-not-top-body-row", alphaPage + [region("? unknown", 0.07, 0.812, 0.1, 0.025)], alphaWant, true, false),
      ("continuation-account-heading-visible", alphaPage + [region("Account", 0.07, 0.60, 0.1, 0.03)], alphaWant, true, false),
      (
        "continuation-other-account-name-visible",
        alphaPage + [region("Claude Research Account Beta Full", 0.07, 0.62, 0.4, 0.03)], alphaWant, true, false
      ),
      ("continuation-invalid-geometry", alphaPage + [region("x", -0.1, 0.5)], alphaWant, true, false),
    ]
    for (name, regions, want, selected, shouldPass) in continuationCases {
      let matched = selectedNameContinuationMatches(
        regions, continuation: want.0, clientLine: want.1, hasSelectedText: { _ in selected })
      if matched != shouldPass { failures.append(name) }
    }
    // A split continuation row with one box not painted selected rejects.
    if selectedNameContinuationMatches(
      splitAlphaPage, continuation: alphaWant.0, clientLine: alphaWant.1,
      hasSelectedText: { $0.text != "Name" })
    {
      failures.append("continuation-split-partly-unselected")
    }
    // The hard 48x12 identity markers through observe(): exact selected
    // prefixes at the top poses, full name at tall Pool closed, and the measured
    // continuation field.
    let shortFooter = "F4 Details F3 Filters · F5 Pool F1 Help · Ctrl+R Options · Ctrl+E Floor"
    let betaPrefixLines = ["Quota status", "Claude Research Account Beta Full", shortFooter]
    let identityObserveCases: [(String, ScreenCheckpoint, [String], String?, Bool?, Bool)] = [
      ("short-beta-selected-prefix", .componentQuotaHardBetaSelected, betaPrefixLines, "Claude Research Account Beta Full", nil, true),
      ("short-beta-selected-foreign", .componentQuotaHardBetaSelected, betaPrefixLines, "OpenAI Research Account Alpha", nil, false),
      ("short-beta-selected-unread", .componentQuotaHardBetaSelected, betaPrefixLines, nil, nil, false),
      ("short-pool-closed-top-prefix", .componentQuotaHardPoolClosedTop, betaPrefixLines, "Claude Research Account Beta Full", nil, true),
      ("short-pool-closed-top-foreign", .componentQuotaHardPoolClosedTop, betaPrefixLines, "OpenAI Research Account Alpha", nil, false),
      (
        "tall-pool-closed-prefix-rejected", .componentQuotaHardPoolClosed,
        ["Quota status", "Claude Research Account Beta Full Name", shortFooter], "Claude Research Account Beta Full", nil, false
      ),
      (
        "short-browse-top-alpha-prefix", .componentQuotaHardBrowseTop,
        ["Quota status", "Credentials locked", shortFooter], "東京 OpenAI Research Account Alpha", nil, true
      ),
      (
        "short-browse-top-foreign", .componentQuotaHardBrowseTop,
        ["Quota status", "Credentials locked", shortFooter], "Claude Research Account Beta Full", nil, false
      ),
      (
        "short-continuation-measured", .componentQuotaHardAlphaNameContinued,
        ["Quota status", "Credentials locked", shortFooter], nil, true, true
      ),
      (
        "short-continuation-failed", .componentQuotaHardAlphaNameContinued,
        ["Quota status", "Credentials locked", shortFooter], nil, false, false
      ),
      (
        "short-continuation-unmeasured", .componentQuotaHardBetaNameContinued,
        ["Quota status", "Credentials locked", shortFooter], nil, nil, false
      ),
    ]
    for (name, checkpoint, lines, selected, continuation, shouldPass) in identityObserveCases {
      let result = observe(
        checkpoint: checkpoint, recognizedLines: lines, selectedAccountName: selected,
        selectedContinuationField: continuation)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    let earlierCount: Int =
      poolCases.count + leakCases.count + windowCases.count + adjacentRowCases.count
      + enterResumeCases.count + splitCases.count + mergeCases.count + 1 + betaExtraCases.count
    let identityCount: Int =
      continuationCases.count + identityObserveCases.count + newSessionCases.count
      + coverageCases.count + unionCases.count + 2
    let repairCount: Int =
      policyCases.count + freshnessCases.count + helpMergeCases.count + 1 + gutterCases.count
    let count = earlierCount + identityCount + repairCount
    return (count, failures)
  }
}
