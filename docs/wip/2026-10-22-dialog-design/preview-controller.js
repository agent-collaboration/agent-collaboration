/* Owns preview state and sample-only effects. Views receive explicit data/callbacks. */
(() => {
  const { elementNode, uiText } = window.RouterTui;
  const { quotaScreen, sessionScreen, modalScreen } = window.RouterScreens;
  const host = document.getElementById('terminal-host');
  const caption = document.getElementById('study-caption');
  const accounts = window.RouterSamples.accounts.map(account => ({ ...account,
    status: account.status === 'Disabled' ? 'Available' : account.status }));
  const session = window.RouterSamples.session;
  const state = { view: 'quota', accounts, selectedId: 'ssdev', filters: { provider: 'All', showDisabled: true },
    modal: null, draftFilters: null, notice: null };
  let screenFrame = null, modalOverlay = null, noticeElement = null, returnFocus = null, noticeTimer = null;
  const selectedAccount = () => state.accounts.find(account => account.id === state.selectedId);
  const visibleAccounts = () => state.accounts.filter(account =>
    (state.filters.provider === 'All' || state.filters.provider === account.provider) && (state.filters.showDisabled || !account.disabled));

  function renderScreen() {
    const visible = visibleAccounts();
    if (!visible.some(account => account.id === state.selectedId)) state.selectedId = visible[0]?.id ?? null;
    host.replaceChildren(); modalOverlay = null; noticeElement = null;
    screenFrame = state.view === 'session' ? sessionScreen(session, dispatch) :
      quotaScreen({ ...state, visibleAccounts: visible }, dispatch);
    host.append(screenFrame);
    host.hidden = state.view === 'components';
    document.getElementById('component-notes').hidden = state.view !== 'components';
    document.querySelectorAll('[data-view]').forEach(button => {
      const selected = button.dataset.view === state.view;
      button.setAttribute('aria-selected', selected); button.tabIndex = selected ? 0 : -1;
    });
    host.setAttribute('aria-labelledby', state.view === 'session' ? 'tab-session' : 'tab-quota');
    caption.textContent = state.view === 'quota' ? 'Quota · sample data · ↑↓ select · / filters · enter details · a actions' :
      state.view === 'session' ? 'Session details · sample data · expand original text · fixed action footer' :
        'One component system · one terminal font size · rows and columns govern spacing';
    if (state.notice) renderNotice();
    if (state.modal) renderModal();
  }

  function renderNotice() {
    noticeElement?.remove();
    noticeElement = elementNode('div', 'tui-notice', [uiText(state.notice, 'teal')], { role: 'status' });
    screenFrame.querySelector('.tui-frame-interior').insertBefore(noticeElement, screenFrame.querySelector('.tui-footer'));
  }

  function notify(message) {
    state.notice = message; renderNotice(); clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => { state.notice = null; noticeElement?.remove(); noticeElement = null; }, 4000);
  }

  function openModal(kind) {
    if (!state.modal) returnFocus = document.activeElement;
    state.modal = kind;
    if (kind === 'filters') state.draftFilters = { ...state.filters };
    renderModal();
  }

  function renderModal() {
    modalOverlay?.remove(); screenFrame.inert = true;
    modalOverlay = elementNode('div', 'tui-overlay', [modalScreen({ kind: state.modal,
      account: selectedAccount(), accounts: state.accounts, draftFilters: state.draftFilters }, dispatch)]);
    host.append(modalOverlay);
    (state.modal === 'filters' ? modalOverlay.querySelector('input') : modalOverlay.querySelector('button'))?.focus();
  }

  function closeModal(restoreFocus = true) {
    state.modal = null; state.draftFilters = null; modalOverlay?.remove(); modalOverlay = null;
    if (screenFrame) screenFrame.inert = false;
    if (restoreFocus && returnFocus?.isConnected) returnFocus.focus();
    returnFocus = null;
  }

  function switchView(view) {
    closeModal(false); state.view = view; state.notice = null;
    history.replaceState(null, '', `#${view}`); renderScreen();
  }

  function selectAccount(id, scroll = false) {
    state.selectedId = id; renderScreen();
    const list = document.getElementById('quota-account-list'); list?.focus();
    if (scroll) document.getElementById(`account-${id}`)?.scrollIntoView({ block: 'nearest' });
  }

  function copySample(value, label) {
    closeModal();
    if (!navigator.clipboard?.writeText) { notify('Clipboard unavailable in this preview.'); return; }
    navigator.clipboard.writeText(value).then(() => notify(`Sample ${label} copied.`))
      .catch(() => notify('Clipboard unavailable in this preview.'));
  }

  function dispatch(name, payload) {
    switch (name) {
      case 'select-account': selectAccount(payload); break;
      case 'quota-view': switchView('quota'); break;
      case 'close-modal': closeModal(); break;
      case 'draft-provider': state.draftFilters.provider = payload; break;
      case 'draft-disabled': state.draftFilters.showDisabled = payload; break;
      case 'apply-filters': state.filters = { ...state.draftFilters }; closeModal(false); renderScreen(); document.getElementById('quota-account-list')?.focus(); break;
      case 'toggle-account': {
        const account = selectedAccount(); if (!account) break;
        account.disabled = !account.disabled; closeModal(false); renderScreen(); document.getElementById('quota-account-list')?.focus();
        notify(`Sample account ${account.disabled ? 'disabled' : 'enabled'}.`); break;
      }
      case 'confirm-remove': {
        const account = selectedAccount(); if (!account) break;
        state.accounts = state.accounts.filter(candidate => candidate.id !== account.id);
        closeModal(false); renderScreen(); document.getElementById('quota-account-list')?.focus();
        notify(`${account.name} removed from sample data. Reload restores it.`); break;
      }
      case 'copy-session': copySample(session.id, 'session ID'); break;
      case 'copy-path': copySample(session.directory, 'path'); break;
      case 'fork-session': notify('Fork selected · prototype only; no session started.'); break;
      case 'resume-session': notify('Resume selected · prototype only; no session started.'); break;
      default: openModal(name);
    }
  }

  document.querySelectorAll('[data-view]').forEach(button => button.addEventListener('click', () => switchView(button.dataset.view)));
  document.querySelector('.study-tabs').addEventListener('keydown', event => {
    if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return;
    const tabs = [...document.querySelectorAll('[data-view]')];
    const current = tabs.indexOf(event.target); if (current < 0) return;
    event.preventDefault();
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? tabs.length - 1 :
      (current + (event.key === 'ArrowRight' ? 1 : -1) + tabs.length) % tabs.length;
    switchView(tabs[next].dataset.view); tabs[next].focus();
  });
  document.getElementById('terminal-size').addEventListener('change', event => {
    const [columns, rows] = event.target.value.split('x');
    host.style.setProperty('--columns', columns); host.style.setProperty('--rows', rows);
  });

  document.addEventListener('keydown', event => {
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    if (state.modal) {
      if (event.key === 'Escape') { event.preventDefault(); if (state.modal === 'remove') openModal('actions'); else closeModal(); return; }
      if (event.key === 'Enter' && state.modal === 'filters' && event.target.matches('input[type="text"]')) {
        event.preventDefault(); dispatch('apply-filters'); return;
      }
      const modalShortcuts = state.modal === 'actions' ? { d: 'toggle-account' } :
        state.modal === 'account' ? { a: 'actions' } : state.modal === 'session-more' ? { c: 'copy-session', p: 'copy-path' } : {};
      if (modalShortcuts[event.key]) { event.preventDefault(); dispatch(modalShortcuts[event.key]); return; }
      if (event.key === 'Tab') {
        const focusable = [...modalOverlay.querySelectorAll('button:not(:disabled):not([tabindex="-1"]), input, summary')];
        const first = focusable[0], last = focusable.at(-1);
        if (event.shiftKey && (document.activeElement === first || !focusable.includes(document.activeElement))) { event.preventDefault(); last?.focus(); }
        else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); }
      }
      return;
    }
    if (event.target.closest('.study-header') || event.target.matches('input, select, summary, a, button')) return;
    if (state.view === 'quota') {
      const visible = visibleAccounts();
      if (['ArrowUp', 'ArrowDown'].includes(event.key) && visible.length) {
        event.preventDefault(); const index = visible.findIndex(account => account.id === state.selectedId);
        selectAccount(visible[(index + (event.key === 'ArrowDown' ? 1 : -1) + visible.length) % visible.length].id, true);
      } else if (event.key === 'Enter' && selectedAccount()) { event.preventDefault(); dispatch('account'); }
      else if (event.key.toLowerCase() === 'a' && selectedAccount()) { event.preventDefault(); dispatch('actions'); }
      else if (event.key === '/') { event.preventDefault(); dispatch('filters'); }
      else if (event.key === '?') { event.preventDefault(); dispatch('help'); }
    } else if (state.view === 'session') {
      const shortcuts = { Enter: 'resume-session', Escape: 'quota-view', f: 'fork-session', c: 'copy-session',
        p: 'copy-path', m: 'session-more', '?': 'session-help' };
      if (shortcuts[event.key]) { event.preventDefault(); dispatch(shortcuts[event.key]); }
    }
  });

  const initialView = location.hash.slice(1);
  if (['quota', 'session', 'components'].includes(initialView)) state.view = initialView;
  renderScreen();
})();
