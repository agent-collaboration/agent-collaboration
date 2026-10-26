if (process.argv.includes("--graceful")) process.on("SIGTERM", (): never => process.exit(0));
if (process.argv.includes("--hold-until-signalled")) setInterval((): void => {}, 60_000);
process.stdout.write("READY\n");
process.stdin.resume();
