if (process.argv.includes("--graceful")) process.on("SIGTERM", (): never => process.exit(0));
process.stdout.write("READY\n");
process.stdin.resume();
