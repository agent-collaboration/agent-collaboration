/** Libtest discovery is strict: one selector, one actual test. */
export function requireOneTest(discovery: string, testName: string): void {
	const selected = discovery.split(/\r?\n/u).filter((line) => line.endsWith(': test'));
	if (selected.length !== 1 || selected[0] !== testName + ': test')
		throw new Error('exact capture test missing or broadened');
}
