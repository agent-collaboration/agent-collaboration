# Docs-only quota-stage control

This temporary draft pull request changes only this document. It is based on
the quota-stage implementation branch so its workflow contains the selector,
while its own diff contains no harness build or fixture inputs.

The control passes when both quota-reset build and test steps are skipped,
the workflow timing summary contains `quota-reset stage skipped: no quota changes`,
and the existing required validation gate succeeds. Workspace tests and all
other checks still run. Main pushes continue to run the quota stage unconditionally.

Close this control pull request without merging after recording those results.
The implementation pull request remains the only change intended for main.
