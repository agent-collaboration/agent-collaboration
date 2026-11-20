# Fleet engine tests

Colocated unit cases cover attempt decoding, guarded attempt writes and request
receipt replay. The guarded writer's wake-backed case uses the real central
schema to check that recording a failure cannot retrigger or resurrect the
subject. The native and Worker host integration suites exercise the engine
through completion, alarms, request and admin entry points.
