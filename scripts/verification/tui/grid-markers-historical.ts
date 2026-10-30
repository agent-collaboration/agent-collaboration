/** Literal adopted scene markers; spatial obligations are checked separately. */
import type { SceneMarkerContract } from './grid-scene-contract.ts';
export const historicalMarkers = {
	'sessions-details': {
		required: ['Session details', 'Codex Alpha', 'Session ID', 'Conversation', 'Close'],
		forbidden: ['Draft:', 'Apply', 'Cancel'],
	},
	'sessions-details-closed': {
		required: ['Codex', '2/6', 'Codex Alpha', 'Details F4'],
		forbidden: ['Session details', 'Account details', 'Draft:'],
	},
	'quota-details': {
		required: ['Account details', 'Claude Gamma', 'Status', 'Reason', 'Close'],
		forbidden: ['Draft:', 'Apply', 'Cancel'],
	},
	'quota-details-closed': {
		required: ['Claude', 'Disabled: Hidden', '1/4 accounts', 'Details F4'],
		forbidden: ['Session details', 'Account details', 'Draft:'],
	},
	'quota-all-top': {
		required: ['Quota status', 'All', '4/4 accounts', 'Claude Delta'],
		forbidden: ['Account provider'],
	},
	'quota-all-next': {
		required: ['Quota status', 'All', '4/4 accounts', 'Claude Gamma'],
		forbidden: ['Account provider'],
	},
	'quota-openai-bottom': {
		required: ['Quota status', 'OpenAI', '2/4 accounts', 'OpenAI Beta'],
		forbidden: ['Claude Gamma', 'Claude Delta', 'Account provider'],
	},
	'quota-claude-bottom': {
		required: ['Quota status', 'Claude', '2/4 accounts', 'Claude Gamma'],
		forbidden: ['OpenAI Alpha', 'OpenAI Beta', 'Account provider'],
	},
	'quota-all': {
		required: ['Quota status', 'All', 'Disabled: Shown', '4/4 accounts'],
		forbidden: ['Account provider'],
	},
	'quota-chooser': {
		required: [
			'Quota status',
			'Account provider',
			'Draft: All',
			'Disabled accounts: Shown',
			'Apply',
			'Cancel',
		],
		forbidden: [],
	},
	'quota-openai': {
		required: ['Quota status', 'OpenAI', '2/4 accounts', 'OpenAI Alpha'],
		forbidden: ['Claude Gamma', 'Claude Delta', 'Account provider'],
	},
	'quota-claude': {
		required: ['Quota status', 'Claude', '2/4 accounts', 'Claude Delta'],
		forbidden: ['OpenAI Alpha', 'OpenAI Beta', 'Account provider'],
	},
	'quota-hidden': {
		required: ['Quota status', 'Claude', 'Disabled: Hidden', '1/4 accounts', 'Claude Gamma'],
		forbidden: ['Claude Delta', 'OpenAI Alpha', 'OpenAI Beta', 'Account provider'],
	},
	'quota-draft': {
		required: [
			'Quota status',
			'Claude',
			'Disabled: Hidden',
			'1/4 accounts',
			'Draft: OpenAI',
			'Disabled accounts: Hidden',
			'Apply',
			'Cancel',
		],
		forbidden: ['Claude Delta'],
	},
	'quota-cancelled': {
		required: ['Quota status', 'Claude', 'Disabled: Hidden', '1/4 accounts', 'Claude Gamma'],
		forbidden: ['Account provider', 'Claude Delta', 'OpenAI Alpha', 'OpenAI Beta'],
	},
	'quota-restored': {
		required: ['Quota status', 'All', 'Disabled: Shown', '4/4 accounts'],
		forbidden: ['Account provider'],
	},
	'quota-all-bottom': {
		required: ['Quota status', 'All', '4/4 accounts', 'OpenAI Beta'],
		forbidden: ['Account provider'],
	},
	'quota-empty': {
		required: [
			'Quota status',
			'Claude',
			'Disabled: Hidden',
			'0/4 accounts',
			'No accounts match these filters',
		],
		forbidden: ['Claude Gamma', 'Claude Delta', 'OpenAI Alpha', 'OpenAI Beta', 'Account provider'],
	},
	'sessions-codex-bottom': {
		required: ['Sessions', 'Codex', '2/6', 'Codex Beta'],
		forbidden: [
			'Claude Gamma',
			'Claude Delta',
			'Cursor Echo',
			'Claude-lookalike Zeta',
			'Session client provider',
		],
	},
	'sessions-claude-bottom': {
		required: ['Sessions', 'Claude Code', '2/6', 'Claude Delta'],
		forbidden: [
			'Codex Alpha',
			'Codex Beta',
			'Cursor Echo',
			'Claude-lookalike Zeta',
			'Session client provider',
		],
	},
	'sessions-new-choice': {
		required: ['Sessions', 'Cursor', '1/6', 'New Codex session'],
		forbidden: [
			'Session client provider',
			'Codex Alpha',
			'Codex Beta',
			'Claude Gamma',
			'Claude Delta',
		],
	},
	'sessions-all': {
		required: ['Sessions', 'All providers', '6/6'],
		forbidden: ['Session client provider'],
	},
	'sessions-chooser': {
		required: [
			'Session client provider',
			'Draft: All providers',
			'Apply',
			'Cancel',
			'Claude Code',
			'Cursor',
		],
		forbidden: [],
	},
	'sessions-codex': {
		required: ['Sessions', 'Codex', '2/6', 'Codex Alpha'],
		forbidden: [
			'Claude Gamma',
			'Claude Delta',
			'Cursor Echo',
			'Claude-lookalike Zeta',
			'Session client provider',
		],
	},
	'sessions-claude': {
		required: ['Sessions', 'Claude Code', '2/6', 'Claude Gamma'],
		forbidden: [
			'Codex Alpha',
			'Codex Beta',
			'Cursor Echo',
			'Claude-lookalike Zeta',
			'Session client provider',
		],
	},
	'sessions-cursor': {
		required: ['Sessions', 'Cursor', '1/6', 'Cursor Echo'],
		forbidden: [
			'Codex Alpha',
			'Codex Beta',
			'Claude Gamma',
			'Claude Delta',
			'Claude-lookalike Zeta',
			'Session client provider',
		],
	},
	'sessions-draft': {
		required: ['Session client provider', 'Draft: Codex', 'Cursor', '1/6', 'Apply', 'Cancel'],
		forbidden: ['Codex Alpha', 'Codex Beta', 'Claude Gamma', 'Claude Delta'],
	},
	'sessions-cancelled': {
		required: ['Sessions', 'Cursor', '1/6', 'Cursor Echo'],
		forbidden: [
			'Session client provider',
			'Codex Alpha',
			'Codex Beta',
			'Claude Gamma',
			'Claude Delta',
		],
	},
	'sessions-empty': {
		required: ['Sessions', 'Cursor', '0/6', 'New Codex session'],
		forbidden: [
			'Cursor Echo',
			'Codex Alpha',
			'Codex Beta',
			'Claude Gamma',
			'Claude Delta',
			'Session client provider',
		],
	},
	'sessions-restored': {
		required: ['Sessions', 'All providers', '6/6'],
		forbidden: ['Session client provider'],
	},
	'sessions-all-bottom': {
		required: ['Sessions', 'All providers', '6/6', 'Claude-lookalike Zeta'],
		forbidden: ['Session client provider'],
	},
	browse: {
		required: ['Quota status', 'pty-alpha', 'account options'],
		forbidden: ['Credit balance', 'Reset credits'],
	},
	returned: {
		required: ['Quota status', 'pty-alpha', 'account options'],
		forbidden: ['Credit balance', 'Reset credits'],
	},
	resets: {
		required: ['Quota status', 'Reset credits', 'Weekly remaining'],
		forbidden: ['Credit balance'],
	},
	credits: {
		required: ['Quota status', 'Credit balance', '42.00', 'Disallow', 'enter edit'],
		forbidden: [],
	},
	cancelled: {
		required: ['Quota status', 'Credit balance', '42.00', 'Disallow', 'enter edit'],
		forbidden: [],
	},
	'allow-draft': {
		required: ['Quota status', 'Credit usage', '42.00', '› Allow', 'enter save'],
		forbidden: [],
	},
	'disallow-draft': {
		required: ['Quota status', 'Credit usage', '42.00', '› Disallow', 'enter save'],
		forbidden: [],
	},
} satisfies Readonly<Record<string, SceneMarkerContract>>;
