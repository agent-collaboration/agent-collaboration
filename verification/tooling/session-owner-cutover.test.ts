import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

function rustSource(path: string): string {
  return readFileSync(new URL(`../../crates/${path}`, import.meta.url), "utf8");
}

function rustFunction(path: string, name: string): string {
  const source = rustSource(path);
  const start = source.indexOf(`fn ${name}(`);
  assert.ok(start >= 0, `${path} must still test ${name}`);
  const end = source.indexOf("\n}\n", start);
  assert.ok(end > start, `${path} must contain the whole function`);
  return source.slice(start, end);
}

test("the retired provider ACP steer wrapper is absent", (): void => {
  assert.doesNotMatch(
    rustSource("codex-router-host/src/provider_acp_delivery_route/provider_content_commands.rs"),
    /\bfn steer_contents\s*\(/,
  );
});

test("history invalidation has no direct hub entry point", (): void => {
  assert.doesNotMatch(
    rustSource("collaboration-runtime/src/provider_session_event_hub.rs"),
    /\bfn begin_history_unavailable\s*\(/,
  );
});

test("provider retirement and saturated shutdown use fixture evidence for their waits", (): void => {
  const fixtures: ReadonlyArray<readonly [string, string]> = [
    [
      "codex-router-host/src/external_provider_supervisor/supervisor_test.rs",
      "provider_retirement_settles_queued_input_without_resubmission",
    ],
    ...["supervisor_shutdown_tests", "lifecycle_tests"].map(
      (module: string): readonly [string, string] => [
        `codex-router-host/tests/external_provider_supervisor/${module}.rs`,
        "supervisor_shutdown_drains_saturated_load_completions_and_preserves_unknown_effect",
      ],
    ),
    [
      "codex-router-host/tests/integration/external_provider_supervisor.rs",
      "assert_process_reaped",
    ],
    [
      "codex-router-host/tests/integration/external_provider_supervisor.rs",
      "wait_for_fixture_loads",
    ],
  ];
  for (const [path, name] of fixtures) {
    assert.doesNotMatch(
      rustFunction(path, name),
      /\b(?:tokio::time|std::thread)::(?:timeout|sleep)\b/,
      `${name} should wait on the fixture's evidence`,
    );
  }
});
