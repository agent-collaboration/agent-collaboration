import type { Receipt } from "./receipt.ts";

export class RunLifetime {
  private readonly resources: { name: string; resource: { close(): Promise<void> } }[] = [];
  constructor(private readonly receipt: Receipt) {}
  own<TResource extends { close(): Promise<void> }>(name: string, resource: TResource): TResource {
    this.resources.push({ name, resource });
    return resource;
  }
  afterDependencies(resource: { close(): Promise<void> }): void {
    const index = this.resources.findIndex((entry): boolean => entry.resource === resource);
    if (index < 0) throw new Error("runtime owner was not registered");
    const entry = this.resources.splice(index, 1)[0];
    if (entry === undefined) throw new Error("runtime owner disappeared");
    this.resources.push(entry);
  }
  async close(): Promise<readonly string[]> {
    const failures: string[] = [];
    for (const resource of this.resources.splice(0).reverse()) {
      try {
        await resource.resource.close();
      } catch {
        failures.push(resource.name);
      }
    }
    await this.receipt.record("runCleanup", {
      failedOwners: failures,
      complete: failures.length === 0,
    });
    return failures;
  }
}
