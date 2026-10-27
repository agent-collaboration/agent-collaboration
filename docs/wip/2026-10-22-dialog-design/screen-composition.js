/* Screens compose shared presentation primitives; state and effects are passed in. */
(() => {
  const { elementNode, uiText, uiHeading, textLines, dialogAction, dialogFrame, horizontalRule,
    dialogBodyViewport, fieldList, contentSection, selectableRow, quotaMeter, disclosure,
    searchSelect } = window.RouterTui;
  const action = (label, shortcut, name, dispatch, emphasis = 'secondary', className = '') =>
    dialogAction({ label, shortcut, onActivate: () => dispatch(name), emphasis, className });
  const statusLabel = account => account.disabled ? 'Disabled' : account.status;
  const statusTone = account => account.disabled ? 'secondary' : account.tone;
  const quotaSummary = (percent, reset) => percent === null ? 'No data reported' :
    `${percent}% left · ${reset ? `resets in ${reset}` : 'reset unknown'}`;

  function quotaWindow(percent, reset, title, stale, colorRole = 'body') {
    return elementNode('div', 'quota-window-line', [
      percent === null ? uiText('No data', 'secondary') :
        elementNode('span', 'quota-value', [quotaMeter(percent, colorRole), uiText(`${percent}%`, colorRole)]),
      uiText('·', 'secondary'), uiText(title, colorRole), uiText('·', 'secondary'),
      uiText(reset ?? '—', reset ? colorRole : 'secondary'), stale ? uiText('stale', 'warning') : null,
    ], { 'aria-label': `${title}: ${percent === null ? 'no data' : percent + '% left'}, ${reset ? 'resets in ' + reset : 'reset unknown'}${stale ? ', stale observation' : ''}` });
  }

  function quotaAccountRow(account, selected, dispatch) {
    const marker = account.disabled ? '⊘' : account.status === 'Preferred' ? '★' :
      account.stale ? '◷' : account.status === 'Held' ? '◇' : account.status === 'Data unknown' ? '?' : '✓';
    return selectableRow({
      id: `account-${account.id}`, selected, label: `${account.name}, ${statusLabel(account)}, ${quotaSummary(account.weekly, account.reset)}`,
      className: 'quota-row', onSelect: () => dispatch('select-account', account.id),
      content: elementNode('div', 'quota-columns', [
        textLines([
          elementNode('div', 'account-name', [account.name]),
          uiText(`${account.provider === 'Claude' ? '◆' : '◎'} ${account.provider} · ${account.clients} ${account.clients === 1 ? 'client' : 'clients'}`, 'secondary'),
          uiText(`${marker} ${statusLabel(account)}`, statusTone(account)),
        ], 'account-cell'),
        elementNode('div', 'quota-window-group', [
          elementNode('div', 'quota-window-label', ['QUOTA LEFT · RESET IN']),
          quotaWindow(account.weekly, account.reset, 'week', account.stale),
          quotaWindow(account.short, account.shortReset, '5h', account.stale, 'short-window'),
        ]),
        textLines([
          elementNode('div', 'quota-window-label', ['WEEKLY PACE']),
          uiText(account.disabled ? '—' : account.pace, account.disabled ? 'secondary' : account.paceTone, true),
          uiText(account.disabled ? 'Excluded' : account.multiple ? `${account.multiple} reset pace` : 'Forecast unavailable', 'secondary'),
        ], 'quota-pace'),
        textLines([
          elementNode('div', 'quota-window-label', ['EXTRA']),
          uiText(account.credits ? `${account.credits} credits${account.stale ? ' · stale' : ''}` : 'Not reported', account.stale ? 'warning' : 'secondary'),
          uiText(account.resets === null ? '' : `${account.resets} resets`, 'secondary'),
        ], 'quota-extra'),
      ]),
    });
  }

  function quotaScreen({ accounts, visibleAccounts, filters, selectedId }, dispatch) {
    const selectedAccount = accounts.find(account => account.id === selectedId);
    const accountList = dialogBodyViewport(visibleAccounts.length ?
      visibleAccounts.map(account => quotaAccountRow(account, account.id === selectedId, dispatch)) :
      [uiText('No matching accounts. Change Filters to see more.', 'secondary')],
    { role: 'listbox', 'aria-label': 'Quota accounts', tabindex: '0', id: 'quota-account-list',
      'aria-activedescendant': selectedId ? `account-${selectedId}` : null });
    const details = action('Details', 'enter', 'account', dispatch, 'primary');
    const accountActions = action('Account actions', 'a', 'actions', dispatch);
    details.disabled = accountActions.disabled = !selectedAccount;
    return dialogFrame({
      title: 'Quota',
      metadata: [elementNode('span', 'client-census', [uiText(`⇄ ${accounts.reduce((sum, account) => sum + account.clients, 0)} clients`, 'secondary')])],
      headerActions: [action('◉ Pool details', null, 'pool', dispatch), action('Help', '?', 'help', dispatch)],
      body: [
        elementNode('div', 'quota-toolbar', [action('Filters', '/', 'filters', dispatch),
          uiText(`${filters.provider === 'All' ? 'All providers' : filters.provider} · Disabled ${filters.showDisabled ? 'shown' : 'hidden'}`, 'secondary'),
          elementNode('span', 'count', [uiText(`${visibleAccounts.length}/${accounts.length} accounts`, 'secondary')])]),
        elementNode('div', 'quota-table-header', [
          elementNode('div', 'quota-heading quota-columns', ['ACCOUNT', 'QUOTA LEFT · RESET IN', 'WEEKLY PACE', 'EXTRA']),
          horizontalRule(),
        ], { 'aria-hidden': 'true' }),
        accountList,
      ],
      actions: [details, accountActions], actionContext: `↑↓ ${selectedAccount?.name ?? 'No account selected'}`,
    });
  }

  function sessionScreen(session, dispatch) {
    return dialogFrame({
      title: 'Session details', headerActions: [action('Help', '?', 'session-help', dispatch)],
      context: elementNode('div', '', [
        elementNode('div', 'session-title', [session.title]),
        elementNode('div', 'tui-status-line', [uiText(`◎ ${session.client}`, 'teal'), uiText(`● ${session.status}`, 'success'),
          uiText(session.machine, 'secondary'), uiText(`Updated ${session.updated}`, 'secondary')]),
      ]),
      body: [dialogBodyViewport([elementNode('div', 'tui-body-content', [
        fieldList([['Session ID', session.id, 'secondary'], ['Directory', session.directory], ['Endpoint', session.endpoint]]),
        contentSection('Original title', [
          uiText("You're the Lead for the ACP metadata presentation fix, in a fresh main conversation."),
          disclosure('Show full original title', uiText(session.originalTitle, 'secondary')),
        ]),
        contentSection('Conversation preview', [textLines([
          uiText('Assistant · latest excerpt', 'secondary'), uiText(session.preview),
        ])]),
        disclosure('Source and identity details', fieldList([
          ['Client', session.client], ['Machine', session.machine], ['State', session.status, 'success'],
          ['Identity', 'Native session · local endpoint', 'secondary'],
        ])),
      ])])],
      actions: [action('Close', 'esc', 'quota-view', dispatch),
        action('Copy ID', 'c', 'copy-session', dispatch, 'secondary', 'session-copy'),
        action('Copy path', 'p', 'copy-path', dispatch, 'secondary', 'session-copy'),
        action('More', 'm', 'session-more', dispatch, 'secondary', 'session-more'),
        action('Fork', 'f', 'fork-session', dispatch, 'primary'),
        action('Resume', 'enter', 'resume-session', dispatch, 'commit')],
    });
  }

  function accountDetails(account) {
    return [uiText(account.name, 'body', true),
      uiText(`${account.provider} · ${account.clients} clients`, 'secondary'),
      contentSection('Quota & selection', [fieldList([
        ['Status', statusLabel(account), statusTone(account)], ['Reason', account.disabled ? 'Disabled by the owner; excluded from routing.' : account.reason],
        ['Weekly', quotaSummary(account.weekly, account.reset)], ['5-hour', quotaSummary(account.short, account.shortReset)],
        ['Weekly pace', account.pace, account.paceTone],
      ])]),
      contentSection('Balance & observation', [fieldList([
        ['Credit balance', account.credits ? `${account.credits} credits` : 'Unknown / not reported'],
        ['Reset inventory', account.resets === null ? 'Unknown' : `${account.resets} resets`],
        ['Observed', account.age, account.stale ? 'warning' : 'secondary'],
      ])]),
    ];
  }

  function poolDetails(accounts) {
    const count = predicate => accounts.filter(predicate).length;
    return [uiText('Pool-wide facts · independent of list filters', 'secondary'),
      fieldList([['Routing band', 'Responses'], ['Accounts', String(accounts.length)],
        ['Serving clients', String(accounts.reduce((sum, account) => sum + account.clients, 0))]]),
      contentSection('Routing availability', [fieldList([
        ['✓ Usable', String(count(account => !account.disabled && ['Available', 'Preferred'].includes(account.status)))],
        ['◇ Reserve', String(count(account => !account.disabled && account.status === 'Held'))],
        ['⊘ Blocked', '0'], ['? Unknown', String(count(account => !account.disabled && (account.status === 'Data unknown' || account.stale)))],
        ['× Excluded', String(count(account => account.disabled))],
      ])]),
      contentSection('Observation quality', [fieldList([
        ['● Recent', String(count(account => account.age !== 'Unknown' && !account.stale))],
        ['◷ Stale', String(count(account => account.stale))], ['? Unknown', String(count(account => account.age === 'Unknown'))],
      ]), uiText('Availability and observation age describe different facts.', 'secondary')]),
    ];
  }

  function modalScreen({ kind, account, accounts, draftFilters }, dispatch) {
    let title, content, actions = [action('Back', 'esc', 'close-modal', dispatch)];
    if (kind === 'filters') {
      title = 'Account filters';
      const toggle = elementNode('input', '', [], { type: 'checkbox', id: 'show-disabled' });
      toggle.checked = draftFilters.showDisabled;
      toggle.addEventListener('change', () => dispatch('draft-disabled', toggle.checked));
      content = [searchSelect({ label: 'Account provider', options: ['All', 'OpenAI', 'Claude'], value: draftFilters.provider,
        onChange: provider => dispatch('draft-provider', provider) }),
      elementNode('label', 'tui-toggle', [toggle, uiText('Show disabled accounts')], { for: 'show-disabled' })];
      actions = [action('Cancel', 'esc', 'close-modal', dispatch), action('Apply filters', 'enter', 'apply-filters', dispatch, 'commit')];
    } else if (kind === 'account' && account) {
      title = 'Account details'; content = accountDetails(account);
      actions.push(action('Account actions', 'a', 'actions', dispatch, 'primary'));
    } else if (kind === 'pool') {
      title = 'Pool details'; content = poolDetails(accounts);
    } else if (kind === 'actions' && account) {
      title = 'Account actions'; content = [uiText(account.name, 'body', true),
        uiText(`${account.provider} · ${statusLabel(account)}`, 'secondary'),
        contentSection('Manage account', [elementNode('div', 'tui-command-list', [
          action('Account details', 'enter', 'account', dispatch, 'primary'),
          action(account.disabled ? 'Enable account' : 'Disable account', 'd', 'toggle-account', dispatch, 'primary'),
          action('× Remove account…', null, 'remove', dispatch, 'danger'),
        ])])];
    } else if (kind === 'remove' && account) {
      title = 'Remove account'; content = textLines([uiText(account.name, 'body', true),
        uiText('Remove this account from Router?'), uiText('Disable keeps it for later. Remove takes it out of the list.', 'secondary'),
        account.clients ? uiText(`⇄ ${account.clients} clients currently use this account.`, 'warning') : null]);
      actions = [action('Keep account', 'esc', 'actions', dispatch), action('Remove account', 'enter', 'confirm-remove', dispatch, 'danger')];
    } else if (kind === 'session-more') {
      title = 'Session actions'; content = [elementNode('div', 'tui-command-list', [
        action('Copy session ID', 'c', 'copy-session', dispatch, 'primary'), action('Copy path', 'p', 'copy-path', dispatch, 'primary'),
      ])];
    } else {
      const sessionHelp = kind === 'session-help'; title = sessionHelp ? 'Session help' : 'Quota help';
      content = [fieldList(sessionHelp ? [['Enter', 'Resume session'], ['F', 'Fork session'], ['C / P', 'Copy ID / path'], ['M', 'More actions'], ['Esc', 'Close details']] :
        [['↑ / ↓', 'Select account'], ['Enter', 'Account details'], ['A', 'Account actions'], ['/', 'Filters'], ['Esc', 'Close current dialog']]),
      contentSection(sessionHelp ? 'Inspect the session' : 'Reading quota', [uiText(sessionHelp ?
        'Scroll the body to read all content. Actions stay in the footer.' :
        'Quota lines read: bar and % left · window · time until reset. A dash means reset time unknown. 1× weekly pace matches the budget until reset. Missing data gets no quota bar.', 'secondary')])];
    }
    return dialogFrame({ title, modal: true, body: [dialogBodyViewport([
      elementNode('div', 'tui-body-content', Array.isArray(content) ? content : [content]),
    ])], actions });
  }

  window.RouterScreens = Object.freeze({ quotaScreen, sessionScreen, modalScreen });
})();
