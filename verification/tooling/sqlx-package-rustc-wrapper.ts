#!/usr/bin/env node
import { executeCompiler } from "./sqlx-compiler-environment.ts";
process.exitCode = executeCompiler("package");
