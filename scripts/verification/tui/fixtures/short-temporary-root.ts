/** Short roots preserve the shared Unix socket-length bound on macOS and Linux. */
export function shortTemporaryRoot(): '/private/tmp' | '/tmp' {
	return process.platform === 'darwin' ? '/private/tmp' : '/tmp';
}
