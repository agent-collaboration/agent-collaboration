/** Count actual native turn starts, including those caused by automatic notifications. */
import type { NativeEvent } from "../clients/router-native.ts";

export function nativeTurnStarts(connections: readonly (readonly NativeEvent[])[]): number {
  return connections.reduce(
    (count, events): number =>
      count + events.filter((event): boolean => event.method === "turn/started").length,
    0,
  );
}
