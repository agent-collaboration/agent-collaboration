import Foundation

extension VerifyScreenImage {
  static func checkMatchingControls() throws {
    let cases: [(String, [String], Bool)] = [
      ("split-value", ["Provider: All Disabled:", "Shown 4/4 accounts"], true),
      ("marker-whitespace", ["Provider: All Disabled:\t Shown 4/4 accounts"], true),
      ("wrong-value", ["Provider: All Disabled:", "Hidden 4/4 accounts"], false),
      ("reordered", ["Shown Provider: All Disabled:", "4/4 accounts"], false),
      ("missing-value", ["Provider: All Disabled:", "4/4 accounts"], false),
      ("wrong-count", ["Provider: All Disabled: Shown 4/40 accounts"], false),
      ("forbidden-shortcut", ["Provider: All Disabled: Shown 4/4 accounts", "Ctrl+F"], false),
      ("blank", [], false),
    ]
    var failures: [String] = []
    let refreshedFactCases: [(String, ScreenCheckpoint, [String], Bool)] = [
      ("fresh-fact-value", .componentQuotaCreditsRefreshedFreshness, ["Freshness Fresh"], true),
      ("fresh-fact-stale", .componentQuotaCreditsRefreshedFreshness, ["Freshness Stale"], false),
      ("fresh-label-without-value", .componentQuotaCreditsRefreshedFreshness, ["Freshness"], false),
      (
        "observation-fact-value", .componentQuotaCreditsRefreshedObservation,
        ["Observation checked 0s ago"], true
      ),
      (
        "observation-wrong-value", .componentQuotaCreditsRefreshedObservation,
        ["Observation checked"], false
      ),
      (
        "observation-missing-label", .componentQuotaCreditsRefreshedObservation, ["checked 0s ago"],
        false
      ),
      (
        "observation-reject-old-unknown-fixture", .componentQuotaCreditsRefreshedObservation,
        ["Observation not checked"], false
      ),
      (
        "observation-reject-wrong-age", .componentQuotaCreditsRefreshedObservation,
        ["Observation checked 60s ago"], false
      ),
      ("policy-boundary-heading", .componentQuotaCreditsPolicyBoundary, ["Credit usage"], true),
      (
        "policy-boundary-wrong-view", .componentQuotaCreditsPolicyBoundary, ["Credit balance"],
        false
      ),
    ]
    for (name, checkpoint, lines, shouldPass) in refreshedFactCases {
      let observationRegions =
        checkpoint == .componentQuotaCreditsRefreshedObservation
        ? observationControlRegions(
          lines.joined(separator: " ").replacingOccurrences(of: "Observation ", with: "")
        )
        .filter {
          lines.first?.hasPrefix("Observation ") == true
            || normalizedMarkerText($0.text) != "observation"
        }
        : []
      let result = observe(
        checkpoint: checkpoint,
        recognizedLines: ["Options · Credits", "alpha-tag"] + lines
          + (checkpoint == .componentQuotaCreditsRefreshedObservation ? ["Close"] : []),
        regions: observationRegions)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    let refreshedBalanceCases: [(String, [String], Bool)] = [
      ("refreshed-balance", ["Options · Credits", "alpha-tag", "63,000"], true),
      ("refreshed-stale-value", ["Options · Credits", "alpha-tag", "62,500"], false),
      ("refreshed-both-values", ["Options · Credits", "alpha-tag", "63,000 62,500"], false),
      ("refreshed-compact-value", ["Options · Credits", "alpha-tag", "63k"], false),
      ("refreshed-wrong-page", ["Options · Resets", "alpha-tag", "63,000"], false),
      ("refreshed-wrong-account", ["Options · Credits", "beta-tag", "63,000"], false),
      ("refreshed-larger-value", ["Options · Credits", "alpha-tag", "63,0000"], false),
      ("refreshed-fractional-value", ["Options · Credits", "alpha-tag", "63,000.5"], false),
      ("refreshed-short-value", ["Options · Credits", "alpha-tag", "63,00"], false),
    ]
    for (name, lines, shouldPass) in refreshedBalanceCases {
      let result = observe(
        checkpoint: .componentQuotaCreditsRefreshedBalance, recognizedLines: lines)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    let identityControls = checkIdentityMatchingControls()
    failures += identityControls.failures
    let observationControls = checkObservationMatchingControls()
    failures += observationControls.failures
    let creditChoiceControls = checkCreditChoiceMatchingControls()
    failures += creditChoiceControls.failures
    let lastVoucherCases: [(String, [String], Bool)] = [
      (
        "last-voucher-title",
        ["Options · Resets", "Synthetic reset voucher 7 · available", "Review"], true
      ),
      (
        "last-voucher-wrapped",
        ["Options · Resets", "Synthetic reset", "voucher 7 · available", "Review"], true
      ),
      ("wrong-voucher-six", ["Options · Resets", "Synthetic reset voucher 6", "Review"], false),
      ("wrong-voucher-name", ["Options · Resets", "Other reset voucher 7", "Review"], false),
      (
        "wrong-voucher-seventy", ["Options · Resets", "Synthetic reset voucher 70", "Review"], false
      ),
      (
        "wrong-voucher-suffix", ["Options · Resets", "Synthetic reset voucher 7x", "Review"], false
      ),
      ("missing-voucher-name", ["Options · Resets", "voucher 7", "Review"], false),
      ("wrong-voucher-pane", ["Credit usage", "Synthetic reset voucher 7", "Review"], false),
      ("missing-voucher-review", ["Options · Resets", "Synthetic reset voucher 7"], false),
      ("id-alone-not-title", ["Options · Resets", "voucher-7", "Review"], false),
    ]
    for (name, lines, shouldPass) in lastVoucherCases {
      let result = observe(checkpoint: .componentQuotaResetLastPage, recognizedLines: lines)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    let optionsTitleCases: [(String, ScreenCheckpoint, [String], Bool)] = [
      ("resets-header", .componentQuotaOptionsIdentity, ["Options · Resets", "alpha-tag"], true),
      (
        "credits-bullet-header", .componentQuotaCreditsIdentity,
        ["Options • Credits", "alpha-tag"], true
      ),
      (
        "credits-plain-header", .componentQuotaCreditsIdentity, ["Options Credits", "alpha-tag"],
        true
      ),
      (
        "header-beside-help", .componentQuotaResetInventory, ["Options · Resets Help", "Review"],
        true
      ),
      (
        "wrong-active-resets", .componentQuotaOptionsIdentity,
        ["Options · Credits", "[Resets] [Credits]", "alpha-tag"], false
      ),
      (
        "wrong-active-credits", .componentQuotaCreditsIdentity,
        ["Options · Resets", "[Resets] [Credits]", "alpha-tag"], false
      ),
      (
        "clipped-resets", .componentQuotaResetInventory,
        ["Options · Re", "[Resets] [Credits]", "Review"], false
      ),
      (
        "clipped-credits", .componentQuotaCreditsIdentity,
        ["Options · Cr", "[Resets] [Credits]", "alpha-tag"], false
      ),
      (
        "body-tabs-not-header", .componentQuotaOptionsIdentity,
        ["Options", "[Resets] [Credits]", "alpha-tag"], false
      ),
      (
        "old-caller-title", .componentQuotaOptionsIdentity,
        ["Account options · Resets", "alpha-tag"], false
      ),
      ("missing-options-purpose", .componentQuotaOptionsIdentity, ["Resets", "alpha-tag"], false),
      (
        "split-body-word", .componentQuotaCreditsIdentity, ["Options", "Credits", "alpha-tag"],
        false
      ),
    ]
    for (name, checkpoint, lines, shouldPass) in optionsTitleCases {
      let facts =
        checkpoint == .componentQuotaOptionsIdentity ? ["Live eligibility"] : ["Credit balance"]
      let result = observe(checkpoint: checkpoint, recognizedLines: lines + facts)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    let compactHeader = [
      ScreenTextRegion(
        text: "Quota status", bounds: CGRect(x: 0.02, y: 0.94, width: 0.2, height: 0.03)),
      ScreenTextRegion(
        text: "Accounts 4 · Usable 0 · Pool unknown 1 · Excluded 3 · Data unknown 4",
        bounds: CGRect(x: 0.02, y: 0.85, width: 0.95, height: 0.05)),
      ScreenTextRegion(text: "Account", bounds: CGRect(x: 0.02, y: 0.7, width: 0.2, height: 0.03)),
    ]
    for (name, lines, shouldPass) in cases {
      let recognized = lines.isEmpty ? [] : ["Quota status", "Help", "Claude Delta"] + lines
      let result = observe(
        checkpoint: .compactQuotaAll, recognizedLines: recognized,
        regions: lines.isEmpty ? [] : compactHeader)
      let passed = result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty
      if passed != shouldPass { failures.append(name) }
    }
    let initialChoice = [
      "Account filters", "Help", "Disabled accounts:", "› Shown", "Cancel Select",
    ]
    let hiddenChoice = [
      "Account filters", "Help", "Disabled accounts:", "Shown", "❯Hidden", "Cancel Select",
    ]
    let choiceCases: [(String, ScreenCheckpoint, [String], Bool)] = [
      ("initial-two-row-choice", .compactQuotaDisabledChoice, initialChoice, true),
      ("hidden-focused-choice", .compactQuotaHiddenChoice, hiddenChoice, true),
      ("initial-cannot-prove-hidden-focus", .compactQuotaHiddenChoice, initialChoice, false),
      ("hidden-cannot-prove-initial-focus", .compactQuotaDisabledChoice, hiddenChoice, false),
    ]
    for (name, checkpoint, lines, shouldPass) in choiceCases {
      let result = observe(checkpoint: checkpoint, recognizedLines: lines)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    let providerQueryCases: [(String, [String], Bool)] = [
      (
        "provider-query-visible",
        ["Account provider: zznomatchzz", "No matching choices", "Cancel Select"], true
      ),
      (
        "provider-query-wrapped",
        ["Account provider:", "zznomatchzz", "No matching choices", "Cancel Select"], true
      ),
      (
        "provider-query-wrong-field",
        ["Disabled accounts: zznomatchzz", "No matching choices", "Cancel Select"], false
      ),
      (
        "provider-query-generic-search",
        ["Search: zznomatchzz", "No matching choices", "Cancel Select"], false
      ),
      (
        "provider-query-wrong-query",
        ["Account provider: another", "No matching choices", "Cancel Select"], false
      ),
      (
        "provider-query-premature-apply",
        ["Account provider: zznomatchzz", "No matching choices", "Cancel Apply"], false
      ),
    ]
    for (name, lines, shouldPass) in providerQueryCases {
      let result = observe(
        checkpoint: .compactQuotaProviderNoMatch,
        recognizedLines: currentQueryControlRegions(["Account filters", "Help"] + lines).map(
          \.text),
        regions: currentQueryControlRegions(["Account filters", "Help"] + lines))
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    let chooserChrome = ["Account filters", "Help", "Cancel Apply"]
    let staleSummary = [
      "Account provider · Draft: OpenAI", "Current: Provider: All Disabled: Shown",
    ]
    let fieldCases: [(String, [String], Bool)] = [
      ("two-row-field-values", ["Account provider OpenAI", "Disabled accounts Shown"], true),
      (
        "stale-all-field", staleSummary + ["Account provider All", "Disabled accounts Shown"], false
      ),
      (
        "stale-claude-field",
        staleSummary + ["Account provider Claude", "Disabled accounts Shown"], false
      ),
      (
        "wrong-disabled-field",
        staleSummary + ["Account provider OpenAI", "Disabled accounts Hidden"], false
      ),
      ("closed-prematurely", ["Quota status", "OpenAI", "Shown"], false),
      ("committed-prematurely", ["Quota status", "Provider: OpenAI", "Disabled: Shown"], false),
    ]
    for (name, fields, shouldPass) in fieldCases {
      let chrome = name.hasSuffix("prematurely") ? [] : chooserChrome
      let lines = chrome + fields
      let regions = lines.enumerated().map { index, text in
        ScreenTextRegion(
          text: text,
          bounds: CGRect(x: 0.1, y: 0.1 + Double(index) * 0.04, width: 0.8, height: 0.03))
      }
      let result = observe(
        checkpoint: .compactQuotaChooser, recognizedLines: lines, regions: regions)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    func region(
      _ text: String, _ x: Double, _ y: Double, _ width: Double = 0.2, _ height: Double = 0.0625
    ) -> ScreenTextRegion {
      ScreenTextRegion(text: text, bounds: CGRect(x: x, y: y, width: width, height: height))
    }
    let normalHeader = [
      region("Quota status", 0.02, 0.94, 0.2, 0.03),
      region("Accounts 2 · Usable 2", 0.02, 0.85, 0.7, 0.03),
      region("Account", 0.02, 0.7, 0.2, 0.03),
    ]
    let headerCases: [(String, [ScreenTextRegion], Bool)] = [
      ("header-current-named", normalHeader, true),
      (
        "header-opaque-with-correct-body-words",
        [
          normalHeader[0], normalHeader[2],
          region("Σ 2 · ✓ 2", 0.02, 0.85), region("Accounts 2 · Usable 2", 0.02, 0.5, 0.7),
        ], false
      ),
      (
        "header-wrong-count",
        [
          normalHeader[0], normalHeader[2],
          region("Accounts 20 · Usable 2", 0.02, 0.85, 0.7),
        ], false
      ),
      ("header-contradictory-counts", normalHeader + [region("Accounts 9", 0.02, 0.8)], false),
      ("header-repeated-counts", normalHeader + [region("Accounts 2", 0.02, 0.8)], false),
      ("header-missing-table-boundary", Array(normalHeader.prefix(2)), false),
      (
        "header-body-is-not-header",
        [
          normalHeader[0], normalHeader[2],
          region("Accounts 2 · Usable 2", 0.02, 0.5, 0.7),
        ], false
      ),
      ("header-ignores-conflicting-body", normalHeader + [region("Accounts 99", 0.02, 0.5)], true),
      ("header-duplicate-title", normalHeader + [normalHeader[0]], false),
      ("header-invalid-geometry", normalHeader + [region("Accounts 2", -0.1, 0.8)], false),
    ]
    for (name, regions, shouldPass) in headerCases {
      let passed = missingQuotaHeaderCounts(checkpoint: .componentQuotaBrowse, regions: regions)
        .isEmpty
      if passed != shouldPass { failures.append(name) }
    }
    let labels = [region("Account provider", 0.1, 0.5), region("Disabled accounts", 0.1, 0.375)]
    let values = [region("OpenAI Y", 0.4, 0.5), region("Shown ▾", 0.4, 0.375)]
    let stale = [
      region("Account provider · Draft: OpenAI", 0.1, 0.8, 0.6),
      region("Current: Provider: All Disabled: Shown", 0.1, 0.7, 0.7),
      region("OpenAI", 0.05, 0.1),
    ]
    let geometryCases: [(String, [ScreenTextRegion], Bool)] = [
      ("column-order-valid", labels + values, true),
      ("observed-r-disclosure", labels + [values[0], region("Shown r", 0.4, 0.375)], true),
      ("wrong-r-value", labels + [values[0], region("Hidden r", 0.4, 0.375)], false),
      ("extra-after-r", labels + [values[0], region("Shown r Hidden", 0.4, 0.375)], false),
      (
        "observed-short-disclosure", labels + [values[0], region("Shown •", 0.4, 0.375)], true
      ),
      (
        "wrong-short-disclosure-value",
        stale + labels + [values[0], region("Hidden •", 0.4, 0.375)], false
      ),
      (
        "extra-word-after-short-disclosure",
        labels + [values[0], region("Shown • Hidden", 0.4, 0.375)], false
      ),
      (
        "observed-underlined-disclosure", labels + [values[0], region("Shown =", 0.4, 0.375)],
        true
      ),
      (
        "wrong-value-with-disclosure",
        stale + labels + [values[0], region("Hidden =", 0.4, 0.375)], false
      ),
      (
        "extra-word-after-disclosure",
        labels + [values[0], region("Shown = Hidden", 0.4, 0.375)], false
      ),
      (
        "unsupported-disclosure",
        labels + [values[0], region("Shown ?", 0.4, 0.375)], false
      ),
      ("at-half-overlap", labels + [region("OpenAI", 0.4, 0.53125), values[1]], true),
      ("below-half-overlap", labels + [region("OpenAI", 0.4, 0.531251), values[1]], false),
      (
        "wrong-adjacent-row", labels + [region("OpenAI", 0.4, 0.375), region("Shown", 0.4, 0.5)],
        false
      ),
      ("stale-decorative-all", stale + labels + [region("All", 0.4, 0.5), values[1]], false),
      ("stale-decorative-claude", stale + labels + [region("Claude", 0.4, 0.5), values[1]], false),
      ("stale-summary-shown", stale + labels + [values[0], region("Hidden", 0.4, 0.375)], false),
      (
        "nearest-wrong-value",
        labels + [region("Claude", 0.35, 0.5, 0.1), region("OpenAI", 0.6, 0.5), values[1]], false
      ),
      ("ambiguous-position", labels + values + [region("Claude", 0.4, 0.5)], false),
      ("missing-value-region", labels + [values[1]], false),
      ("degenerate-region", labels + values + [region("OpenAI", 0.4, 0.5, 0)], false),
      ("invalid-region", labels + values + [region("OpenAI", -0.1, 0.5)], false),
      ("nonfinite-region", labels + values + [region("OpenAI", .nan, 0.5)], false),
      ("missing-regions", [], false),
    ]
    for (name, regions, shouldPass) in geometryCases {
      let passed =
        measuredFieldPairs(regions) == ["account provider openai", "disabled accounts shown"]
      if passed != shouldPass { failures.append(name) }
    }
    let policyCases: [(String, [String], Bool)] = [
      ("saved-policy", ["Use credits Allow"], true),
      (
        "cancelled-policy-is-not-saved",
        ["Use credits Disallow", "Allow is limited to eligible requests"], false
      ),
      ("bare-explanation-is-not-policy", ["Allow is limited to eligible requests"], false),
    ]
    for (name, fields, shouldPass) in policyCases {
      let lines = ["Credit usage", "Cancel Save"] + fields
      let regions = lines.enumerated().map { index, text in
        ScreenTextRegion(
          text: text, bounds: CGRect(x: 0.1, y: 0.1 + Double(index) * 0.1, width: 0.8, height: 0.05)
        )
      }
      let result = observe(
        checkpoint: .componentQuotaPolicySaved, recognizedLines: lines, regions: regions)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    let policyLabel = region("Use credits", 0.1, 0.6, 0.3, 0.05)
    let stackedPolicyCases: [(String, [ScreenTextRegion], Bool)] = [
      ("policy-horizontal-value", [policyLabel, region("Allow ▾", 0.5, 0.6, 0.2, 0.05)], true),
      ("policy-stacked-value", [policyLabel, region("Allow ▾", 0.1, 0.53, 0.2, 0.05)], true),
      (
        "policy-wrong-stacked-value", [policyLabel, region("Disallow ▾", 0.1, 0.53, 0.2, 0.05)],
        false
      ),
      ("policy-decorative-header-only", [region("Credit usage Allow", 0.1, 0.8, 0.6)], false),
      ("policy-wrong-field-label", [region("Usage Allow", 0.1, 0.6, 0.6)], false),
      ("policy-wrong-column-below", [policyLabel, region("Allow", 0.5, 0.53, 0.2, 0.05)], false),
      ("policy-too-far-below", [policyLabel, region("Allow", 0.1, 0.3, 0.2, 0.05)], false),
      (
        "policy-nearest-below-must-not-be-skipped",
        [
          region("Use credits", 0.1, 0.6, 0.3, 0.08), region("Wrong", 0.1, 0.55, 0.2, 0.02),
          region("Allow", 0.1, 0.52, 0.2, 0.02),
        ],
        false
      ),
      (
        "policy-wrong-right-not-replaced-by-below",
        [
          policyLabel, region("Disallow", 0.5, 0.6, 0.2, 0.05),
          region("Allow", 0.1, 0.53, 0.2, 0.05),
        ], false
      ),
      (
        "policy-ambiguous-stacked-values",
        [
          policyLabel, region("Allow", 0.1, 0.53, 0.2, 0.05),
          region("Disallow", 0.1, 0.53, 0.2, 0.05),
        ], false
      ),
    ]
    for (name, regions, shouldPass) in stackedPolicyCases {
      if (measuredFieldPairs(regions) == ["use credits allow"]) != shouldPass {
        failures.append(name)
      }
    }
    let expandedPolicyCases: [(String, ScreenCheckpoint, [String], Bool)] = [
      (
        "expanded-policy-select", .componentQuotaPolicyChoice, ["Use credits:", "Cancel Select"],
        true
      ),
      (
        "expanded-policy-save-is-wrong-action", .componentQuotaPolicyChoice,
        ["Use credits:", "Cancel Save"], false
      ),
      (
        "empty-policy-select", .componentQuotaPolicyNoMatch,
        ["Use credits: zznomatchzz", "No matching choices", "Cancel Select"], true
      ),
      (
        "empty-policy-save-is-wrong-action", .componentQuotaPolicyNoMatch,
        ["Use credits: zznomatchzz", "No matching choices", "Cancel Save"], false
      ),
      (
        "empty-policy-missing-field-query", .componentQuotaPolicyNoMatch,
        ["No matching choices", "Cancel Select"], false
      ),
      (
        "empty-policy-wrong-query", .componentQuotaPolicyNoMatch,
        ["Use credits: another", "No matching choices", "Cancel Select"], false
      ),
    ]
    for (name, checkpoint, content, shouldPass) in expandedPolicyCases {
      let choice = checkpoint == .componentQuotaPolicyChoice
      let result = observe(
        checkpoint: checkpoint,
        recognizedLines: ["Credit usage"] + (choice ? ["alpha-tag", "Disallow", "Allow"] : [])
          + content,
        regions: choice ? creditChoiceControlRegions() : [],
        selectedDecision: choice ? "allow" : nil)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    let resetTailCases: [(String, ScreenCheckpoint, [String], Bool)] = [
      (
        "no-return-browse-options", .componentQuotaConfirmNoReturned,
        ["Options Resets", "Close Credits Inspect"], false
      ),
      (
        "no-return-is-not-review", .componentQuotaConfirmNoReturned,
        ["Options Resets", "Cancel Review"], false
      ),
      (
        "returned-with-options-overlay", .componentQuotaReturned,
        ["Quota status OpenAI", "Options Resets Close Inspect"], false
      ),
      (
        "returned-with-result-overlay", .componentQuotaReturned,
        ["Quota status OpenAI", "Options Resets Success reset completed Close"], false
      ),
      ("actual-returned-browse", .componentQuotaReturned, ["Quota status OpenAI"], true),
    ]
    for (name, checkpoint, lines, shouldPass) in resetTailCases {
      let result = observe(checkpoint: checkpoint, recognizedLines: lines, regions: normalHeader)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    let decisionLines = [
      "Confirm reset", "東京 [alpha-tag]", "1 credit. Cannot undo.", "Reset quota? No Yes",
      "Cancel Confirm",
    ]
    let decisionCases: [(String, [String], Bool)] = [
      ("single-decision-group", decisionLines, true),
      ("duplicate-decision-actions", decisionLines + ["No Yes"], false),
      (
        "old-decision-label",
        decisionLines.map { $0.replacingOccurrences(of: "Reset quota?", with: "Consume?") }, false
      ),
      ("decision-missing-risk", decisionLines.filter { !$0.contains("Cannot undo") }, false),
      (
        "decision-wrong-target",
        decisionLines.map { $0.replacingOccurrences(of: "alpha-tag", with: "beta-tag") }, false
      ),
      (
        "decision-background-target",
        ["東京 [alpha-tag]"]
          + decisionLines.map { $0.replacingOccurrences(of: "alpha-tag", with: "beta-tag") }, false
      ),
    ]
    for (name, lines, shouldPass) in decisionCases {
      let regions = lines.enumerated().map { index, text in
        region(text, 0.1, 0.9 - Double(index) * 0.1, 0.8, 0.04)
      }
      let result = observe(
        checkpoint: .componentQuotaConfirmNo, recognizedLines: lines, regions: regions)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    // Retain the measured adjoining Vision edges from the actual No/Yes PNGs.
    // A subpixel rounding difference must not hide a visible risk line, while
    // background, overlapping and below-choice text still cannot qualify it.
    let decisionGeometryCases: [(String, Double, Double, Bool)] = [
      ("decision-risk-touching-choice", 0.5, 0.15, true),
      ("decision-risk-roundoff-above", 0.5000000004, 0.15, true),
      ("decision-risk-roundoff-below", 0.4999999997, 0.15, true),
      ("decision-risk-real-overlap", 0.495, 0.15, false),
      ("decision-risk-below-choice", 0.44, 0.15, false),
      ("decision-risk-background-above-title", 0.85, 0.15, false),
      ("decision-risk-background-left", 0.5, 0.02, false),
    ]
    for (name, riskY, riskX, shouldPass) in decisionGeometryCases {
      let regions = [
        region("Confirm reset", 0.15, 0.7, 0.2, 0.03),
        region("東京 [alpha-tag]", 0.15, 0.65, 0.7, 0.03),
        region("1 credit. Cannot undo.", riskX, riskY, 0.2, 0.03),
        region("Reset quota? No Yes", 0.15, 0.47, 0.2, 0.03),
        region("Cancel Confirm", 0.15, 0.4, 0.2, 0.03),
      ]
      let result = observe(
        checkpoint: .componentQuotaConfirmYes, recognizedLines: regions.map(\.text),
        regions: regions)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    let titleX = 0.09714286108571417
    let titleBottom = 0.665289255785124
    let tagHeight = 0.045454545454545525
    let decisionEdgeCases: [(String, Double, Double, Bool)] = [
      ("current-No-tag-roundoff", 0.09714285191836738, 0.6652892559917356, true),
      ("current-Yes-tag-roundoff", 0.09714285191836738, 0.6652892559917356, true),
      ("tag-exact-top-bound", titleX, titleBottom + 1e-7, true),
      ("tag-exact-left-bound", titleX - 1e-7, titleBottom, true),
      ("tag-beyond-top-bound", titleX, titleBottom + 1.01e-7, false),
      ("tag-beyond-left-bound", titleX - 1.01e-7, titleBottom, false),
      ("tag-real-title-overlap", titleX, titleBottom + 0.005, false),
      ("tag-left-outside", titleX - 0.01, titleBottom, false),
      ("tag-wrong-captured-account", titleX, titleBottom, false),
      ("tag-wrong-row", titleX, 0.3, false),
      ("tag-duplicate-choices", titleX, titleBottom, false),
      ("tag-blank", titleX, titleBottom, false),
    ]
    for (name, tagX, tagTop, shouldPass) in decisionEdgeCases {
      var regions = [
        region("Confirm reset", titleX, titleBottom, 0.40571428571428575, 0.04132231404958686),
        region(
          name == "tag-wrong-captured-account" ? "東京 [beta-tag]" : "*F OpenAI.. [alpha-tag]",
          tagX, tagTop - tagHeight, 0.7314285714285715, tagHeight),
        region(
          "1 credit. Cannot undo.", 0.09714286388571411, 0.37190082652892564,
          0.6742857142857143, 0.04132231404958686),
        region(
          "Reset quota? No Yes", titleX, 0.3305785125737899,
          0.5828571428571429, 0.04132231404958675),
        region("Cancel Confirm", titleX, 0.24, 0.5, 0.04),
      ]
      if name == "tag-duplicate-choices" {
        regions.append(region("No Yes", titleX, 0.3, 0.3, 0.02))
      }
      if name == "tag-blank" { regions = [] }
      let result = observe(
        checkpoint: .componentQuotaConfirmNo,
        recognizedLines: regions.map(\.text), regions: regions)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    let retainedSessionsControls = checkRetainedSessionsControls()
    failures.append(contentsOf: retainedSessionsControls.failures)
    let helpBodyCases: [(String, ScreenCheckpoint, [String], Bool)] = [
      (
        "help-body-shortcuts-are-contextual", .componentQuotaHelpTop,
        ["Quota Help", "Current quota", "Ctrl+F / F3", "Enter / F4", "Close"], true
      ),
      (
        "help-still-needs-close", .componentQuotaHelpTop,
        ["Quota Help", "Current quota", "Ctrl+F / F3"], false
      ),
      (
        "help-bottom-final-instruction", .componentQuotaHelpBottom,
        ["Quota Help", "Ctrl+C / Ctrl+D", "Exit Quota unless a reset is being applied.", "Close"],
        true
      ),
      (
        "help-top-is-not-bottom", .componentQuotaHelpBottom,
        ["Quota Help", "Current quota", "Close"], false
      ),
      (
        "help-old-vague-exit-copy", .componentQuotaHelpBottom,
        ["Quota Help", "Ctrl+C / Ctrl+D", "Existing forced-exit rules still apply", "Close"], false
      ),
      (
        "help-unconditional-exit-copy", .componentQuotaHelpBottom,
        ["Quota Help", "Ctrl+C / Ctrl+D", "Exit Quota", "Close"], false
      ),
      (
        "details-bottom-final-note", .componentQuotaDetailsBottom,
        ["Account details", "Activity", "guards", "note", "alpha detail", "Close"], true
      ),
      (
        "details-top-is-not-bottom", .componentQuotaDetailsBottom,
        ["Account details", "Selected account", "alpha detail", "Close"], false
      ),
      (
        "browse-still-forbids-footer", .componentQuotaBrowse,
        ["Quota status OpenAI 62.5k", "Ctrl+F / F3"], false
      ),
      (
        "floor-still-forbids-footer", .componentQuotaFloorOriginal,
        ["Weekly floor Floor 0% Cancel Save", "Ctrl+E"], false
      ),
    ]
    for (name, checkpoint, lines, shouldPass) in helpBodyCases {
      let result = observe(checkpoint: checkpoint, recognizedLines: lines, regions: normalHeader)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    let currentLabel = region("Current", 0.1, 0.5, 0.2, 0.03)
    let floorAssociationCases: [(String, [ScreenTextRegion], Bool)] = [
      ("saved-floor-contiguous", [region("Current 1%", 0.1, 0.5, 0.5, 0.03)], true),
      ("saved-floor-columns", [currentLabel, region("1%", 0.4, 0.5, 0.1, 0.03)], true),
      (
        "unsaved-floor-draft",
        [currentLabel, region("disabled (0%)", 0.4, 0.5, 0.4, 0.03), region("Floor 1%", 0.1, 0.4)],
        false
      ),
      (
        "saved-floor-wrong-current",
        [currentLabel, region("0%", 0.4, 0.5, 0.1, 0.03), region("Floor 1%", 0.1, 0.4)], false
      ),
      (
        "saved-floor-nearer-wrong-value",
        [currentLabel, region("0%", 0.32, 0.5, 0.1, 0.03), region("1%", 0.5, 0.5, 0.1, 0.03)], false
      ),
      ("saved-floor-off-row-value", [currentLabel, region("1%", 0.4, 0.3, 0.1, 0.03)], false),
      (
        "saved-floor-ambiguous-value",
        [currentLabel, region("0%", 0.4, 0.5, 0.1, 0.03), region("1%", 0.4, 0.5, 0.1, 0.03)], false
      ),
      (
        "saved-floor-conflicting-current",
        [region("Current 1%", 0.1, 0.5), region("Current 0%", 0.1, 0.4)], false
      ),
    ]
    for (name, regions, shouldPass) in floorAssociationCases {
      let result = observe(
        checkpoint: .componentQuotaFloorSaved,
        recognizedLines: ["Weekly floor", "Cancel Save"] + regions.map(\.text), regions: regions)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    let floorHeadingCases: [(String, [String], Bool)] = [
      ("floor-separated-heading", ["Weekly floor", "Floor 0% Cancel Save"], true),
      ("floor-joined-heading", ["Weeklyfloor", "Floor 0% Cancel Save"], true),
      ("floor-spaced-heading", ["Weekly  floor", "Floor 0% Cancel Save"], true),
      ("floor-wrong-heading", ["Monthly floor", "Floor 0% Cancel Save"], false),
      ("floor-missing-heading", ["Floor 0% Cancel Save"], false),
      (
        "floor-background-warning-is-not-heading",
        [
          "Monthly floor", "Warning: Weekly floor 1% blocks credit routing.",
          "Floor 0% Cancel Save",
        ],
        false
      ),
    ]
    for (name, lines, shouldPass) in floorHeadingCases {
      let result = observe(checkpoint: .componentQuotaFloorOriginal, recognizedLines: lines)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    let markerWhitespaceMatches =
      normalizedMarkerText("Disabled:\nShown") == normalizedMarkerText("Disabled:  Shown")
    if !markerWhitespaceMatches { failures.append("expected-marker-whitespace") }
    func focus(
      _ label: String = "hidden", _ labelPixels: Int = 131, _ glyphPixels: Int = 25,
      _ columns: Int = 5, _ rows: Int = 12, _ height: Int = 20
    ) -> ChoiceFocusMeasurement {
      ChoiceFocusMeasurement(
        label: label, labelYellowPixels: labelPixels, glyphYellowPixels: glyphPixels,
        glyphColumns: columns, glyphRows: rows, labelHeight: height)
    }
    let focusCases: [(String, [ChoiceFocusMeasurement], Bool)] = [
      ("original-dropped-chevron", [focus()], true),
      ("bare-keyword", [], false),
      ("missing-glyph", [focus("hidden", 131, 0, 0, 0)], false),
      ("missing-label-color", [focus("hidden", 0)], false),
      ("wrong-row-glyph", [focus("hidden", 131, 25, 5, 0)], false),
      ("wrong-label", [focus("shown")], false),
      ("ambiguous-focus", [focus(), focus("shown")], false),
      ("duplicate-region", [focus(), focus()], false),
      ("at-color-threshold", [focus("hidden", 10, 4, 2, 6)], true),
      ("below-label-threshold", [focus("hidden", 9)], false),
      ("below-glyph-threshold", [focus("hidden", 131, 3)], false),
    ]
    for (name, measurements, shouldPass) in focusCases {
      let result = observe(
        checkpoint: .compactQuotaHiddenChoice,
        recognizedLines: ["Account filters", "Help", "Shown", "Hidden", "Cancel Select"],
        choiceFocusMeasurements: measurements)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != shouldPass {
        failures.append(name)
      }
    }
    let lowerCases: [(ScreenCheckpoint, String, Bool)] = [
      (.componentQuotaBrowseCreditWindow, "62.5k", true),
      (.componentQuotaBrowseCreditWindow, "62,500 5h 99%", false),
      (.componentQuotaBrowseCreditWindow, "5h 99%", false),
      (.componentQuotaBrowsePaceHeading, "5h 99% Weekly pace", true),
      (.componentQuotaBrowsePaceHeading, "Weekly pace", false),
      (.componentQuotaBrowsePaceHeading, "5h 70% Weekly pace", false),
      (.componentQuotaBrowsePaceHeading, "99% Weekly pace", false),
      (.componentQuotaBrowsePaceHeading, "Weekly remaining", false),
      (.componentQuotaBrowsePaceValue, "burn unavailable", true),
      (.componentQuotaBrowsePaceValue, "burn unavaila ble", true),
      (.componentQuotaBrowsePaceValue, "burn temporarily unavailable", false),
      (.componentQuotaBrowsePaceValue, "burn available", false),
      (.componentQuotaOptionsIdentity, "Options · Resets alpha-tag [Resets] [Credits]", false),
      (.componentQuotaCreditsIdentity, "Options · Credits alpha-tag [Resets] [Credits]", false),
    ]
    for (checkpoint, text, expected) in lowerCases {
      let result = observe(
        checkpoint: checkpoint, recognizedLines: ["Quota status", text], regions: normalHeader)
      if (result.missingMarkers.isEmpty && result.unexpectedMarkers.isEmpty) != expected {
        failures.append("lower-context-" + text)
      }
    }
    let floorGuidanceTrap = observe(
      checkpoint: .componentQuotaFloorDraft,
      recognizedLines: ["Weekly floor", "Floor 0%", "Warning: limit 1%", "Cancel Save"])
    if floorGuidanceTrap.missingMarkers.isEmpty && floorGuidanceTrap.unexpectedMarkers.isEmpty {
      failures.append("floor-guidance-cannot-prove-active-value")
    }
    let idleReturnControls = checkIdleReturnMatchingControls()
    failures.append(contentsOf: idleReturnControls.failures)
    let sessionsOwnerControls = checkSessionsOwnerCorrectionControls()
    failures.append(contentsOf: sessionsOwnerControls.failures)
    let compactPaceControls = checkCompactPaceControls()
    failures.append(contentsOf: compactPaceControls.failures)
    let nativeFieldControls = checkNativeFieldMatchingControls()
    failures.append(contentsOf: nativeFieldControls.failures)
    let controlCaseCounts: [Int] = [
      cases.count, choiceCases.count, fieldCases.count, geometryCases.count, focusCases.count,
      policyCases.count, expandedPolicyCases.count, resetTailCases.count, helpBodyCases.count,
      headerCases.count, decisionCases.count, stackedPolicyCases.count, floorHeadingCases.count,
      providerQueryCases.count, floorAssociationCases.count, decisionGeometryCases.count,
      retainedSessionsControls.count, lastVoucherCases.count,
      optionsTitleCases.count,
      identityControls.count,
      observationControls.count,
      creditChoiceControls.count,
      nativeFieldControls.count + 1 + lowerCases.count,
      refreshedBalanceCases.count,
      refreshedFactCases.count,
      decisionEdgeCases.count,
      compactPaceControls.count,
      idleReturnControls.count,
      sessionsOwnerControls.count,
    ]
    let controlCaseCount: Int = controlCaseCounts.reduce(1, +)
    let controlReport: [String: String] = [
      "cases": String(controlCaseCount),
      "failures": failures.joined(separator: ","),
    ]
    let encodedControlReport: Data = try JSONEncoder().encode(controlReport)
    FileHandle.standardOutput.write(encodedControlReport)
    FileHandle.standardOutput.write(Data("\n".utf8))
    if !failures.isEmpty { exit(1) }
  }
}
