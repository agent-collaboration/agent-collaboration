import Foundation

// Narrow native-reading repairs. Each binds one measured Vision
// misreading of a correctly painted original to an exact re-observation or a
// single bounded token position; none normalizes prose.
extension VerifyScreenImage {
  // The expanded credit-choice consumer uses the same observed label for
  // its selected decision and its active-dialog geometry.
  static func selectedCreditChoice(
    words: [ScreenTextRegion], rawRows: [ScreenTextRegion], pixels: OriginalChoicePixels
  ) -> ScreenTextRegion? {
    let actions = words.filter { normalizedMarkerText($0.text) == "select" }
    guard actions.count == 1, let action = actions.first,
      pixels.enabledTextPixels(in: action.bounds) >= 8 else { return nil }
    // Vision can give Allow the entire ">Allow" observation's box. The
    // unfocused sibling supplies the existing list's shared label column,
    // keeping the attested arrow's pixels separate from the label's pixels.
    let fusedRows = rawRows.filter {
      $0.text.range(of: #"^[›>❯](Allow|Disallow)$"#,
        options: [.regularExpression, .caseInsensitive]) != nil
    }
    if !fusedRows.isEmpty {
      guard fusedRows.count == 1, let raw = fusedRows.first,
        regionsHaveValidBounds(words), regionsHaveValidBounds(rawRows) else { return nil }
      let name = normalizedMarkerText(String(raw.text.dropFirst()))
      let opposite = name == "allow" ? "disallow" : "allow"
      let candidates = words.filter { normalizedMarkerText($0.text) == name }
      let siblings = words.filter { normalizedMarkerText($0.text) == opposite }
      guard candidates.count == 1, siblings.count == 1,
        let candidate = candidates.first, let sibling = siblings.first,
        candidate.bounds.minX == raw.bounds.minX, raw.bounds.contains(candidate.bounds),
        candidate.bounds.minX < sibling.bounds.minX,
        sibling.bounds.minX < candidate.bounds.maxX,
        abs(candidate.bounds.midY - sibling.bounds.midY)
          <= 1.5 * max(candidate.bounds.height, sibling.bounds.height),
        pixels.yellowPixels(in: sibling.bounds) < 8,
        !pixels.selectedNameGutter(beside: sibling)
      else { return nil }
      let context = rawRows.filter {
        normalizedMarkerText($0.text).hasPrefix("credit usage") || holdsExactAlphaTag($0.text)
      }
      let tags = context.filter { holdsExactAlphaTag($0.text) }
      guard expandedCreditChoiceMatches(context + words, selected: "allow"),
        tags.count == 1, let tag = tags.first,
        sibling.bounds.midY < tag.bounds.minY, sibling.bounds.midY > action.bounds.maxY
      else { return nil }
      let label = ScreenTextRegion(text: candidate.text,
        bounds: CGRect(x: sibling.bounds.minX, y: candidate.bounds.minY,
          width: candidate.bounds.maxX - sibling.bounds.minX, height: candidate.bounds.height))
      guard pixels.measure(label)?.hasIndependentFocusPixels == true else { return nil }
      return label
    }
    let focused = words.filter {
      ["allow", "disallow"].contains(normalizedMarkerText($0.text))
        && pixels.selectedNameGutter(beside: $0)
    }
    return focused.count == 1 ? focused.first : nil
  }

  // The exact identity tag as every downstream consumer requires it: the
  // ASCII "[alpha-tag]" in the normalized text.
  static func holdsExactAlphaTag(_ text: String) -> Bool {
    normalizedMarkerText(text).contains("[alpha-tag]")
  }

  // The oracles that require the exact ASCII tag: idle-return eligibility, the
  // observation age, the floor draft and the active credit choice. The
  // confirmation decision oracles bind only "alpha-tag" and keep the first pass
  // (76x30 confirm no/yes/help-returned).
  static let exactIdentityTagCheckpoints: Set<ScreenCheckpoint> = [
    .componentQuotaConfirmNoReturned, .componentQuotaCreditsRefreshedObservation,
    .componentQuotaFloorDraft, .componentQuotaPolicyChoice,
  ]

  // The full-screen pass can misread the modal identity tag (76x30
  // "lalpha-tag]"); an en-US pass over that one row reads it exactly.
  // The row is the single first-pass box naming
  // alpha-tag without the exact tag.
  static func identityTagRowRecognitionBounds(_ regions: [ScreenTextRegion])
    -> (row: ScreenTextRegion, bounds: CGRect)?
  {
    guard regionsHaveValidBounds(regions) else { return nil }
    let rows = regions.filter { normalizedMarkerText($0.text).contains("alpha-tag") }
    guard rows.count == 1, let row = rows.first, !holdsExactAlphaTag(row.text) else { return nil }
    let box = row.bounds
    let minX = max(0, box.minX - box.height)
    let minY = max(0, box.minY - box.height / 2)
    let maxX = min(1, box.maxX + box.height)
    let maxY = min(1, box.maxY + box.height / 2)
    return (row, CGRect(x: minX, y: minY, width: maxX - minX, height: maxY - minY))
  }

  // The Help identity oracle reads the fullwidth tag (compactHelpIdentity); only
  // a census it cannot read re-observes the tag row, which replaces the whole row
  // (120x36: the fullwidth tag shares the CJK name's row).
  static func helpTagRowRecognitionBounds(_ censusRegions: [ScreenTextRegion])
    -> (row: ScreenTextRegion, bounds: CGRect)?
  {
    guard !censusRegions.contains(where: { compactHelpIdentity($0.text).contains("[alpha-tag]") })
    else { return nil }
    return identityTagRowRecognitionBounds(censusRegions)
  }

  // Only the row pass's own boxes on that terminal row replace it, and only
  // when they hold the exact tag; otherwise the first pass stands unchanged.
  static func mergingIdentityTagRow(
    firstPass: [ScreenTextRegion], row: ScreenTextRegion, rowPass: [ScreenTextRegion]
  ) -> [ScreenTextRegion] {
    guard regionsHaveValidBounds(rowPass) else { return firstPass }
    let replacement = rowPass.filter { sharesTerminalRow($0, row) }
    guard replacement.contains(where: { holdsExactAlphaTag($0.text) }) else { return firstPass }
    return firstPass.filter { !sharesTerminalRow($0, row) } + replacement
  }

  static func checkReadingRepairControls() -> (count: Int, failures: [String]) {
    func box(_ text: String, _ x: Double, _ y: Double, _ width: Double, _ height: Double)
      -> ScreenTextRegion
    {
      ScreenTextRegion(text: text, bounds: CGRect(x: x, y: y, width: width, height: height))
    }
    var failures: [String] = []
    // Collapsed choice: 76x30 read the provider's ▾ as a spaced "-".
    let providerLabel = box("Account provider", 0.1485, 0.6619, 0.1354, 0.0287)
    let dashCases: [(String, String, Bool)] = [
      ("dash-disclosure", "OpenAI -", true),
      ("dash-wrong-value", "Claude -", false),
      ("dash-extra-word", "OpenAI - Claude", false),
      ("dash-before-value", "- OpenAI", false),
    ]
    for (name, value, matches) in dashCases {
      let pairs = measuredFieldPairs([providerLabel, box(value, 0.3230, 0.6617, 0.0702, 0.0290)])
      if pairs.contains("account provider openai") != matches { failures.append(name) }
    }
    // Stacked FormField values (native boxes): 40x24 Use credits
    // indented 1.8 cells, 76x30 Freshness in the label column, and a value box
    // reaching into the label row.
    let useCredits = box("Use credits", 0.0741, 0.4587, 0.2778, 0.0372)
    let freshness = box("Freshness", 0.1620, 0.4708, 0.1165, 0.0241)
    let stackedCases: [(String, [ScreenTextRegion], String, Bool)] = [
      (
        "stacked-use-credits-indented",
        [useCredits, box("Disallow y", 0.1203, 0.4128, 0.2548, 0.0421)], "use credits disallow", true
      ),
      (
        "stacked-use-credits-indented-too-far",
        [useCredits, box("Disallow y", 0.1751, 0.4128, 0.2548, 0.0421)], "use credits disallow", false
      ),
      ("stacked-freshness", [freshness, box("Fresh", 0.1594, 0.4327, 0.0685, 0.0315)], "freshness fresh", true),
      (
        "stacked-freshness-wrong-value",
        [freshness, box("Stale", 0.1594, 0.4327, 0.0685, 0.0315)], "freshness fresh", false
      ),
      (
        "stacked-eligibility-touching-row",
        [box("Eligibility", 0.0741, 0.5785, 0.2731, 0.0413), box("Not checked", 0.0694, 0.5420, 0.2824, 0.0372)],
        "eligibility not checked", true
      ),
    ]
    for (name, regions, pair, matches) in stackedCases {
      if measuredFieldPairs(regions).contains(pair) != matches { failures.append(name) }
    }
    // 40x24: "Weekly" and "pace" are separate boxes on one terminal
    // row, enumerated with the window rows between them.
    let footer = "F4 Details F3 Filters · F5 Pool F1 Help · Ctrl+R Options"
    let paceLines = ["Quota status", "Weekly", "83% · week · 7d 0h", "99% · 5h · 5h 0m", "pace", footer]
    let paceRow = [
      box("Quota status", 0.02, 0.94, 0.2, 0.03), box("Weekly", 0.05, 0.40, 0.15, 0.03),
      box("83% · week · 7d 0h", 0.05, 0.50, 0.4, 0.03), box("99% · 5h · 5h 0m", 0.05, 0.46, 0.4, 0.03),
      box("pace", 0.22, 0.40, 0.1, 0.03),
    ]
    let paceCases: [(String, [ScreenTextRegion], Bool)] = [
      ("pace-heading-split-one-row", paceRow, true),
      (
        "pace-heading-split-across-rows",
        paceRow.map { $0.text == "pace" ? box("pace", 0.22, 0.30, 0.1, 0.03) : $0 }, false
      ),
      ("pace-heading-word-between", paceRow + [box("burn", 0.13, 0.40, 0.08, 0.03)], false),
    ]
    for (name, regions, shouldPass) in paceCases {
      let result = observe(
        checkpoint: .componentQuotaBrowsePaceHeading, recognizedLines: paceLines, regions: regions)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    // hard 40x24 reads the second "·" as "." ("• 5h. 2h 10m").
    let alphaFiveHour = "window alpha 83% 5h 2h 10m"
    let windowCases: [(String, [ScreenTextRegion], Bool)] = [
      ("window-dot-separator", [box("83%", 0.05, 0.50, 0.08, 0.03), box("• 5h . 2h 10m", 0.14, 0.50, 0.3, 0.03)], true),
      (
        "window-dot-wrong-window",
        [box("83%", 0.05, 0.50, 0.08, 0.03), box("• week . 2h 10m", 0.14, 0.50, 0.3, 0.03)], false
      ),
    ]
    for (name, regions, present) in windowCases {
      if missingWindowLines([alphaFiveHour], regions: regions).isEmpty != present { failures.append(name) }
    }
    // Exact checker-merged Pool body regions of the hard-case 40x24 middle
    // original, whose ROI read "Pool unknown: Ø"; the split form uses the
    // rounded probe boxes of the same row.
    let poolMiddle = [
      box("Pool details", 0.07407407658179009, 0.6652892563360882, 0.3009259259259259, 0.04132231404958686),
      box("responses", 0.07903158222906004, 0.6246673987800249, 0.22214313587777149, 0.028680459961595933),
      box("Accounts: 2", 0.07903158693733132, 0.5877925225196434, 0.27305092019541055, 0.03277766755068755),
      box("Usable: 1", 0.0740949037516021, 0.5411720668454241, 0.22738850815855388, 0.039977198444546586),
      box("Reserve: 0", 0.07903158046271362, 0.5058483504837865, 0.2452830458650394, 0.028680459961595933),
      box("Blocked: 1", 0.0744036002778863, 0.4607790564594882, 0.24991102786249295, 0.03277766852753822),
      box("Pool unknown: Ø", 0.07903158559206022, 0.4239041795063751, 0.37023855979628584, 0.028680459961595885),
      box("Excluded: 0", 0.07440359900519129, 0.3747376770565018, 0.2776789198472144, 0.040972085659422786),
      box("Fresh: 0", 0.07422049213096094, 0.3329560007914435, 0.20399742972997986, 0.03849405783309654),
      box("Stale: 1", 0.07888318747881255, 0.29618638670773917, 0.19467203117545667, 0.03418632232363812),
      box("CLose", 0.07399628005432912, 0.2518214195660825, 0.12978521982828775, 0.03354724379610419),
    ]
    func replacingPool(_ text: String, with replacement: String) -> [ScreenTextRegion] {
      poolMiddle.map { $0.text == text ? ScreenTextRegion(text: replacement, bounds: $0.bounds) : $0 }
    }
    func splittingPoolUnknown(_ count: String) -> [ScreenTextRegion] {
      poolMiddle.filter { $0.text != "Pool unknown: Ø" } + [
        box("Pool unknown:", 0.0791, 0.4239, 0.3239, 0.0328), box(count, 0.3984, 0.4239, 0.0509, 0.0328),
      ]
    }
    let poolCases: [(String, [ScreenTextRegion], Bool)] = [
      ("pool-native-slashed-zero-count", poolMiddle, true),
      ("pool-native-split-slashed-zero", splittingPoolUnknown("Ø"), true),
      ("pool-slashed-zero-wrong-count", replacingPool("Blocked: 1", with: "Blocked: Ø"), false),
      ("pool-double-slashed-zero", replacingPool("Pool unknown: Ø", with: "Pool unknown: ØØ"), false),
      ("pool-slashed-zero-then-letter", replacingPool("Pool unknown: Ø", with: "Pool unknown: Øs"), false),
      ("pool-slashed-zero-then-word", replacingPool("Pool unknown: Ø", with: "Pool unknown: Øword"), false),
      ("pool-split-slashed-zero-then-letter", splittingPoolUnknown("Øs"), false),
      ("pool-slashed-zero-then-second-count", replacingPool("Pool unknown: Ø", with: "Pool unknown: Ø 1"), false),
    ]
    for (name, regions, complete) in poolCases {
      let missing = missingPoolCensus(checkpoint: .componentQuotaHardPoolDetailsMiddle, regions: regions)
      if missing.isEmpty != complete { failures.append(name) }
    }
    // modal identity row: first pass "lalpha-tag]" (76x30) or a
    // fullwidth tag, en-US row pass exact. Only
    // the ASCII tag downstream consumers require counts as exact.
    let title = box("Options · Resets", 0.1595, 0.6632, 0.2, 0.0344)
    let misreadRow = box("東京 OpenAI Research Account Alpha Long .. lalpha-tag]", 0.1595, 0.6289, 0.6861, 0.0344)
    let fullwidthRow = box("東京 OpenAI Research Account Alpha Long . ［alpha-tag］", 0.1595, 0.6289, 0.6861, 0.0344)
    let exactRowPass = [box("OpenAI Research Account Alpha Long ... [alpha-tag]", 0.2233, 0.6317, 0.6212, 0.0433)]
    func repaired(_ firstPass: [ScreenTextRegion], _ rowPass: [ScreenTextRegion]) -> [ScreenTextRegion] {
      guard let tagRow = identityTagRowRecognitionBounds(firstPass) else { return firstPass }
      return mergingIdentityTagRow(firstPass: firstPass, row: tagRow.row, rowPass: rowPass)
    }
    func holdsExactTag(_ regions: [ScreenTextRegion]) -> Bool {
      regions.contains { holdsExactAlphaTag($0.text) }
    }
    let tagRowCases: [(String, [ScreenTextRegion], Bool)] = [
      ("tag-row-pass-recovers-exact-tag", repaired([title, misreadRow], exactRowPass), true),
      ("tag-row-fullwidth-first-pass-recovers-ascii", repaired([title, fullwidthRow], exactRowPass), true),
      (
        "tag-row-pass-other-row-ignored",
        repaired([title, misreadRow], [box("[alpha-tag]", 0.2233, 0.50, 0.2, 0.0433)]), false
      ),
      (
        "tag-row-pass-misread-keeps-first-pass",
        repaired(
          [title, misreadRow],
          [box("OpenAI Research Account Alpha Long . ［alpha-tagi", 0.2233, 0.6317, 0.6212, 0.0433)]),
        false
      ),
      (
        "tag-row-pass-fullwidth-not-exact",
        repaired(
          [title, misreadRow],
          [box("OpenAI Research Account Alpha Long . ［alpha-tag］", 0.2233, 0.6317, 0.6212, 0.0433)]),
        false
      ),
      (
        "tag-row-two-candidate-rows-unrepaired",
        repaired([title, misreadRow, box("lalpha-tag]", 0.1595, 0.40, 0.2, 0.0344)], exactRowPass), false
      ),
    ]
    for (name, regions, exact) in tagRowCases {
      if holdsExactTag(regions) != exact { failures.append(name) }
    }
    // A row pass without the exact tag never replaces the first-pass row.
    let retained = repaired(
      [title, misreadRow],
      [box("OpenAI Research Account Alpha Long . ［alpha-tagi", 0.2233, 0.6317, 0.6212, 0.0433)])
    if !retained.contains(where: { $0.text == misreadRow.text && $0.bounds == misreadRow.bounds }) {
      failures.append("tag-row-misread-pass-keeps-first-pass-row")
    }
    // A first pass that already reads the exact ASCII tag is never re-observed.
    if identityTagRowRecognitionBounds([
      title, box("東京 OpenAI Researc.. [alpha-tag]", 0.0741, 0.6198, 0.7870, 0.0455),
    ]) != nil {
      failures.append("tag-row-exact-first-pass-not-repaired")
    }
    // Idle-return eligibility rows whose boxes touch (40x24 native
    // boxes) or whose re-observed tag row reaches into
    // the title row (76x30). Close/Inspect words sit on the measured action row.
    let idle40 = [
      box("Options • Resets", 0.07407407199074077, 0.6652892565426998, 0.39814814814814814, 0.04132231404958675),
      box("*F OpenAI Researc.. [alpha-tag]", 0.07407407731481476, 0.6198347113119835, 0.7870370370370371, 0.045454545454545414),
      box("Eligibility", 0.0740740721230159, 0.5785123965171193, 0.27314814814814814, 0.04132231404958675),
      box("Not checked", 0.06944444726851845, 0.541322314338843, 0.28240740740740744, 0.037190082644628086),
      box("Close", 0.07407407515432098, 0.2520661158402203, 0.12962962962962965, 0.03305785123966942),
      box("Inspect", 0.45, 0.2438, 0.19, 0.0455),
    ]
    let idle76 = [
      box("Options • Resets", 0.15949366981012658, 0.663230240836197, 0.21012658227848102, 0.03436426116838498),
      box("OpenAI Research Account Alpha Long ... [alpha-tag]", 0.21348443174666204, 0.6313205691703485, 0.6311074178125906, 0.03600065455735557),
      box("Eligibility", 0.15945776566833694, 0.5632888788488808, 0.14690725592118278, 0.0349342700132389),
      box("Not checked", 0.15949367210970467, 0.53264604825315, 0.1468354430379747, 0.030927835051546393),
      box("Close", 0.1620, 0.3333, 0.07, 0.0309),
      box("Inspect", 0.37, 0.3333, 0.09, 0.0309),
    ]
    func movingTag(_ regions: [ScreenTextRegion], to y: Double) -> [ScreenTextRegion] {
      regions.map {
        holdsExactAlphaTag($0.text) ? box($0.text, $0.bounds.minX, y, $0.bounds.width, $0.bounds.height) : $0
      }
    }
    let idleCases: [(String, [ScreenTextRegion], Bool)] = [
      ("idle-return-native-touching-rows", idle40, true),
      ("idle-return-row-pass-tag-into-title-row", idle76, true),
      ("idle-return-tag-on-title-row", movingTag(idle40, to: 0.6653), false),
      ("idle-return-tag-below-label", movingTag(idle40, to: 0.50), false),
    ]
    for (name, regions, paired) in idleCases {
      if idleReturnFieldPairs(regions).contains("eligibility not checked") != paired { failures.append(name) }
    }
    // Only the exact-tag oracles re-observe a misread tag row; the three
    // confirmation decision oracles bind "alpha-tag" on the first pass.
    let exactTagMembership: [(ScreenCheckpoint, Bool)] = [
      (.componentQuotaConfirmNoReturned, true), (.componentQuotaCreditsRefreshedObservation, true),
      (.componentQuotaFloorDraft, true), (.componentQuotaPolicyChoice, true),
      (.componentQuotaConfirmNo, false), (.componentQuotaConfirmYes, false),
      (.componentQuotaConfirmHelpReturned, false),
    ]
    for (checkpoint, member) in exactTagMembership
    where exactIdentityTagCheckpoints.contains(checkpoint) != member {
      failures.append("exact-tag-membership-" + checkpoint.rawValue)
    }
    // Help body (40x24 native census): the
    // zh-Hans body pass read the tag row "lalpha-tagl"; the first-pass en-US box
    // of that row read it exactly. Only an exact Alpha tag on that one row
    // completes the identity.
    let alphaHelpIdentity = "東京 OpenAI Research Account Alpha Long Name [alpha-tag]"
    let helpCensus = [
      box("Quota Help", 0.07407407662037035, 0.6611570249586777, 0.25462962962962965, 0.04132231404958675),
      box("Account", 0.07433555335780795, 0.6247647013807921, 0.18062739527972424, 0.03721133409244596),
      box("東京 OpenAI", 0.4954008014405032, 0.6162188022433486, 0.273349261049738, 0.050226648121236075),
      box("Research Account", 0.49092197532533194, 0.5842613972600009, 0.3980064517651321, 0.0366883266703914),
      box("Alpha Long Name", 0.4909219815499675, 0.5353436281725128, 0.37486654177878725, 0.04484128815270063),
      box("lalpha-tagl", 0.4952030435729196, 0.4929073187717313, 0.2691167894615755, 0.048184290745210544),
      box("Current quota", 0.0744036032458605, 0.41304920659011163, 0.3239587398088285, 0.04076480741154602),
      box("Pool", 0.07440359933135915, 0.3763608802254563, 0.10644358593718652, 0.04076480741154602),
      box("Accounts 2 •", 0.4909219833086007, 0.37636087991972017, 0.3008188298224836, 0.03668832667039146),
      box("CLose", 0.07344823520822709, 0.24998997674010348, 0.13088130950927734, 0.03721012950928737),
    ]
    let helpTagBounds = (x: 0.49491741318055166, y: 0.4936293240711944, w: 0.26942443847656244, h: 0.04993143554561397)
    let exactHelpTag = box("[alpha-tag]", helpTagBounds.x, helpTagBounds.y, helpTagBounds.w, helpTagBounds.h)
    let helpCases: [(String, [ScreenTextRegion], Bool)] = [
      ("help-tag-row-recovers-identity", repaired(helpCensus, [exactHelpTag]), true),
      (
        "help-tag-row-wrong-tag",
        repaired(helpCensus, [box("[beta-tag]", helpTagBounds.x, helpTagBounds.y, helpTagBounds.w, helpTagBounds.h)]),
        false
      ),
      (
        "help-tag-row-duplicate-tag",
        repaired(helpCensus, [exactHelpTag, box("[alpha-tag]", 0.78, helpTagBounds.y, 0.14, helpTagBounds.h)]), false
      ),
      (
        "help-tag-row-foreign-row",
        repaired(helpCensus, [box("[alpha-tag]", helpTagBounds.x, 0.30, helpTagBounds.w, helpTagBounds.h)]), false
      ),
    ]
    for (name, regions, matches) in helpCases {
      if contextualHelpAccountIdentityMatches(regions, capturedIdentity: alphaHelpIdentity) != matches {
        failures.append(name)
      }
    }
    // 120x36 native census: the
    // fullwidth tag on the CJK name's row already completes the identity and is
    // never re-observed; the 40x24 misread tag row is.
    let help120 = [
      box("Quota Help", 0.14843255833417612, 0.6903820063962919, 0.08486408277203558, 0.028978107310297818),
      box("Account", 0.15043660873940173, 0.6680783416776295, 0.058703036598408115, 0.019942911298431565),
      box("東京", 0.29067164132386974, 0.6652293546104393, 0.030982158204715414, 0.02564088595512625),
      box(
        "OpenAI Research Account Alpha Long Name ［alpha-tag］", 0.32980699456256424, 0.6623803670256829,
        0.42722765524397016, 0.028489873283473615),
      box("Current quota", 0.15043660965256006, 0.6110985950014711, 0.10762223376374822, 0.022791898626778884),
      box("Close", 0.14841819274280632, 0.277767077812749, 0.04410978009222383, 0.02612773600143825),
    ]
    if !contextualHelpAccountIdentityMatches(help120, capturedIdentity: alphaHelpIdentity) {
      failures.append("help-fullwidth-name-row-identity")
    }
    if helpTagRowRecognitionBounds(help120) != nil {
      failures.append("help-fullwidth-name-row-not-reobserved")
    }
    if helpTagRowRecognitionBounds(helpCensus) == nil {
      failures.append("help-misread-tag-row-reobserved")
    }
    let count: Int =
      dashCases.count + stackedCases.count + paceCases.count + windowCases.count
      + poolCases.count + tagRowCases.count + idleCases.count + exactTagMembership.count
      + helpCases.count + 5
    return (count, failures)
  }
}
