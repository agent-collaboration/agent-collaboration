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
    ["shown", "hidden"].contains(label) && labelYellowPixels >= 10
      && glyphYellowPixels >= 4 && glyphColumns >= 2
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
    guard ["shown", "hidden"].contains(label),
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
    let regions = (request.results ?? []).compactMap { observation -> ScreenTextRegion? in
      guard let text = observation.topCandidates(1).first?.string else { return nil }
      return ScreenTextRegion(text: text, bounds: observation.boundingBox)
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
    ].contains(checkpoint)
    let accountWords = needsSelectedAccount ? recognizedWordRegions(request.results ?? []) : []
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
    let compactPace =
      checkpoint == .componentQuotaBrowsePaceValue
      ? accountPixels.flatMap { pixels in
        compactPaceFieldMatches(
          words: accountWords, pixelWidth: pixels.width,
          pixelHeight: pixels.height, selectedName: selectedName)
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
          } + recognizedWordRegions(request.results ?? []) : regions,
      choiceFocusMeasurements: measurements,
      selectedAccountName: selectedName,
      compactPaceField: compactPace,
      selectedDecision: checkpoint == .componentQuotaConfirmHelpReturned
        || checkpoint == .componentQuotaPolicyChoice
        ? OriginalChoicePixels(url: URL(fileURLWithPath: arguments[2])).flatMap { pixels in
          if checkpoint == .componentQuotaPolicyChoice {
            let words = recognizedWordRegions(request.results ?? [])
            let actions = words.filter { normalizedMarkerText($0.text) == "select" }
            guard actions.count == 1, let action = actions.first,
              pixels.enabledTextPixels(in: action.bounds) >= 8
            else { return nil }
            let focused = words.filter {
              ["allow", "disallow"].contains(normalizedMarkerText($0.text))
                && pixels.selectedNameGutter(beside: $0)
            }
            return focused.count == 1 ? focused.first.map { normalizedMarkerText($0.text) } : nil
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
        .componentQuotaBrowsePaceValue,
      ].contains(checkpoint)
        ? OriginalChoicePixels(url: URL(fileURLWithPath: arguments[2])).flatMap {
          $0.accountSelectorVisible(in: recognizedWordRegions(request.results ?? []))
        } : false)
    FileHandle.standardOutput.write(try JSONEncoder().encode(observation))
    FileHandle.standardOutput.write(Data("\n".utf8))
    if !observation.missingMarkers.isEmpty || !observation.unexpectedMarkers.isEmpty {
      exit(1)
    }
  }

  static func observe(
    checkpoint: ScreenCheckpoint, recognizedLines: [String], regions: [ScreenTextRegion] = [],
    choiceFocusMeasurements: [ChoiceFocusMeasurement] = [], selectedAccountName: String? = nil,
    compactPaceField: Bool? = nil, selectedDecision: String? = nil,
    selectedFloorDraft: String? = nil, selectedRowVisible: Bool? = false
  )
    -> ScreenImageObservation
  {
    let recognizedText = normalizedMarkerText(recognizedLines.joined(separator: "\n"))
    var fieldPairs =
      checkpoint == .componentQuotaConfirmNoReturned
      ? idleReturnFieldPairs(regions)
      : checkpoint == .compactQuotaChooser || checkpoint == .compactSessionsChooser
        || checkpoint == .componentQuotaFloorSaved || checkpoint == .componentQuotaCreditsIdentity
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
    }
    let fieldMarkers = [
      "account provider openai", "disabled accounts shown", "use credits allow",
      "use credits disallow", "current 1%", "client codex", "eligibility not checked",
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
      if checkpoint == .componentQuotaBrowsePaceValue, marker == "burn unavailable" {
        if let compactPaceField { return !compactPaceField }
        // Wider body wrapping may split a word; retain every letter and order.
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
      if checkpoint == .componentQuotaNextAccount,
        marker == "claude research account beta full name"
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
    missingMarkers += missingQuotaHeaderCounts(checkpoint: checkpoint, regions: regions)
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
    let unexpectedMarkers = checkpoint.forbiddenMarkers.filter {
      recognizedText.contains(normalizedMarkerText($0))
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

  // Bound the census to the actual surface header between its title and table
  // headings. Incidental words/values in rows or an overlay cannot qualify it.
  static func missingQuotaHeaderCounts(
    checkpoint: ScreenCheckpoint, regions: [ScreenTextRegion]
  ) -> [String] {
    let expected = checkpoint.requiredQuotaHeaderCounts
    guard !expected.isEmpty else { return [] }
    guard
      regions.allSatisfy({ region in
        let box = region.bounds
        return [box.minX, box.minY, box.width, box.height].allSatisfy(\.isFinite)
          && box.width > 0 && box.height > 0 && box.minX >= 0 && box.minY >= 0
          && box.maxX <= 1 && box.maxY <= 1
      })
    else { return expected.map { "header " + $0 } }
    let titles = regions.filter { normalizedMarkerText($0.text).hasPrefix("quota status") }
    guard titles.count == 1, let title = titles.first,
      let table = regions.filter({ region in
        let text = normalizedMarkerText(region.text)
        return (text == "account" || text == "account status quota")
          && region.bounds.maxY < title.bounds.minY
      }).max(by: { $0.bounds.maxY < $1.bounds.maxY })
    else { return expected.map { "header " + $0 } }
    let header = regions.filter {
      $0.bounds.minY > table.bounds.maxY && $0.bounds.maxY <= title.bounds.maxY + 0.0000001
    }.sorted {
      $0.bounds.midY > $1.bounds.midY
    }
    // Native boxes on one terminal row have slightly different vertical
    // centers. Keep that row together before ordering its fragments by x;
    // a count on another row must never complete an unfinished label.
    var headerRows: [[ScreenTextRegion]] = []
    for region in header {
      let matchingRows = headerRows.indices.filter { index in
        headerRows[index].allSatisfy { sibling in
          let overlap = min(region.bounds.maxY, sibling.bounds.maxY)
            - max(region.bounds.minY, sibling.bounds.minY)
          return overlap >= min(region.bounds.height, sibling.bounds.height) / 2
        }
      }
      guard matchingRows.count <= 1 else { return expected.map { "header " + $0 } }
      if let index = matchingRows.first {
        headerRows[index].append(region)
      } else {
        headerRows.append([region])
      }
    }
    let rowTexts = headerRows.map { row in
      normalizedMarkerText(
        row.sorted { $0.bounds.minX < $1.bounds.minX }.map(\.text).joined(separator: " "))
    }
    return expected.compactMap { association in
      let parts = association.split(separator: " ")
      let label = parts.dropLast().joined(separator: " ")
      let count = String(parts.last ?? "")
      let pattern =
        "(?<![a-z0-9])" + NSRegularExpression.escapedPattern(for: label)
        + "[ \\t]+([0-9]+)(?![0-9])"
      guard let expression = try? NSRegularExpression(pattern: pattern) else {
        return "header " + association
      }
      let values = rowTexts.flatMap { text in
        expression.matches(in: text, range: NSRange(text.startIndex..., in: text))
          .compactMap { match -> String? in
            guard let range = Range(match.range(at: 1), in: text) else { return nil }
            return String(text[range])
          }
      }
      return values == [count] ? nil : "header " + association
    }
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
    ]
    return fields.compactMap { label, allowed in
      var values: Set<String> = []
      for region in regions {
        let text = normalizedMarkerText(region.text)
        if text.hasPrefix(label + " "),
          let value = fieldValue(String(text.dropFirst(label.count + 1)), allowed: allowed)
        {
          values.insert(value)
        } else if text == label && region.bounds.height > 0 {
          let right = regions.filter { candidate in
            let overlap =
              min(region.bounds.maxY, candidate.bounds.maxY)
              - max(region.bounds.minY, candidate.bounds.minY)
            return candidate.bounds.minX >= region.bounds.maxX
              && overlap >= 0.5 * min(region.bounds.height, candidate.bounds.height)
              && candidate.bounds.height > 0
          }.sorted { $0.bounds.minX < $1.bounds.minX }
          // Narrow Use credits and Client FormFields stack their value directly
          // below the label. Never skip an intervening same-column observation.
          let below =
            ["use credits", "client", "eligibility"].contains(label) && right.isEmpty
            ? regions.filter { candidate in
              candidate.bounds.maxY <= region.bounds.minY
                && region.bounds.minY - candidate.bounds.maxY
                  <= max(region.bounds.height, candidate.bounds.height)
                && abs(candidate.bounds.minX - region.bounds.minX)
                  <= 0.1 * min(region.bounds.width, candidate.bounds.width)
            }.sorted { $0.bounds.maxY > $1.bounds.maxY }
            : []
          let candidates = right.isEmpty ? below : right
          // Do not skip a wrong nearest value to find a convenient word behind
          // it. Equal-position competing observations are ambiguous and reject.
          if let nearest = candidates.first,
            candidates.dropFirst().allSatisfy({ candidate in
              right.isEmpty
                ? abs(candidate.bounds.maxY - nearest.bounds.maxY) > 0.001
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
    // observed Vision readings of it (underlined ▾ read as '=', '•' or ':');
    // another word/value is never accepted.
    if words.count == 2 && allowed.contains(words[0])
      && ["▾", "▼", "⌄", "v", "y", "r", "=", "•", ":"].contains(words[1])
    {
      return words[0]
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
      // Vision may split one painted row into adjacent observations. Keep
      // every token and its order; apply identical whitespace semantics to
      // required and forbidden markers so this cannot join isolated words.
      .replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression)
      .trimmingCharacters(in: .whitespacesAndNewlines)
      .lowercased()
  }

}
