import Foundation

enum ScreenCheckpoint: String {
  case browse
  case resets
  case credits
  case disallowDraft = "disallow-draft"
  case allowDraft = "allow-draft"
  case cancelled
  case returned
  case quotaAllTop = "quota-all-top"
  case quotaAllNext = "quota-all-next"
  case quotaOpenaiBottom = "quota-openai-bottom"
  case quotaClaudeBottom = "quota-claude-bottom"
  case quotaAll = "quota-all"
  case quotaChooser = "quota-chooser"
  case quotaOpenai = "quota-openai"
  case quotaClaude = "quota-claude"
  case quotaHidden = "quota-hidden"
  case quotaDraft = "quota-draft"
  case quotaCancelled = "quota-cancelled"
  case quotaRestored = "quota-restored"
  case quotaAllBottom = "quota-all-bottom"
  case quotaEmpty = "quota-empty"
  case sessionsCodexBottom = "sessions-codex-bottom"
  case sessionsClaudeBottom = "sessions-claude-bottom"
  case sessionsNewChoice = "sessions-new-choice"
  case sessionsAll = "sessions-all"
  case sessionsChooser = "sessions-chooser"
  case sessionsCodex = "sessions-codex"
  case sessionsClaude = "sessions-claude"
  case sessionsCursor = "sessions-cursor"
  case sessionsDraft = "sessions-draft"
  case sessionsCancelled = "sessions-cancelled"
  case sessionsEmpty = "sessions-empty"
  case sessionsRestored = "sessions-restored"
  case sessionsAllBottom = "sessions-all-bottom"
  case sessionsDetails = "sessions-details"
  case sessionsDetailsClosed = "sessions-details-closed"
  case quotaDetails = "quota-details"
  case quotaDetailsClosed = "quota-details-closed"
  case compactSessionsAll = "compact-sessions-all"
  case compactSessionsChooser = "compact-sessions-chooser"
  case compactSessionsHelp = "compact-sessions-help"
  case compactSessionsClaude = "compact-sessions-claude"
  case compactSessionsCancelled = "compact-sessions-cancelled"
  case compactSessionsDetails = "compact-sessions-details"
  case compactSessionsCodexDetails = "compact-sessions-codex-details"
  case compactSessionsFork = "compact-sessions-fork"
  case compactSessionsForkChoice = "compact-sessions-fork-choice"
  case compactSessionsNew = "compact-sessions-new"
  case compactSessionsNewMachine = "compact-sessions-new-machine"
  case compactSessionsNewNoMatch = "compact-sessions-new-no-match"
  case compactSessionsNewReturned = "compact-sessions-new-returned"
  case compactSessionsMachineSources = "compact-sessions-machine-sources"
  case compactSessionsClientNoMatch = "compact-sessions-client-no-match"
  case compactSessionsFolderChoice = "compact-sessions-folder-choice"
  case compactSessionsModelProviderChoice = "compact-sessions-model-provider-choice"
  case compactSessionsSourceChoice = "compact-sessions-source-choice"
  case compactSessionsRuntimeChoice = "compact-sessions-runtime-choice"
  case compactSessionsSortChoice = "compact-sessions-sort-choice"
  case compactQuotaAll = "compact-quota-all"
  case compactQuotaProviderNoMatch = "compact-quota-provider-no-match"
  case compactQuotaDisabledChoice = "compact-quota-disabled-choice"
  case compactQuotaHiddenChoice = "compact-quota-hidden-choice"
  case compactQuotaChooser = "compact-quota-chooser"
  case compactQuotaHelp = "compact-quota-help"
  case compactQuotaClaude = "compact-quota-claude"
  case compactQuotaCancelled = "compact-quota-cancelled"
  case compactQuotaDetails = "compact-quota-details"

  case componentQuotaBrowse = "component-quota-browse"
  case componentQuotaBrowseCreditWindow = "component-quota-browse-credit-window"
  case componentQuotaBrowsePaceHeading = "component-quota-browse-pace-heading"
  case componentQuotaBrowsePaceValue = "component-quota-browse-pace-value"
  case componentQuotaBrowsePageReturned = "component-quota-browse-page-returned"
  case componentQuotaNextAccount = "component-quota-next-account"
  case componentQuotaRestoredAccount = "component-quota-restored-account"
  case componentQuotaDetailsTop = "component-quota-details-top"
  case componentQuotaDetailsBottom = "component-quota-details-bottom"
  case componentQuotaHelpTop = "component-quota-help-top"
  case componentQuotaHelpBottom = "component-quota-help-bottom"
  case componentQuotaFloorOriginal = "component-quota-floor-original"
  case componentQuotaFloorDraft = "component-quota-floor-draft"
  case componentQuotaFloorCancelled = "component-quota-floor-cancelled"
  case componentQuotaFloorSaved = "component-quota-floor-saved"
  case componentQuotaOptionsIdentity = "component-quota-options-identity"
  case componentQuotaResetInventory = "component-quota-reset-inventory"
  case componentQuotaResetLastPage = "component-quota-reset-last-page"
  case componentQuotaCreditsIdentity = "component-quota-credits-identity"
  case componentQuotaCreditsBalance = "component-quota-credits-balance"
  case componentQuotaCreditsRefreshedBalance = "component-quota-credits-refreshed-balance"
  case componentQuotaCreditsRefreshedFreshness = "component-quota-credits-refreshed-freshness"
  case componentQuotaCreditsRefreshedObservation = "component-quota-credits-refreshed-observation"
  case componentQuotaCreditsPolicyBoundary = "component-quota-credits-policy-boundary"
  case componentQuotaPolicyOriginal = "component-quota-policy-original"
  case componentQuotaPolicyChoice = "component-quota-policy-choice"
  case componentQuotaPolicyNoMatch = "component-quota-policy-no-match"
  case componentQuotaPolicyChoiceReturned = "component-quota-policy-choice-returned"
  case componentQuotaPolicyDraft = "component-quota-policy-draft"
  case componentQuotaPolicyCancelled = "component-quota-policy-cancelled"
  case componentQuotaPolicySaved = "component-quota-policy-saved"
  case componentQuotaRefreshResult = "component-quota-refresh-result"
  case componentQuotaResetReview = "component-quota-reset-review"
  case componentQuotaConfirmNo = "component-quota-confirm-no"
  case componentQuotaConfirmNoReturned = "component-quota-confirm-no-returned"
  case componentQuotaConfirmYes = "component-quota-confirm-yes"
  case componentQuotaConfirmHelpTop = "component-quota-confirm-help-top"
  case componentQuotaConfirmHelpIdentity = "component-quota-confirm-help-identity"
  case componentQuotaConfirmHelpReturned = "component-quota-confirm-help-returned"
  case componentQuotaResetResult = "component-quota-reset-result"
  case componentQuotaResetResultBottom = "component-quota-reset-result-bottom"
  case componentQuotaReturned = "component-quota-returned"

  // These are the current bound fixtures' pool projection, not account-row words.
  // Zero detail counters on constrained screens remain a contextual Help duty.
  var requiredQuotaHeaderCounts: [String] {
    switch self {
    case .componentQuotaBrowse, .componentQuotaBrowseCreditWindow, .componentQuotaBrowsePaceHeading,
      .componentQuotaBrowsePaceValue, .componentQuotaBrowsePageReturned,
      .componentQuotaNextAccount, .componentQuotaRestoredAccount, .componentQuotaReturned:
      ["accounts 2", "usable 2"]
    case .compactQuotaAll, .compactQuotaClaude, .compactQuotaCancelled:
      ["accounts 4", "usable 0", "pool unknown 1", "excluded 3", "data unknown 4"]
    default:
      []
    }
  }

  var requiredMarkers: [String] {
    switch self {
    case .componentQuotaBrowse, .componentQuotaBrowsePageReturned:
      ["Quota status", "OpenAI"]
    case .componentQuotaBrowseCreditWindow:
      ["Quota status", "62.5k"]
    case .componentQuotaBrowsePaceHeading:
      ["Quota status", "5h", "99%", "Weekly pace"]
    case .componentQuotaBrowsePaceValue:
      ["Quota status", "burn unavailable"]
    case .componentQuotaNextAccount:
      ["Quota status", "Claude Research Account Beta Full Name", "Claude"]
    case .componentQuotaRestoredAccount:
      ["Quota status", "OpenAI"]
    case .componentQuotaDetailsTop:
      ["Account details", "Status", "Reason", "Close"]
    case .componentQuotaDetailsBottom:
      ["Account details", "note", "alpha detail", "Close"]
    case .componentQuotaHelpTop:
      ["Quota Help", "Current quota", "Close"]
    case .componentQuotaHelpBottom:
      ["Quota Help", "Exit Quota unless a reset is being applied.", "Close"]
    case .componentQuotaFloorOriginal:
      ["Weekly floor", "Floor", "0%", "Cancel", "Save"]
    case .componentQuotaFloorDraft:
      ["Weekly floor", "Floor", "1%", "Cancel", "Save"]
    case .componentQuotaFloorCancelled:
      ["Weekly floor", "Floor", "0%", "Cancel", "Save"]
    case .componentQuotaFloorSaved:
      ["Weekly floor", "Current 1%", "Cancel", "Save"]
    case .componentQuotaOptionsIdentity:
      ["Options · Resets", "alpha-tag", "initial reset task fact"]
    case .componentQuotaResetInventory:
      ["Options · Resets", "Review"]
    case .componentQuotaResetLastPage:
      ["Options · Resets", "Synthetic reset voucher 7", "Review"]
    case .componentQuotaCreditsIdentity:
      ["Options · Credits", "alpha-tag", "initial credit task fact"]
    case .componentQuotaCreditsBalance:
      ["Options · Credits", "62,500"]
    case .componentQuotaCreditsRefreshedBalance:
      ["Options · Credits", "alpha-tag", "63,000"]
    case .componentQuotaCreditsRefreshedFreshness:
      ["Options · Credits", "alpha-tag", "Freshness Fresh"]
    case .componentQuotaCreditsRefreshedObservation:
      ["Options · Credits", "alpha-tag", "Observation checked 0s ago"]
    case .componentQuotaCreditsPolicyBoundary:
      ["Options · Credits", "alpha-tag", "Credit usage"]
    case .componentQuotaPolicyOriginal:
      ["Credit usage", "Use credits Disallow", "Cancel", "Save"]
    case .componentQuotaPolicyChoice:
      ["Credit usage", "alpha-tag", "Allow", "Cancel", "Select"]
    case .componentQuotaPolicyNoMatch:
      ["Credit usage", "Use credits: zznomatchzz", "No matching choices", "Cancel", "Select"]
    case .componentQuotaPolicyChoiceReturned:
      ["Credit usage", "Use credits Disallow", "Cancel", "Save"]
    case .componentQuotaPolicyDraft:
      ["Credit usage", "Use credits Allow", "Cancel", "Save"]
    case .componentQuotaPolicyCancelled:
      ["Credit usage", "Use credits Disallow", "Cancel", "Save"]
    case .componentQuotaPolicySaved:
      ["Credit usage", "Use credits Allow", "Cancel", "Save"]
    case .componentQuotaRefreshResult:
      ["Options · Credits", "Credit balance refreshed."]
    case .componentQuotaResetReview:
      ["Options · Resets", "Review"]
    case .componentQuotaConfirmNo:
      [
        "Confirm reset", "alpha-tag", "1 credit. Cannot undo.", "Reset quota?", "No", "Yes",
        "Cancel",
        "Confirm",
      ]
    case .componentQuotaConfirmNoReturned:
      ["Options · Resets", "alpha-tag", "eligibility not checked", "Close", "Inspect"]
    case .componentQuotaConfirmYes, .componentQuotaConfirmHelpReturned:
      [
        "Confirm reset", "alpha-tag", "1 credit. Cannot undo.", "Reset quota?", "No", "Yes",
        "Cancel",
        "Confirm",
      ]
    case .componentQuotaConfirmHelpTop:
      ["Quota Help", "Selected account", "Account", "Close"]
    case .componentQuotaConfirmHelpIdentity:
      ["Quota Help", "東京 OpenAI Research Account Alpha Long Name [alpha-tag]", "Close"]
    case .componentQuotaResetResult:
      ["Reset result", "alpha-tag", "Success", "reset completed", "Close"]
    case .componentQuotaResetResultBottom:
      ["Reset result", "alpha-tag", "Return to quota status.", "Close"]
    case .componentQuotaReturned:
      ["Quota status", "OpenAI"]
    case .compactSessionsAll:
      ["Sessions", "Help", "This machine", "All providers", "6/6", "Codex Alpha"]
    case .compactSessionsChooser:
      ["Session filters", "Help", "Client Codex", "Apply", "Cancel"]
    case .compactSessionsHelp:
      ["Sessions Help", "Session filters", "Move field focus", "Close"]
    case .compactSessionsCodexDetails:
      ["Session details", "Codex Alpha", "Resume", "Fork", "Close"]
    case .compactSessionsFork:
      ["Fork session", "Cancel", "Fork"]
    case .compactSessionsForkChoice:
      ["Fork session", "Destination:", "Cancel", "Select"]
    case .compactSessionsNew:
      ["New session", "Client", "Codex", "Cancel", "Start"]
    case .compactSessionsNewMachine:
      ["New session", "Machine:", "Cancel", "Select"]
    case .compactSessionsNewNoMatch:
      ["New session", "Machine: zznomatchzz", "No matching choices", "Cancel", "Select"]
    case .compactSessionsNewReturned:
      ["New session", "Machine", "Cancel", "Start"]
    case .compactSessionsMachineSources:
      ["Choose machine", "Machine:", "Cancel", "Select"]
    case .compactSessionsClientNoMatch:
      ["Session filters", "Client: zznomatchzz", "No matching choices", "Cancel", "Select"]
    case .compactSessionsFolderChoice:
      ["Session filters", "Folder: Checkout", "Cancel", "Select"]
    case .compactSessionsModelProviderChoice:
      ["Session filters", "Model provider:", "Any model provider", "Cancel", "Select"]
    case .compactSessionsSourceChoice:
      ["Session filters", "Source:", "All sources", "Cancel", "Select"]
    case .compactSessionsRuntimeChoice:
      ["Session filters", "Runtime:", "Active", "Cancel", "Select"]
    case .compactSessionsSortChoice:
      ["Session filters", "Sort:", "Updated", "Cancel", "Select"]
    case .compactSessionsClaude, .compactSessionsCancelled:
      ["Sessions", "Help", "Claude Code", "2/6", "Claude Gamma", "Details"]
    case .compactSessionsDetails:
      ["Session details", "Help", "Claude Gamma", "Session ID", "Endpoint", "State", "Close"]
    case .compactQuotaAll:
      ["Quota status", "Help", "All", "Disabled: Shown", "4/4", "Claude Delta"]
    case .compactQuotaProviderNoMatch:
      [
        "Account filters", "Help", "Account provider: zznomatchzz", "No matching choices", "Select",
        "Cancel",
      ]
    case .compactQuotaDisabledChoice:
      ["Account filters", "Help", "Disabled accounts:", "› Shown", "Select", "Cancel"]
    case .compactQuotaHiddenChoice:
      ["Account filters", "Help", "Shown", "› Hidden", "Select", "Cancel"]
    case .compactQuotaChooser:
      [
        "Account filters", "Help", "Account provider OpenAI", "Disabled accounts Shown",
        "Apply", "Cancel",
      ]
    case .compactQuotaHelp:
      ["Quota Help", "Account filters", "Move field", "Close"]
    case .compactQuotaClaude, .compactQuotaCancelled:
      ["Quota status", "Help", "Claude", "Disabled: Hidden", "1/4", "Claude Gamma", "Details"]
    case .compactQuotaDetails:
      ["Account details", "Help", "Status", "Reason", "Claude Gamma", "Close"]
    case .sessionsDetails:
      ["Session details", "Codex Alpha", "Session ID", "Conversation", "Close"]
    case .sessionsDetailsClosed:
      ["Codex", "2/6", "Codex Alpha", "Details F4"]
    case .quotaDetails:
      ["Account details", "Claude Gamma", "Status", "Reason", "Close"]
    case .quotaDetailsClosed:
      ["Claude", "Disabled: Hidden", "1/4 accounts", "Details F4"]
    case .quotaAllTop:
      ["Quota status", "All", "4/4 accounts", "Claude Delta"]
    case .quotaAllNext:
      ["Quota status", "All", "4/4 accounts", "Claude Gamma"]
    case .quotaOpenaiBottom:
      ["Quota status", "OpenAI", "2/4 accounts", "OpenAI Beta"]
    case .quotaClaudeBottom:
      ["Quota status", "Claude", "2/4 accounts", "Claude Gamma"]
    case .quotaAll:
      ["Quota status", "All", "Disabled: Shown", "4/4 accounts"]
    case .quotaChooser:
      [
        "Quota status", "Account provider", "Draft: All", "Disabled accounts: Shown", "Apply",
        "Cancel",
      ]
    case .quotaOpenai:
      ["Quota status", "OpenAI", "2/4 accounts", "OpenAI Alpha"]
    case .quotaClaude:
      ["Quota status", "Claude", "2/4 accounts", "Claude Delta"]
    case .quotaHidden:
      ["Quota status", "Claude", "Disabled: Hidden", "1/4 accounts", "Claude Gamma"]
    case .quotaDraft:
      [
        "Quota status", "Claude", "Disabled: Hidden", "1/4 accounts", "Draft: OpenAI",
        "Disabled accounts: Hidden", "Apply", "Cancel",
      ]
    case .quotaCancelled:
      ["Quota status", "Claude", "Disabled: Hidden", "1/4 accounts", "Claude Gamma"]
    case .quotaRestored:
      ["Quota status", "All", "Disabled: Shown", "4/4 accounts"]
    case .quotaAllBottom:
      ["Quota status", "All", "4/4 accounts", "OpenAI Beta"]
    case .quotaEmpty:
      [
        "Quota status", "Claude", "Disabled: Hidden", "0/4 accounts",
        "No accounts match these filters",
      ]
    case .sessionsCodexBottom:
      ["Sessions", "Codex", "2/6", "Codex Beta"]
    case .sessionsClaudeBottom:
      ["Sessions", "Claude Code", "2/6", "Claude Delta"]
    case .sessionsNewChoice:
      ["Sessions", "Cursor", "1/6", "New Codex session"]
    case .sessionsAll:
      ["Sessions", "All providers", "6/6"]
    case .sessionsChooser:
      [
        "Session client provider", "Draft: All providers", "Apply", "Cancel", "Claude Code",
        "Cursor",
      ]
    case .sessionsCodex:
      ["Sessions", "Codex", "2/6", "Codex Alpha"]
    case .sessionsClaude:
      ["Sessions", "Claude Code", "2/6", "Claude Gamma"]
    case .sessionsCursor:
      ["Sessions", "Cursor", "1/6", "Cursor Echo"]
    case .sessionsDraft:
      ["Session client provider", "Draft: Codex", "Cursor", "1/6", "Apply", "Cancel"]
    case .sessionsCancelled:
      ["Sessions", "Cursor", "1/6", "Cursor Echo"]
    case .sessionsEmpty:
      ["Sessions", "Cursor", "0/6", "New Codex session"]
    case .sessionsRestored:
      ["Sessions", "All providers", "6/6"]
    case .sessionsAllBottom:
      ["Sessions", "All providers", "6/6", "Claude-lookalike Zeta"]
    case .browse, .returned:
      ["Quota status", "pty-alpha", "account options"]
    case .resets:
      ["Quota status", "Reset credits", "Weekly remaining"]
    case .credits, .cancelled:
      ["Quota status", "Credit balance", "42.00", "Disallow", "enter edit"]
    case .allowDraft:
      ["Quota status", "Credit usage", "42.00", "› Allow", "enter save"]
    case .disallowDraft:
      ["Quota status", "Credit usage", "42.00", "› Disallow", "enter save"]
    }
  }

  var optionsPageTitle: String? {
    switch self {
    case .componentQuotaOptionsIdentity, .componentQuotaResetInventory,
      .componentQuotaResetLastPage, .componentQuotaResetReview, .componentQuotaConfirmNoReturned:
      "Options · Resets"
    case .componentQuotaCreditsIdentity, .componentQuotaCreditsBalance,
      .componentQuotaCreditsRefreshedBalance, .componentQuotaRefreshResult:
      "Options · Credits"
    case .componentQuotaCreditsRefreshedFreshness, .componentQuotaCreditsRefreshedObservation,
      .componentQuotaCreditsPolicyBoundary:
      "Options · Credits"
    default: nil
    }
  }

  // Compact chrome intentionally omits the unit at narrow widths. Enforce the
  // complete count token independently so 4/40 can never satisfy 4/4.
  var exactCompactCount: String? {
    switch self {
    case .compactSessionsAll: "6/6"
    case .compactSessionsClaude, .compactSessionsCancelled: "2/6"
    case .compactQuotaAll: "4/4"
    case .compactQuotaClaude, .compactQuotaCancelled: "1/4"
    default: nil
    }
  }

  var forbiddenMarkers: [String] {
    switch self {
    case .componentQuotaHelpTop, .componentQuotaHelpBottom,
      .componentQuotaConfirmHelpTop, .componentQuotaConfirmHelpIdentity:
      []
    case .componentQuotaReturned:
      hiddenShortcutMarkers + [
        "Options", "Account details", "Credit usage", "Weekly floor", "Quota Help",
        "Confirm reset", "Checking reset", "Reset in progress", "Reset result", "Success", "[No]",
        "[Yes]",
      ]
    case .componentQuotaConfirmNoReturned:
      hiddenShortcutMarkers + ["Review", "Confirm reset", "Reset result", "[No]", "[Yes]"]
    case .componentQuotaCreditsRefreshedBalance:
      hiddenShortcutMarkers + ["62,500"]
    case .componentQuotaCreditsRefreshedFreshness, .componentQuotaCreditsRefreshedObservation,
      .componentQuotaCreditsPolicyBoundary:
      hiddenShortcutMarkers
    case .componentQuotaDetailsTop, .componentQuotaDetailsBottom:
      hiddenShortcutMarkers + ["Selected account"]
    case .componentQuotaPolicyNoMatch, .componentQuotaPolicyChoiceReturned, .componentQuotaBrowse,
      .componentQuotaBrowseCreditWindow, .componentQuotaBrowsePaceHeading,
      .componentQuotaBrowsePaceValue, .componentQuotaBrowsePageReturned, .componentQuotaNextAccount, .componentQuotaRestoredAccount,
      .componentQuotaFloorOriginal, .componentQuotaFloorDraft,
      .componentQuotaFloorCancelled, .componentQuotaFloorSaved, .componentQuotaOptionsIdentity,
      .componentQuotaResetInventory, .componentQuotaResetLastPage, .componentQuotaCreditsIdentity,
      .componentQuotaCreditsBalance, .componentQuotaPolicyOriginal, .componentQuotaPolicyChoice,
      .componentQuotaPolicyDraft, .componentQuotaPolicyCancelled, .componentQuotaPolicySaved,
      .componentQuotaRefreshResult, .componentQuotaResetReview, .componentQuotaConfirmNo,
      .componentQuotaConfirmYes, .componentQuotaResetResult, .componentQuotaResetResultBottom:
      hiddenShortcutMarkers
    case .componentQuotaConfirmHelpReturned:
      hiddenShortcutMarkers + ["Quota Help"]
    case .compactSessionsCodexDetails:
      ["Session filters", "New session", "Fork session"]
    case .compactSessionsFork:
      ["Search:", "New session", "Session filters"]
    case .compactSessionsForkChoice:
      ["New session", "Session filters"]
    case .compactSessionsNew, .compactSessionsNewReturned:
      ["Search:", "No matching choices", "Session filters", "Fork session"]
    case .compactSessionsNewMachine:
      ["No matching choices", "Session filters", "Fork session"]
    case .compactSessionsNewNoMatch:
      ["Session filters", "Fork session"]
    case .compactSessionsMachineSources:
      ["New session", "Session filters", "Fork session"]
    case .compactSessionsClientNoMatch, .compactSessionsFolderChoice,
      .compactSessionsModelProviderChoice, .compactSessionsSourceChoice,
      .compactSessionsRuntimeChoice, .compactSessionsSortChoice:
      ["New session", "Fork session", "Sessions Help"]
    case .compactSessionsAll:
      hiddenShortcutMarkers + ["Session filters", "Search:", "View:", "Sort:"]
    case .compactSessionsChooser:
      hiddenShortcutMarkers + ["Sessions Help", "Quota Help"]
    case .compactSessionsHelp:
      ["Quota Help", "Apply  Cancel"]
    case .compactSessionsClaude, .compactSessionsCancelled:
      hiddenShortcutMarkers + [
        "Codex Alpha", "Codex Beta", "Cursor Echo", "Session client provider", "Draft:",
      ]
    case .compactSessionsDetails:
      hiddenShortcutMarkers + ["Draft:", "Session client provider", "Sessions Help"]
    case .compactQuotaAll:
      hiddenShortcutMarkers + ["Account provider", "Quota Help"]
    case .compactQuotaProviderNoMatch:
      hiddenShortcutMarkers + ["Quota Help", "Sessions Help"]
    case .compactQuotaDisabledChoice:
      hiddenShortcutMarkers + ["Quota Help", "Sessions Help", "No matching choices", "› Hidden"]
    case .compactQuotaHiddenChoice:
      hiddenShortcutMarkers + ["Quota Help", "Sessions Help", "No matching choices", "› Shown"]
    case .compactQuotaChooser:
      hiddenShortcutMarkers + ["Quota Help", "Sessions Help", "Search:", "No matching choices"]
    case .compactQuotaHelp:
      ["Sessions Help", "Draft:"]
    case .compactQuotaClaude, .compactQuotaCancelled:
      hiddenShortcutMarkers + ["Claude De", "OpenAI Al", "OpenAI Be", "Account provider", "Draft:"]
    case .compactQuotaDetails:
      hiddenShortcutMarkers + ["Draft:", "Account provider", "Quota Help", "Selected account"]
    case .sessionsDetails, .quotaDetails:
      ["Draft:", "Apply", "Cancel"]
    case .sessionsDetailsClosed, .quotaDetailsClosed:
      ["Session details", "Account details", "Draft:"]
    case .quotaAllTop, .quotaAllNext:
      ["Account provider"]
    case .quotaOpenaiBottom:
      ["Claude Gamma", "Claude Delta", "Account provider"]
    case .quotaClaudeBottom:
      ["OpenAI Alpha", "OpenAI Beta", "Account provider"]
    case .quotaAll:
      ["Account provider"]
    case .quotaChooser:
      []
    case .quotaOpenai:
      ["Claude Gamma", "Claude Delta", "Account provider"]
    case .quotaClaude:
      ["OpenAI Alpha", "OpenAI Beta", "Account provider"]
    case .quotaHidden:
      ["Claude Delta", "OpenAI Alpha", "OpenAI Beta", "Account provider"]
    case .quotaDraft:
      ["Claude Delta"]
    case .quotaCancelled:
      ["Account provider", "Claude Delta", "OpenAI Alpha", "OpenAI Beta"]
    case .quotaRestored:
      ["Account provider"]
    case .quotaAllBottom:
      ["Account provider"]
    case .quotaEmpty:
      ["Claude Gamma", "Claude Delta", "OpenAI Alpha", "OpenAI Beta", "Account provider"]
    case .sessionsCodexBottom:
      [
        "Claude Gamma", "Claude Delta", "Cursor Echo", "Claude-lookalike Zeta",
        "Session client provider",
      ]
    case .sessionsClaudeBottom:
      [
        "Codex Alpha", "Codex Beta", "Cursor Echo", "Claude-lookalike Zeta",
        "Session client provider",
      ]
    case .sessionsNewChoice:
      ["Session client provider", "Codex Alpha", "Codex Beta", "Claude Gamma", "Claude Delta"]
    case .sessionsAll:
      ["Session client provider"]
    case .sessionsChooser:
      []
    case .sessionsCodex:
      [
        "Claude Gamma", "Claude Delta", "Cursor Echo", "Claude-lookalike Zeta",
        "Session client provider",
      ]
    case .sessionsClaude:
      [
        "Codex Alpha", "Codex Beta", "Cursor Echo", "Claude-lookalike Zeta",
        "Session client provider",
      ]
    case .sessionsCursor:
      [
        "Codex Alpha", "Codex Beta", "Claude Gamma", "Claude Delta", "Claude-lookalike Zeta",
        "Session client provider",
      ]
    case .sessionsDraft:
      ["Codex Alpha", "Codex Beta", "Claude Gamma", "Claude Delta"]
    case .sessionsCancelled:
      ["Session client provider", "Codex Alpha", "Codex Beta", "Claude Gamma", "Claude Delta"]
    case .sessionsEmpty:
      [
        "Cursor Echo", "Codex Alpha", "Codex Beta", "Claude Gamma", "Claude Delta",
        "Session client provider",
      ]
    case .sessionsRestored:
      ["Session client provider"]
    case .sessionsAllBottom:
      ["Session client provider"]
    case .browse, .returned:
      ["Credit balance", "Reset credits"]
    case .resets:
      ["Credit balance"]
    case .credits, .cancelled, .allowDraft, .disallowDraft:
      []
    }
  }

  private var hiddenShortcutMarkers: [String] {
    ["Ctrl+", "ctrl-", "F1", "F2", "F3", "F4", "enter apply", "esc cancel"]
  }
}
