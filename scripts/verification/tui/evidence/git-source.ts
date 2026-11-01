import { execFileSync } from 'node:child_process';
export function gitOutput(repository: string, argumentsList: readonly string[]): string {
	return execFileSync('git', ['-C', repository, ...argumentsList], { encoding: 'utf8' });
}
