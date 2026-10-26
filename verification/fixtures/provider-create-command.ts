#!/usr/bin/env node
import { writeFileSync } from "node:fs";

if (process.env.JOURNEY_CLI_RECEIPT !== undefined)
  writeFileSync(process.env.JOURNEY_CLI_RECEIPT, JSON.stringify(process.argv.slice(2)));
console.log('{"kind":"created","note":"synthetic command fixture"}');
