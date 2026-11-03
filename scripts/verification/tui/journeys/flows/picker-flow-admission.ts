/** Missing fixture contracts reject before any product process or evidence directory. */
import type { FlowAdmissionGap, FlowInvocation } from './flow-executor.ts';
import { admitFlow } from './flow-executor.ts';
import type { NativeFlowBindings } from './native-flow-runtime.ts';
import { nativeFlowCapabilities } from './native-flow-runtime.ts';

export function pickerFlowAdmission(
	invocation: FlowInvocation,
	bindings: NativeFlowBindings | null,
): readonly FlowAdmissionGap[] {
	if (bindings !== null) return admitFlow(invocation, nativeFlowCapabilities(bindings));
	const name = invocation.journey.journeyId;
	if (name === 'same-id-hosted-codex-fork')
		return [
			{
				kind: 'fixture',
				name: 'fork-ssh-profile: qualified SSH same-ID HostedCodex fixture not declared',
			},
		];
	if (name === 'sessions-toolbar-and-menu')
		return [
			{
				kind: 'fixture',
				name: 'claude-new-success: successful Claude New inventory and both run modes not declared',
			},
		];
	if (name === 'picker-list-option-navigation' || name === 'sessions-help-option-navigation')
		return [
			{
				kind: 'fixture',
				name: 'large-list: independent order and destinations beyond two viewport pages not declared',
			},
		];
	return [{ kind: 'fixture', name: 'no native adapter: ' + name }];
}
