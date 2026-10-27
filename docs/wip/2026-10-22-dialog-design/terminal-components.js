/* Presentation primitives mirror codex-router-tui. Product state stays in callers. */
(() => {
  function elementNode(tagName, className = '', children = [], attributes = {}) {
    const element = document.createElement(tagName);
    element.className = className;
    for (const [name, value] of Object.entries(attributes)) {
      if (value !== null && value !== undefined) element.setAttribute(name, String(value));
    }
    for (const child of children.flat(Infinity)) {
      if (child !== null && child !== undefined) element.append(child);
    }
    return element;
  }

  function uiText(content, role = 'body', bold = false) {
    return elementNode('span', `tui-role-${role}${bold ? ' tui-bold' : ''}`, [String(content)]);
  }

  function uiHeading(content, level = 'section') {
    return elementNode(level === 'surface' ? 'h2' : 'h3', `tui-heading ${level}`, [content]);
  }

  function textLines(children, className = '') {
    return elementNode('div', `tui-lines ${className}`, children);
  }

  function dialogAction({ label, shortcut, onActivate, emphasis = 'secondary', disabled = false, className = '' }) {
    const button = elementNode('button', `tui-action ${emphasis} ${className}`, [
      shortcut ? elementNode('span', 'key', [`[${shortcut}]`]) : null,
      elementNode('span', 'label', [label]),
    ], { type: 'button' });
    button.disabled = disabled;
    button.addEventListener('click', onActivate);
    return button;
  }

  function surfaceHeader({ title, metadata = [], actions = [] }) {
    return elementNode('header', 'tui-surface-header', [
      elementNode('div', 'heading', [uiHeading(title, 'surface')]),
      ...metadata,
      elementNode('div', 'tui-header-actions', actions),
    ]);
  }

  function horizontalRule() {
    return elementNode('div', 'tui-rule', ['─'.repeat(220)], { 'aria-hidden': 'true' });
  }

  function dialogActionBar(actions, context = null) {
    return elementNode('footer', 'tui-footer', [
      horizontalRule(),
      elementNode('div', 'tui-action-bar', [
        context ? elementNode('span', 'tui-action-context', [context]) : null,
        ...actions,
      ]),
    ]);
  }

  function dialogBodyViewport(children, attributes = {}) {
    return elementNode('div', 'tui-body-viewport', children, attributes);
  }

  function borderRow(top) {
    return elementNode('div', `tui-frame-border horizontal ${top ? 'top' : 'bottom'}`, [
      elementNode('span', '', [top ? '╭' : '╰']),
      elementNode('span', 'line', ['─'.repeat(220)]),
      elementNode('span', '', [top ? '╮' : '╯']),
    ], { 'aria-hidden': 'true' });
  }

  function dialogFrame({ title, metadata, headerActions, context, body, actions, actionContext, modal = false }) {
    const frame = elementNode('section', 'tui-frame', [
      borderRow(true),
      elementNode('span', 'tui-frame-border vertical left', ['│\n'.repeat(80)], { 'aria-hidden': 'true' }),
      elementNode('div', 'tui-frame-interior', [
        surfaceHeader({ title, metadata, actions: headerActions }),
        context ? elementNode('div', 'tui-context', [context]) : null,
        ...body,
        dialogActionBar(actions, actionContext),
      ]),
      elementNode('span', 'tui-frame-border vertical right', ['│\n'.repeat(80)], { 'aria-hidden': 'true' }),
      borderRow(false),
    ], { 'aria-label': title });
    if (modal) {
      frame.setAttribute('role', 'dialog');
      frame.setAttribute('aria-modal', 'true');
    }
    return frame;
  }

  function formField(label, content, role = 'body') {
    return elementNode('div', 'tui-form-field', [uiText(label, 'label'),
      typeof content === 'string' ? uiText(content, role) : content]);
  }

  function fieldList(fields) {
    return elementNode('div', 'tui-field-list', fields.map(field => formField(...field)));
  }

  function contentSection(title, children) {
    return elementNode('section', 'tui-section', [uiHeading(title), ...children]);
  }

  function selectableRow({ selected, content, onSelect, id, label, role = 'option', className = '' }) {
    const row = elementNode(role === 'radio' ? 'button' : 'div', `tui-selectable-row ${className}`, [
      elementNode('span', 'tui-selector', [selected ? '❯' : ' '], { 'aria-hidden': 'true' }),
      elementNode(role === 'radio' ? 'span' : 'div', 'tui-row-content', [content]),
    ], { role, id, 'aria-label': label, [role === 'radio' ? 'aria-checked' : 'aria-selected']: selected,
      tabindex: '-1' });
    row.addEventListener('click', onSelect);
    return row;
  }

  function quotaMeter(percent, colorRole = 'body') {
    const totalSegments = 6;
    const filledSegments = Math.round(percent * totalSegments / 100);
    return elementNode('span', 'quota-meter', [
      uiText('█'.repeat(filledSegments), colorRole), uiText('░'.repeat(totalSegments - filledSegments), 'secondary'),
    ], { 'aria-hidden': 'true' });
  }

  function disclosure(title, content) {
    return elementNode('details', 'tui-disclosure', [
      elementNode('summary', '', [title]),
      elementNode('div', 'tui-disclosure-body', [content]),
    ]);
  }

  function searchSelect({ label, options, value, onChange }) {
    let currentValue = value;
    const choiceList = elementNode('div', 'tui-choice-list', [], { role: 'radiogroup', 'aria-label': label });
    const input = elementNode('input', '', [], { type: 'text', 'aria-label': `Search ${label.toLowerCase()}`, autocomplete: 'off' });
    function renderChoices(focusValue = null) {
      const matching = options.filter(option => option.toLowerCase().includes(input.value.toLowerCase()));
      choiceList.replaceChildren(...matching.map(option => selectableRow({
        role: 'radio', selected: currentValue === option, content: uiText(option), label: option,
        onSelect: () => { currentValue = option; onChange(option); renderChoices(option); },
      })));
      if (!matching.length) choiceList.append(uiText('No matching providers', 'secondary'));
      if (focusValue) [...choiceList.querySelectorAll('button')].find(button => button.getAttribute('aria-label') === focusValue)?.focus();
    }
    input.addEventListener('input', () => renderChoices());
    const root = elementNode('section', 'tui-search-select', [
      uiHeading(label),
      elementNode('div', 'search-line', [uiText('Search:', 'label'), input]), choiceList,
    ]);
    root.addEventListener('keydown', event => {
      if (!['ArrowDown', 'ArrowUp'].includes(event.key)) return;
      const choices = [...choiceList.querySelectorAll('button')];
      if (!choices.length) return;
      event.preventDefault();
      const index = choices.indexOf(document.activeElement);
      const next = index < 0 ? 0 : (index + (event.key === 'ArrowDown' ? 1 : -1) + choices.length) % choices.length;
      currentValue = choices[next].getAttribute('aria-label'); onChange(currentValue); renderChoices(currentValue);
    });
    renderChoices();
    return root;
  }

  window.RouterTui = Object.freeze({ elementNode, uiText, uiHeading, textLines, dialogAction,
    surfaceHeader, horizontalRule, dialogActionBar, dialogBodyViewport, dialogFrame,
    formField, fieldList, contentSection, selectableRow, quotaMeter, disclosure, searchSelect });
})();
