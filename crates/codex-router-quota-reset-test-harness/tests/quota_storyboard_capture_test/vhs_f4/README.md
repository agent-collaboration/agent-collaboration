# Native function keys in the private capture tool

The pinned VHS0.12.1 parser has no function-key command. `pinned-vhs-f4.patch`
adds F1, F4 and F5 at the existing token, parser and native-key dispatch
boundary: F1 opens Help, F4 opens Details and F5 opens Quota Pool details in the
capture tapes. It does not add another input transport or application shortcut.
The upstream source is `charmbracelet/vhs` commit
`24fa2254a9806091e6ee6a980e9f3bcfe0a9ba53`.

Make an isolated source archive in an owned system temporary task directory;
keep the OSS checkout and original VHS binary unchanged. Copy
`parser_regression_test.go` into that archive's `parser/` directory. The test
fails on the original source for each key, because each one is a STRING, before
applying the patch. Apply only this zero-context patch with
`git apply --unidiff-zero`, then run the parser, token and lexer packages. Build
with the archive's unchanged `go.mod` and `go.sum`, `-mod=readonly`, one compiler
job and private Go caches. Label the resulting tool `0.12.1+private-fkeys`, bind
its upstream/patch/module/binary hashes, and nominate it in a separate
capture-tools root with the verified ttyd and libwebsockets artifacts. Then check
that `vhs validate` accepts a tape that presses each of the three keys. This is a
private tool repair, not an upstream release or global installation.

Go-rod0.116.2 defines native F1, F4 and F5 as key/code F1/F4/F5 with virtual keys
112/115/116. The existing VHS ExecuteKey path sends that event to the browser;
textual escape-byte typing would depend on input timing and is not used.
Token/parser success cannot prove delivery: the permanent compiled-TUI capture
journeys require the actual Help, Details and Pool details images and the return
to the unchanged list. Mapping a key to Enter or a no-op cannot satisfy those
independent scene markers; Pool details must also reject an Account details image.

The observed Go dependency-fetch stall was a TCP dial wait, including after a
host grant; no TLS/auth cause was established. The bounded alternative fetched
only versions already listed in pinned `go.sum` through normally verified HTTPS
into a private file proxy. The build used that proxy for this command only,
kept checksum verification enabled and left both module files unchanged. No
global proxy, trust, account or app dependency setting changed.
