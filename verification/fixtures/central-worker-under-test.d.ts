/** Capabilities used by the debug wrapper around the generated Rust module. */
declare module "central-worker-under-test" {
  export interface WorkerStorage {
    get(key: string): Promise<unknown>;
    put(key: string, value: unknown): Promise<void>;
    delete(key: string): Promise<boolean>;
    getAlarm(): Promise<number | null>;
    setAlarm(at: Date | number): Promise<void>;
    deleteAlarm(): Promise<void>;
  }
  export interface WorkerState {
    readonly storage: WorkerStorage;
  }
  export class FleetObject {
    constructor(state: WorkerState, environment: unknown);
    fetch(request: Request): Promise<Response>;
    alarm(): Promise<void>;
  }
  const worker: unknown;
  export default worker;
  export const BootstrapObject: unknown;
  export const ConnectionObject: unknown;
  export const LeaseRegistryObject: unknown;
  export const MailboxObject: unknown;
  export const ProjectCopyObject: unknown;
}
