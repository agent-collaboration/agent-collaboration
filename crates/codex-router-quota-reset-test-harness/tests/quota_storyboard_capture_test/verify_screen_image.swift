import Foundation
import ImageIO
import Vision

struct ScreenTextRegion {
  let text: String
  let bounds: CGRect
}

struct ChoiceFocusMeasurement: Codable {
  let label: String
  let labelYellowPixels: Int
  let glyphYellowPixels: Int
  let glyphColumns: Int
  let glyphRows: Int
  let labelHeight: Int

  var provesFocus: Bool {
    ["shown", "hidden"].contains(label) && hasIndependentFocusPixels
  }

  var hasIndependentFocusPixels: Bool {
    labelYellowPixels >= 10 && glyphYellowPixels >= 4 && glyphColumns >= 2
      && labelHeight >= 4 && glyphRows >= max(3, labelHeight / 3)
  }
}

// Decode this renderer's original RGB PNG only. Unsupported pixel layouts reject;
// there is no render, color substitution, image export or expected-scene input.
struct OriginalChoicePixels {
  let width: Int
  let height: Int
  let stride: Int
  let bytes: [UInt8]

  init?(url: URL) {
    guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
      let image = CGImageSourceCreateImageAtIndex(source, 0, nil),
      image.bitsPerComponent == 8, image.bitsPerPixel == 32,
      image.colorSpace?.model == .rgb,
      image.bitmapInfo.intersection(.byteOrderMask).isEmpty,
      image.alphaInfo == .noneSkipLast,
      let data = image.dataProvider?.data,
      CFDataGetLength(data) >= image.bytesPerRow * image.height,
      let pointer = CFDataGetBytePtr(data)
    else { return nil }
    width = image.width
    height = image.height
    stride = image.bytesPerRow
    bytes = Array(UnsafeBufferPointer(start: pointer, count: CFDataGetLength(data)))
  }

  func measure(_ region: ScreenTextRegion) -> ChoiceFocusMeasurement? {
    let label = VerifyScreenImage.normalizedMarkerText(region.text)
    let box = region.bounds
    guard ["shown", "hidden", "allow", "disallow"].contains(label),
      [box.minX, box.minY, box.width, box.height].allSatisfy(\.isFinite),
      box.width > 0, box.height > 0, box.minX >= 0, box.minY >= 0,
      box.maxX <= 1, box.maxY <= 1
    else { return nil }
    let left = Int(floor(box.minX * Double(width)))
    let right = Int(ceil(box.maxX * Double(width)))
    let top = Int(floor((1 - box.maxY) * Double(height)))
    let bottom = Int(ceil((1 - box.minY) * Double(height)))
    guard left >= 2 * (bottom - top), right <= width, top >= 0, bottom <= height,
      right > left, bottom > top
    else { return nil }
    var labelPixels = 0
    var glyphPixels = 0
    var columns: Set<Int> = []
    var rows: Set<Int> = []
    for row in top..<bottom {
      for column in (left - 2 * (bottom - top))..<right {
        let index = row * stride + column * 4
        let red = Int(bytes[index])
        let green = Int(bytes[index + 1])
        let blue = Int(bytes[index + 2])
        if red > 150 && green > 150 && blue < 150 && min(red, green) * 2 > blue * 3 {
          if column >= left {
            labelPixels += 1
          } else {
            glyphPixels += 1
            columns.insert(column)
            rows.insert(row)
          }
        }
      }
    }
    return ChoiceFocusMeasurement(
      label: label, labelYellowPixels: labelPixels, glyphYellowPixels: glyphPixels,
      glyphColumns: columns.count, glyphRows: rows.count, labelHeight: bottom - top)
  }
}

struct ScreenImageObservation: Codable {
  let checkpoint: String
  let recognizedLines: [String]
  let recognizedFieldPairs: [String]
  let choiceFocusMeasurements: [ChoiceFocusMeasurement]
  let missingMarkers: [String]
  let unexpectedMarkers: [String]
}

enum ImageVerificationError: Error {
  case invalidArguments
}

struct ImageAnalysisFailure: Codable {
  let kind: String
  let domain: String
  let code: Int
  let message: String
}

@main
struct VerifyScreenImage {
  static func main() throws {
    let arguments = CommandLine.arguments
    if arguments == [arguments[0], "--matching-controls"] {
      try checkMatchingControls()
      return
    }
    guard arguments.count == 3,
      let checkpoint = ScreenCheckpoint(rawValue: arguments[1])
    else {
      throw ImageVerificationError.invalidArguments
    }
    let request = VNRecognizeTextRequest()
    request.recognitionLevel = .accurate
    request.usesLanguageCorrection = false
    request.recognitionLanguages = ["en-US"]
    let handler = VNImageRequestHandler(url: URL(fileURLWithPath: arguments[2]), options: [:])
    do {
      if checkpoint == .componentQuotaConfirmHelpTop
        || checkpoint == .componentQuotaConfirmHelpIdentity
        || checkpoint == .componentQuotaFloorDraft
        || [
          .componentQuotaBrowseCreditWindow, .componentQuotaBrowsePaceHeading,
          .componentQuotaBrowsePaceValue,
        ].contains(checkpoint)
      {
        let languages = ["ja-JP", "en-US"]
        let supported = try request.supportedRecognitionLanguages()
        guard languages.allSatisfy({ supported.contains($0) }) else {
          throw NSError(
            domain: "VerifyScreenImage", code: 1,
            userInfo: [
              NSLocalizedDescriptionKey: "Native Japanese and English recognition unavailable"
            ])
        }
        request.recognitionLanguages =
          checkpoint == .componentQuotaFloorDraft
          ? languages : ["en-US", "ja-JP"]
        request.automaticallyDetectsLanguage = checkpoint != .componentQuotaFloorDraft
      }
      try handler.perform([request])
    } catch {
      let failure = error as NSError
      let diagnostic = ImageAnalysisFailure(
        kind: "image-analysis-unavailable",
        domain: failure.domain,
        code: failure.code,
        message: failure.localizedDescription
      )
      FileHandle.standardError.write(try JSONEncoder().encode(diagnostic))
      FileHandle.standardError.write(Data("\n".utf8))
      exit(2)
    }
    var regions = (request.results ?? []).compactMap { observation -> ScreenTextRegion? in
      guard let text = observation.topCandidates(1).first?.string else { return nil }
      return ScreenTextRegion(text: text, bounds: observation.boundingBox)
    }
    // A misread modal identity tag is re-observed on its own row (see
    // identityTagRowRecognitionBounds) for the oracles that require the exact
    // tag; every other box and checkpoint keeps this pass.
    if exactIdentityTagCheckpoints.contains(checkpoint),
      let tagRow = identityTagRowRecognitionBounds(regions)
    {
      regions = mergingIdentityTagRow(
        firstPass: regions, row: tagRow.row,
        rowPass: recognizedRegions(in: tagRow.bounds, languages: ["en-US"], handler: handler))
    }
    // C6: Pool details judges its census on a second recognition of the
    // dialog body (see poolBodyRecognitionBounds); every other marker keeps
    // the full-screen pass.
    var censusRegions = regions
    if !checkpoint.requiredPoolCensus.isEmpty, let bounds = poolBodyRecognitionBounds(regions) {
      let bodyRequest = VNRecognizeTextRequest()
      bodyRequest.recognitionLevel = .accurate
      bodyRequest.usesLanguageCorrection = false
      bodyRequest.recognitionLanguages = ["en-US"]
      bodyRequest.regionOfInterest = bounds
      // Observation boxes are normalized to the region of interest (probe:
      // raw "Pool details" x0.0203 y0.943 maps to the full-pass x0.1485 y0.6934).
      let bodyPass: [ScreenTextRegion] =
        ((try? handler.perform([bodyRequest])) != nil ? bodyRequest.results ?? [] : [])
        .compactMap { observation in
          guard let text = observation.topCandidates(1).first?.string else { return nil }
          let box = observation.boundingBox
          return ScreenTextRegion(
            text: text,
            bounds: CGRect(
              x: bounds.minX + box.minX * bounds.width, y: bounds.minY + box.minY * bounds.height,
              width: box.width * bounds.width, height: box.height * bounds.height))
        }
      censusRegions = mergingPoolBody(firstPass: regions, bodyPass: bodyPass, bounds: bounds)
    }
    // The full-screen pass reads the CJK account prefix as Latin at 120x36
    // (help identity "*F"); a zh-Hans-first pass over the help body
    // reads the exact name and tag.
    if checkpoint == .componentQuotaConfirmHelpIdentity,
      let bounds = helpBodyRecognitionBounds(regions)
    {
      censusRegions = mergingHelpBody(
        firstPass: regions,
        bodyPass: recognizedRegions(in: bounds, languages: ["zh-Hans", "en-US"], handler: handler),
        bounds: bounds)
      // The zh-Hans body pass can misread the tag brackets (40x24
      // "lalpha-tagl"); a tag the Help oracle cannot read is re-observed in
      // en-US on its row, and replaced only by an exact ASCII tag.
      if let tagRow = helpTagRowRecognitionBounds(censusRegions) {
        censusRegions = mergingIdentityTagRow(
          firstPass: censusRegions, row: tagRow.row,
          rowPass: recognizedRegions(in: tagRow.bounds, languages: ["en-US"], handler: handler))
      }
    }
    let measurements: [ChoiceFocusMeasurement]
    if checkpoint == .compactQuotaHiddenChoice || checkpoint == .compactQuotaDisabledChoice,
      let pixels = OriginalChoicePixels(url: URL(fileURLWithPath: arguments[2]))
    {
      measurements = regions.compactMap { pixels.measure($0) }
    } else {
      measurements = []
    }
    let needsSelectedAccount = [
      .componentQuotaNextAccount, .componentQuotaBrowseCreditWindow,
      .componentQuotaBrowsePaceHeading, .componentQuotaBrowsePaceValue,
      .componentQuotaHardBetaWindows, .componentQuotaHardPoolClosed,
      .componentQuotaHardBetaSelected, .componentQuotaHardBetaExtra,
      .componentQuotaHardPoolClosedTop, .componentQuotaHardBrowseTop,
    ].contains(checkpoint)
    // The first pass reads the selected CJK prefix as Latin (hard
    // 48x12 "FR"; Normal 76x30/40x24 "#R"); a zh-Hans-first
    // full-frame pass reads it exactly. It supplies only the selected-name words of
    // the Alpha-selected poses; recognizedLines and every other marker keep the
    // first pass.
    let firstPassWords = needsSelectedAccount ? recognizedWordRegions(request.results ?? []) : []
    let zhHansAccountWordCheckpoints: [ScreenCheckpoint] = [
        .componentQuotaHardBrowseTop, .componentQuotaBrowseCreditWindow,
        .componentQuotaBrowsePaceHeading, .componentQuotaBrowsePaceValue,
      ]
    let accountWords = zhHansAccountWordCheckpoints.contains(checkpoint)
      ? recognizedWordRegions(languages: ["zh-Hans", "en-US"], handler: handler)
      : firstPassWords
    let windowNameWords: [ScreenTextRegion] =
      checkpoint.requiredWindowLines.isEmpty || uniqueAccountHeader(censusRegions) == nil
      ? []
      : zhHansAccountWordCheckpoints.contains(checkpoint)
        ? accountWords : recognizedWordRegions(languages: ["zh-Hans", "en-US"], handler: handler)
    let accountPixels =
      needsSelectedAccount
      ? OriginalChoicePixels(url: URL(fileURLWithPath: arguments[2])) : nil
    let selectedName = accountPixels.flatMap { pixels in
      selectedAccountName(
        wordRegions: accountWords, pixelWidth: pixels.width,
        hasGutter: { pixels.selectedAccountGutter(beside: $0, header: $1) },
        hasSelectedText: { pixels.yellowPixels(in: $0.bounds) >= 8 },
        hasBlankOverlap: { pixels.selectedWordOverlapIsBlank($0, $1) })
    }
    // C1/C5: the pace value must sit in the selected account's own Weekly pace
    // region at every grid; a whole-image word match never qualifies.
    let selectedPace =
      checkpoint == .componentQuotaBrowsePaceValue
      ? accountPixels.map { pixels in
        // The pace region keeps the first-pass words; only the selected name
        // comes from the zh-Hans words.
        selectedWeeklyPaceMatches(
          words: firstPassWords, selectedName: selectedName,
          expectedName: "東京 OpenAI Research Account Alpha Long Name",
          hasGutter: { pixels.selectedAccountGutter(beside: $0, header: $1) },
          hasSelectedText: { pixels.yellowPixels(in: $0.bounds) >= 8 })
      } ?? false : nil
    let continuation: (name: String, client: String)? =
      checkpoint == .componentQuotaHardAlphaNameContinued ? ("long name", "openai · 1 client")
      : checkpoint == .componentQuotaHardBetaNameContinued ? ("name", "claude · 0 clients") : nil
    let selectedContinuation = continuation.map { expected in
      OriginalChoicePixels(url: URL(fileURLWithPath: arguments[2])).map { pixels in
        selectedNameContinuationMatches(
          regions, continuation: expected.name, clientLine: expected.client,
          hasSelectedText: { pixels.yellowPixels(in: $0.bounds) >= 8 })
      } ?? false
    }
    let creditChoiceWords = checkpoint == .componentQuotaPolicyChoice
      ? recognizedWordRegions(request.results ?? []) : []
    let creditChoiceSelection = checkpoint == .componentQuotaPolicyChoice
      ? OriginalChoicePixels(url: URL(fileURLWithPath: arguments[2])).flatMap {
        selectedCreditChoice(words: creditChoiceWords, rawRows: regions, pixels: $0)
      } : nil
    let observation = observe(
      checkpoint: checkpoint, recognizedLines: regions.map(\.text),
      regions: checkpoint == .componentQuotaConfirmNoReturned
        ? regions.filter { !["close", "inspect"].contains(normalizedMarkerText($0.text)) }
          + recognizedWordRegions(request.results ?? []).filter {
            ["close", "inspect"].contains(normalizedMarkerText($0.text))
          }
        : checkpoint == .componentQuotaCreditsRefreshedObservation
          || checkpoint == .componentQuotaPolicyChoice
          ? regions.filter {
            normalizedMarkerText($0.text).hasPrefix(
              checkpoint == .componentQuotaPolicyChoice ? "credit usage" : "options")
              || normalizedMarkerText($0.text).contains("[alpha-tag]")
          } + (checkpoint == .componentQuotaPolicyChoice
            ? creditChoiceWords.map { word in
              if let selected = creditChoiceSelection,
                normalizedMarkerText(word.text) == normalizedMarkerText(selected.text) {
                return selected
              }
              return word
            } : recognizedWordRegions(request.results ?? [])) : censusRegions,
      choiceFocusMeasurements: measurements,
      selectedAccountName: selectedName,
      selectedPaceField: selectedPace,
      selectedDecision: checkpoint == .componentQuotaConfirmHelpReturned
        || checkpoint == .componentQuotaPolicyChoice
        ? OriginalChoicePixels(url: URL(fileURLWithPath: arguments[2])).flatMap { pixels in
          if checkpoint == .componentQuotaPolicyChoice {
            return creditChoiceSelection.map { normalizedMarkerText($0.text) }
          }
          let choices = recognizedWordRegions(request.results ?? []).filter {
            ["no", "yes"].contains(normalizedMarkerText($0.text))
              && pixels.yellowPixels(in: $0.bounds) >= 8
          }
          return choices.count == 1 ? choices.first.map { normalizedMarkerText($0.text) } : nil
        } : nil,
      selectedFloorDraft: checkpoint == .componentQuotaFloorDraft
        ? OriginalChoicePixels(url: URL(fileURLWithPath: arguments[2])).flatMap { pixels in
          let context = regions.filter {
            normalizedMarkerText($0.text).range(
              of: #"^[0-9]+%$"#,
              options: .regularExpression) == nil
          }
          return selectedFloorDraftValue(
            context + recognizedPercentRegions(request.results ?? []),
            selectedText: { pixels.yellowPixels(in: $0.bounds) >= 8 })
        } : nil,
      selectedRowVisible: [
        .componentQuotaBrowseCreditWindow, .componentQuotaBrowsePaceHeading,
        .componentQuotaBrowsePaceValue, .componentQuotaHardBetaExtra,
      ].contains(checkpoint)
        ? OriginalChoicePixels(url: URL(fileURLWithPath: arguments[2])).flatMap {
          $0.accountSelectorVisible(in: recognizedWordRegions(request.results ?? []))
        } : false,
      selectedContinuationField: selectedContinuation, windowNameWords: windowNameWords)
    FileHandle.standardOutput.write(try JSONEncoder().encode(observation))
    FileHandle.standardOutput.write(Data("\n".utf8))
    if !observation.missingMarkers.isEmpty || !observation.unexpectedMarkers.isEmpty {
      exit(1)
    }
  }

  // One region-of-interest recognition, mapped back to full-image coordinates
  // (observation boxes are normalized to the region of interest).
  static func recognizedRegions(
    in bounds: CGRect, languages: [String], handler: VNImageRequestHandler
  ) -> [ScreenTextRegion] {
    let request = VNRecognizeTextRequest()
    request.recognitionLevel = .accurate
    request.usesLanguageCorrection = false
    request.recognitionLanguages = languages
    request.automaticallyDetectsLanguage = false
    request.regionOfInterest = bounds
    guard (try? handler.perform([request])) != nil else { return [] }
    return (request.results ?? []).compactMap { observation in
      guard let text = observation.topCandidates(1).first?.string else { return nil }
      let box = observation.boundingBox
      return ScreenTextRegion(
        text: text,
        bounds: CGRect(
          x: bounds.minX + box.minX * bounds.width, y: bounds.minY + box.minY * bounds.height,
          width: box.width * bounds.width, height: box.height * bounds.height))
    }
  }

  // One full-frame recognition in the given language order, enumerated into
  // native word boxes.
  static func recognizedWordRegions(
    languages: [String], handler: VNImageRequestHandler
  ) -> [ScreenTextRegion] {
    let request = VNRecognizeTextRequest()
    request.recognitionLevel = .accurate
    request.usesLanguageCorrection = false
    request.recognitionLanguages = languages
    request.automaticallyDetectsLanguage = false
    guard (try? handler.perform([request])) != nil else { return [] }
    return recognizedWordRegions(request.results ?? [])
  }

  static func observe(
    checkpoint: ScreenCheckpoint, recognizedLines: [String], regions: [ScreenTextRegion] = [],
    choiceFocusMeasurements: [ChoiceFocusMeasurement] = [], selectedAccountName: String? = nil,
    selectedPaceField: Bool? = nil, selectedDecision: String? = nil,
    selectedFloorDraft: String? = nil, selectedRowVisible: Bool? = false,
    selectedContinuationField: Bool? = nil, windowNameWords: [ScreenTextRegion] = []
  )
    -> ScreenImageObservation
  {
    let recognizedText = normalizedMarkerText(recognizedLines.joined(separator: "\n"))
    var fieldPairs =
      checkpoint == .componentQuotaConfirmNoReturned
      ? idleReturnFieldPairs(regions)
      : checkpoint == .compactQuotaChooser || checkpoint == .compactSessionsChooser
        || checkpoint == .componentQuotaFloorSaved || checkpoint == .componentQuotaCreditsIdentity
        || checkpoint == .componentQuotaCreditsRefreshedFreshness
        || checkpoint.rawValue.hasPrefix("component-quota-policy-")
        ? measuredFieldPairs(regions)
        : []
    if checkpoint.rawValue.hasPrefix("compact-sessions-details") {
      fieldPairs = sessionsDetailsPairs(regions)
    } else if checkpoint == .compactSessionsNewNoMatch {
      fieldPairs = sessionsChoiceQueryPairs(regions, title: "new session", field: "machine")
    } else if checkpoint == .compactSessionsClientNoMatch {
      fieldPairs = sessionsChoiceQueryPairs(regions, title: "session filters", field: "client")
    } else if checkpoint == .compactSessionsFolderChoice {
      fieldPairs = sessionsChoiceQueryPairs(
        regions, title: "session filters", field: "folder",
        expectedQuery: "checkout", noMatch: false)
    } else if checkpoint == .compactQuotaProviderNoMatch {
      fieldPairs = sessionsChoiceQueryPairs(
        regions, title: "account filters", field: "account provider")
    } else if checkpoint == .componentQuotaPolicyNoMatch {
      fieldPairs = sessionsChoiceQueryPairs(regions, title: "credit usage", field: "use credits")
    }
    let fieldMarkers = [
      "account provider openai", "disabled accounts shown", "use credits allow",
      "use credits disallow", "current 1%", "client codex", "eligibility not checked",
      "freshness fresh",
    ]
    let focused = choiceFocusMeasurements.filter(\.provesFocus)
    let measuredFocus =
      focused.count == 1
        && choiceFocusMeasurements.filter({ $0.label == focused[0].label }).count == 1
      ? focused[0].label : nil
    var missingMarkers = checkpoint.requiredMarkers.filter {
      let marker = normalizedMarkerText($0)
      if checkpoint == .componentQuotaOptionsIdentity, marker == "initial reset task fact" {
        return recognizedText.range(
          of:
            #"\b(?:checking reset eligibility|live eligibility|reset credits)\b|\[voucher-[0-9]+\]"#,
          options: .regularExpression) == nil
      }
      if checkpoint == .componentQuotaCreditsIdentity, marker == "initial credit task fact" {
        return !recognizedText.contains("credit balance")
          && !fieldPairs.contains("use credits disallow")
      }
      // Vision may enumerate "Weekly" and "pace" as separate boxes with other
      // rows between them (40x24); the heading is one terminal row.
      if checkpoint == .componentQuotaBrowsePaceHeading, marker == "weekly pace" {
        return !recognizedText.contains(marker)
          && !(terminalRows(regions)?.contains { terminalRowText($0).contains(marker) } ?? false)
      }
      if checkpoint == .componentQuotaBrowsePaceValue, marker == "burn unavailable" {
        if let selectedPaceField { return !selectedPaceField }
        // Body wrapping may split a word; retain every letter and order.
        return recognizedText.range(
          of: #"\bburn\s+u\s*n\s*a\s*v\s*a\s*i\s*l\s*a\s*b\s*l\s*e\b"#,
          options: .regularExpression) == nil
      }
      if checkpoint == .componentQuotaFloorDraft, marker == "1%" {
        return selectedFloorDraft != "1%"
      }
      if checkpoint == .componentQuotaCreditsRefreshedObservation,
        marker == "observation checked 0s ago"
      {
        return !observationFieldIsZero(regions)
      }
      if [
        .componentQuotaNextAccount, .componentQuotaHardBetaWindows, .componentQuotaHardPoolClosed,
      ].contains(checkpoint),
        marker == "claude research account beta full name"
      {
        return selectedAccountName.map(normalizedMarkerText) != marker
      }
      // Hard 48x12 top pose (one body row): exactly the measured selected
      // prefix; the continuation and Details tag complete the identity.
      if [.componentQuotaHardBetaSelected, .componentQuotaHardPoolClosedTop].contains(checkpoint),
        marker == "claude research account beta full"
      {
        return selectedAccountName.map(normalizedMarkerText) != marker
      }
      if checkpoint == .componentQuotaCreditsRefreshedBalance, marker == "63,000" {
        return recognizedText.range(
          of: #"(?<![0-9,.])63,000(?![0-9,]|\.[0-9])"#, options: .regularExpression
        ) == nil
      }
      if checkpoint == .componentQuotaConfirmHelpTop,
        ["selected account", "account"].contains(marker)
      {
        let expected = marker == "account" ? "selected account account" : marker
        return contextualHelpBody(regions)?.contains(expected) != true
      }
      if checkpoint == .componentQuotaConfirmHelpIdentity,
        marker == "東京 openai research account alpha long name [alpha-tag]"
      {
        return !contextualHelpAccountIdentityMatches(regions, capturedIdentity: marker)
      }
      if let pageTitle = checkpoint.optionsPageTitle,
        marker == normalizedMarkerText(pageTitle)
      {
        let expected = normalizedMarkerText(
          pageTitle.replacingOccurrences(of: "[·•]", with: " ", options: .regularExpression))
        // The inactive tab is also present in body content. Require the whole
        // active header on one OCR line, allowing its adjacent Help action.
        return !recognizedLines.contains { line in
          let title = normalizedMarkerText(
            line.replacingOccurrences(of: "[·•]", with: " ", options: .regularExpression))
          return title == expected || title == expected + " help"
        }
      }
      if checkpoint == .componentQuotaResetLastPage, marker == "synthetic reset voucher 7" {
        return recognizedText.range(
          of: #"\bsynthetic reset voucher 7\b"#, options: .regularExpression
        ) == nil
      }
      if marker == "weekly floor",
        [
          .componentQuotaFloorOriginal, .componentQuotaFloorDraft,
          .componentQuotaFloorCancelled, .componentQuotaFloorSaved,
        ].contains(checkpoint)
      {
        // Vision joins this observed heading's words. Match the exact heading
        // line, so a background warning cannot stand in for a missing title.
        return !recognizedLines.contains {
          ["weekly floor", "weeklyfloor"].contains(normalizedMarkerText($0))
        }
      }
      if fieldMarkers.contains(marker) || marker.hasPrefix("details ") || marker.hasPrefix("field ")
      {
        return !fieldPairs.contains(marker)
      }
      if (checkpoint == .compactQuotaHiddenChoice && marker == "› hidden")
        || (checkpoint == .compactQuotaDisabledChoice && marker == "› shown")
      {
        return !recognizedText.contains(marker) && measuredFocus != String(marker.dropFirst(2))
      }
      return !recognizedText.contains(marker)
    }
    missingMarkers += missingPoolCensus(checkpoint: checkpoint, regions: regions)
    missingMarkers += missingWindowLines(
      checkpoint.requiredWindowLines, regions: regions, nameWords: windowNameWords)
    if checkpoint == .componentQuotaPolicyChoice,
      !expandedCreditChoiceMatches(regions, selected: selectedDecision)
    {
      missingMarkers.append("selected Allow in active credit choice")
    }
    if checkpoint == .componentQuotaConfirmHelpReturned, selectedDecision != "yes" {
      missingMarkers.append("preserved selected Yes")
    }
    if checkpoint == .componentQuotaConfirmNo || checkpoint == .componentQuotaConfirmYes
      || checkpoint == .componentQuotaConfirmHelpReturned
    {
      let titles = regions.filter { normalizedMarkerText($0.text) == "confirm reset" }
      let choices = regions.filter { normalizedMarkerText($0.text).hasPrefix("reset quota?") }
      let context: String
      if titles.count == 1, choices.count == 1, let title = titles.first,
        let choice = choices.first,
        title.bounds.minY > choice.bounds.maxY
      {
        context = normalizedMarkerText(
          regions.filter {
            // Actual current No/Yes tag edges differ from the title by 9.17e-9
            // horizontally and 2.07e-10 vertically. The1e-7 bound is normalized
            // image space: under0.000049pixel at350x484, never a real row overlap.
            // Keep the independently measured previous lower-edge bound intact.
            $0.bounds.maxY <= title.bounds.minY + 0.0000001
              && $0.bounds.minY >= choice.bounds.maxY - 0.000001
              && $0.bounds.minX >= title.bounds.minX - 0.0000001
          }.sorted { $0.bounds.midY > $1.bounds.midY }.map(\.text).joined(separator: " "))
      } else {
        context = ""
      }
      for marker in ["alpha-tag", "1 credit. cannot undo."] {
        if !context.contains(marker) { missingMarkers.append("decision context " + marker) }
      }
      // Fixed fixture background names contain neither word. This catches the
      // previous duplicated body/action controls; pointer/style proof is separate.
      for choice in ["no", "yes"] {
        let expression = try? NSRegularExpression(pattern: "\\b" + choice + "\\b")
        let count = expression?.numberOfMatches(
          in: recognizedText, range: NSRange(recognizedText.startIndex..., in: recognizedText))
        if count != 1 { missingMarkers.append("single decision choice " + choice) }
      }
    }
    if let expectedCount = checkpoint.exactCompactCount {
      let pattern =
        "(?<![0-9])" + NSRegularExpression.escapedPattern(for: expectedCount) + "(?![0-9])"
      if recognizedText.range(of: pattern, options: .regularExpression) == nil {
        missingMarkers.append("exact count " + expectedCount)
      }
    }
    if [
      .componentQuotaBrowseCreditWindow, .componentQuotaBrowsePaceHeading,
      .componentQuotaBrowsePaceValue,
    ].contains(checkpoint),
      selectedAccountName.map(normalizedMarkerText) == "claude research account beta full name"
    {
      // A wide next-account frame can retain Alpha's unselected facts. The
      // captured lower-content journey belongs to Alpha, never selected Beta.
      missingMarkers.append("captured Alpha account; foreign selected Beta")
    }
    if [
      .componentQuotaBrowseCreditWindow, .componentQuotaBrowsePaceHeading,
      .componentQuotaBrowsePaceValue,
    ].contains(checkpoint),
      selectedRowVisible == nil
        || (selectedRowVisible == true
          && selectedAccountName.map(compactHelpIdentity)
            != compactHelpIdentity("東京 OpenAI Research Account Alpha Long Name"))
    {
      missingMarkers.append("visible selected account is missing or ambiguous")
    }
    // C1: Beta's Extra pose belongs to selected Beta. A wide or tall browse with
    // Alpha selected also shows Beta's Extra cell, so a readable selection must
    // be Beta; an unreadable selection passes only when the selector column is
    // measured empty (a scrolled short pose relying on Beta-only values and
    // continuity). Visible-but-unreadable or unknown visibility rejects.
    if checkpoint == .componentQuotaHardBetaExtra {
      let selected = selectedAccountName.map(normalizedMarkerText)
      if (selected != nil && selected != "claude research account beta full name")
        || (selected == nil && selectedRowVisible != false)
      {
        missingMarkers.append("captured Beta account; foreign or unreadable selection")
      }
    }
    if checkpoint == .componentQuotaHardBrowseTop,
      selectedAccountName.map(compactHelpIdentity)
        != compactHelpIdentity("東京 OpenAI Research Account Alpha")
    {
      missingMarkers.append("selected Alpha name prefix")
    }
    if [.componentQuotaHardAlphaNameContinued, .componentQuotaHardBetaNameContinued]
      .contains(checkpoint), selectedContinuationField != true
    {
      missingMarkers.append("selected name continuation")
    }
    var unexpectedMarkers = checkpoint.forbiddenMarkers.filter {
      // A background "New" column and the next observation's "Session
      // filters" heading are different painted rows, not a New session dialog.
      if $0 == "New session", !regions.isEmpty {
        guard regionsHaveValidBounds(regions), let rows = terminalRows(regions) else { return true }
        return rows.contains { terminalRowText($0).contains("new session") }
      }
      return recognizedText.contains(normalizedMarkerText($0))
    }
    if checkpoint.forbidsBrowseCensus {
      unexpectedMarkers += browseCensusLeaks(regions).map { "browse census " + $0 }
    }
    if [
      .compactSessionsDetails, .compactSessionsDetailsTitle, .compactSessionsDetailsIdentity,
      .compactSessionsDetailsState, .compactSessionsDetailsPath,
    ].contains(checkpoint), labelsEnterAsFixedResume(recognizedText) {
      unexpectedMarkers.append("fixed enter resume")
    }
    return ScreenImageObservation(
      checkpoint: checkpoint.rawValue,
      recognizedLines: recognizedLines,
      recognizedFieldPairs: fieldPairs,
      choiceFocusMeasurements: choiceFocusMeasurements,
      missingMarkers: missingMarkers,
      unexpectedMarkers: unexpectedMarkers
    )
  }

  // Vision can enumerate label and value columns separately. Bind only these
  // actual fields; retain native recognition order in the diagnostic output.
  static func measuredFieldPairs(_ regions: [ScreenTextRegion]) -> [String] {
    guard !regions.isEmpty,
      regions.allSatisfy({ region in
        let box = region.bounds
        return [box.minX, box.minY, box.width, box.height].allSatisfy(\.isFinite)
          && box.width > 0 && box.height > 0 && box.minX >= 0 && box.minY >= 0
          && box.maxX <= 1 && box.maxY <= 1
      })
    else { return [] }
    let fields = [
      ("account provider", ["all", "openai", "claude"]),
      ("disabled accounts", ["shown", "hidden"]),
      ("use credits", ["allow", "disallow"]),
      ("current", ["disabled (0%)", "0%", "1%"]),
      ("client", ["all providers", "codex", "claude code", "cursor"]),
      ("eligibility", ["not checked"]),
      ("freshness", ["fresh", "stale", "unknown"]),
    ]
    // Vision can split a label into adjacent native boxes on the same row.
    // Reconstruct only an exact known label, with no skipped or distant word.
    let splitLabels: [ScreenTextRegion] = (terminalRows(regions) ?? []).compactMap { row in
      let label = terminalRowText(row)
      guard row.count > 1, fields.contains(where: { $0.0 == label }),
        let first = row.first, let last = row.last
      else { return nil }
      let cell = (last.bounds.maxX - first.bounds.minX) / Double(label.count)
      guard zip(row, row.dropFirst()).allSatisfy({ left, right in
        right.bounds.minX - left.bounds.maxX >= -0.1 * cell
          && right.bounds.minX - left.bounds.maxX <= 1.5 * cell
      }) else { return nil }
      return ScreenTextRegion(text: label, bounds: row.dropFirst().reduce(first.bounds) {
        $0.union($1.bounds)
      })
    }
    let fieldRegions = regions + splitLabels
    return fields.compactMap { label, allowed in
      var values: Set<String> = []
      for region in fieldRegions {
        let text = normalizedMarkerText(region.text)
        if text.hasPrefix(label + " "),
          let value = fieldValue(String(text.dropFirst(label.count + 1)), allowed: allowed)
        {
          values.insert(value)
        } else if text == label && region.bounds.height > 0 {
          let right = fieldRegions.filter { candidate in
            let overlap =
              min(region.bounds.maxY, candidate.bounds.maxY)
              - max(region.bounds.minY, candidate.bounds.minY)
            return candidate.bounds.minX >= region.bounds.maxX
              && overlap >= 0.5 * min(region.bounds.height, candidate.bounds.height)
              && candidate.bounds.height > 0
          }.sorted { $0.bounds.minX < $1.bounds.minX }
          // Narrow FormFields stack their value below the label, in its column
          // or indented up to two cells (40x24 Use credits +1.8
          // cells); a value box may touch the label row, so its centre must lie
          // below the label. Never skip an intervening same-column observation.
          let cell = region.bounds.width / Double(max(1, label.count))
          let below =
            ["account provider", "disabled accounts", "use credits", "client", "eligibility", "freshness"].contains(label) && right.isEmpty
            ? fieldRegions.filter { candidate in
              candidate.bounds.midY < region.bounds.minY
                && region.bounds.minY - candidate.bounds.maxY
                  <= max(region.bounds.height, candidate.bounds.height)
                && candidate.bounds.minX - region.bounds.minX
                  >= -0.1 * min(region.bounds.width, candidate.bounds.width)
                && candidate.bounds.minX - region.bounds.minX <= 2.5 * cell
            }.sorted { $0.bounds.midY > $1.bounds.midY }
            : []
          let candidates = right.isEmpty ? below : right
          // Do not skip a wrong nearest value to find a convenient word behind
          // it. Equal-position competing observations are ambiguous and reject.
          if let nearest = candidates.first,
            candidates.dropFirst().allSatisfy({ candidate in
              right.isEmpty
                ? abs(candidate.bounds.midY - nearest.bounds.midY) > 0.001
                : abs(candidate.bounds.minX - nearest.bounds.minX) > 0.001
            }
            ),
            let value = fieldValue(normalizedMarkerText(nearest.text), allowed: allowed)
          {
            values.insert(value)
          }
        }
      }
      guard values.count == 1, let value = values.first else { return nil }
      return label + " " + value
    }
  }

  static func fieldValue(_ text: String, allowed: [String]) -> String? {
    let words = text.split(separator: " ").map(String.init)
    if allowed.contains(text) { return text }
    // A collapsed choice paints one disclosure glyph. These are the actual
    // observed Vision readings of it (underlined ▾ read as '=', '•' or ':';
    // 76x30 read the provider's ▾ as '-'); another word/value is
    // never accepted.
    if words.count == 2 && allowed.contains(words[0])
      && ["▾", "▼", "⌄", "v", "y", "r", "=", "•", ":", "-"].contains(words[1])
    {
      return words[0]
    }
    // The underlined disclosure can also attach to the value with no space
    // (calibration-004 chooser read "Shown:"). Letter readings never attach.
    if words.count == 1, let glyph = text.last,
      ["▾", "▼", "⌄", "=", "•", ":"].contains(String(glyph)),
      allowed.contains(String(text.dropLast()))
    {
      return String(text.dropLast())
    }
    return nil
  }

  static func normalizedMarkerText(_ text: String) -> String {
    text
      .replacingOccurrences(of: ">", with: "›")
      .replacingOccurrences(of: "❯", with: "›")
      .replacingOccurrences(
        of: "›[ \t]*(Shown|Hidden)\\b", with: "› $1", options: .regularExpression
      )
      // Vision sometimes joins the chevron and label at the narrow width.
      // Preserve the selection marker and full Allow token; never accept
      // the pane's unrelated explanatory text as a selected draft.
      .replacingOccurrences(of: "›[ \t]*Allow\\b", with: "› Allow", options: .regularExpression)
      .replacingOccurrences(
        of: "›[ \t]*Disallow\\b", with: "› Disallow", options: .regularExpression
      )
      // OCR can split the count and unit into adjacent observations even
      // when they occupy one painted toolbar row. Normalize only this
      // literal count/unit pair; keep draft and provider markers intact.
      .replacingOccurrences(
        of: "([0-9]+/[0-9]+)[ \t\r\n]+(accounts|sessions)\\b", with: "$1 $2",
        options: .regularExpression
      )
      // Vision can split this single subdued label inside one observation.
      // Repair this split token before normalizing inter-word OCR whitespace.
      .replacingOccurrences(
        of: "\\bHe[ \\t]+lp\\b", with: "Help", options: [.regularExpression, .caseInsensitive]
      )
      // Keep every token and its recognition order. Consumers that bind one
      // painted row must use native region geometry before matching it.
      .replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression)
      .trimmingCharacters(in: .whitespacesAndNewlines)
      .lowercased()
  }

}
