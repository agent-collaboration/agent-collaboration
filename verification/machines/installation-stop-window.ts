/** An upgrade observer runs only after owned old children have settled; failure aborts startup. */
export async function observeInstallationStopWindow(input: {
  stopOwnedChildren: () => Promise<void>;
  observeStoppedOld?: () => Promise<void>;
}): Promise<void> {
  await input.stopOwnedChildren();
  await input.observeStoppedOld?.();
}
